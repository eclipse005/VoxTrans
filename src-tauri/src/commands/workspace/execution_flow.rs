use tauri::{AppHandle, Manager};
use tauri::async_runtime::spawn_blocking;
use tokio::runtime::Handle;

use crate::db::store::TaskStore;
use crate::services::pipeline::StepContext;

use crate::domain::error::{WorkspaceError, WorkspaceResult};
use crate::domain::task::adapters::{
    source_text_from_step2_segments, step2_segments_to_srt,
    workspace_subtitle_segments_from_step2_segments,
};
use crate::domain::task::runtime_settings::resolve_runtime_settings;

use super::output_completion::finish_transcribe_only;
use super::pipeline_runner::execute_workspace_step;
use super::pipeline_steps::{Step1AsrPipelineStep, Step2SegmentsPipelineStep};
use super::preview::update_subtitle_preview;
use super::progress::{mark_task_failed, report_task_stage};
use super::translation_flow::execute_translate_steps;
use super::{
    TaskStage, get_task_record, normalize_intent, normalize_task_source_lang,
    normalize_task_target_lang, patch_task_item,
};

pub(super) async fn execute_single_task(app: &AppHandle, task_id: &str) -> WorkspaceResult<()> {
    let result = execute_single_task_inner(app, task_id).await;
    mark_task_failed_after_execution_error(result, |err| {
        let err_string = err.to_string();
        let task_id = task_id.to_string();
        let app = app.clone();
        async move { mark_task_failed(&app, &task_id, &err_string).await }
    })
    .await
}

async fn execute_single_task_inner(app: &AppHandle, task_id: &str) -> WorkspaceResult<()> {
    let record = get_task_record(task_id)?;
    let intent = normalize_intent(&record.intent).to_string();
    let store = app.state::<TaskStore>().inner();
    let runtime =
        resolve_runtime_settings(store, &record.frozen, intent == "TRANSCRIBE_TRANSLATE")?;
    let mut source_lang = normalize_task_source_lang(&record.source_lang);
    let target_lang = normalize_task_target_lang(&record.target_lang);
    let step_context = StepContext { task_id, store };

    report_task_stage(app, task_id, TaskStage::Preparing, "", 1, 1).await?;
    patch_task_item(app, task_id, |task| {
        task.item.transcribe_error = String::new();
        task.item.result_text = String::new();
        task.item.result_srt = String::new();
        task.item.subtitle_segments_json = "[]".to_string();
    })
    .await?;

    let asr_input_path = if runtime.enable_vocal_separation {
        separate_vocals_for_task(
            app,
            task_id,
            &record.item.path,
            &runtime.demucs_model,
        )
        .await?
    } else {
        record.item.path.clone()
    };

    let step1_exec = execute_workspace_step(
        &Step1AsrPipelineStep {
            task_id: task_id.to_string(),
            media_path: asr_input_path,
            source_lang: source_lang.clone(),
            asr_model: runtime.asr_model.clone(),
            align_model: runtime.align_model.clone(),
            provider: runtime.provider.clone(),
            chunk_target_seconds: runtime.chunk_target_seconds,
            app: app.clone(),
        },
        &step_context,
        store,
    )
    .await?;

    if !step1_exec.output.source_lang.trim().is_empty() {
        source_lang = step1_exec.output.source_lang.clone();
    }

    let step2_words = step1_exec
        .output
        .words
        .iter()
        .map(|word| crate::commands::transcription::WordTokenCommandDto {
            start: word.start,
            end: word.end,
            word: word.word.clone(),
        })
        .collect::<Vec<_>>();

    report_task_stage(app, task_id, TaskStage::Segmenting, "", 0, 0).await?;

    let step2_exec = execute_workspace_step(
        &Step2SegmentsPipelineStep {
            task_id: task_id.to_string(),
            media_path: record.item.path.clone(),
            source_lang: source_lang.clone(),
            subtitle_length_preset: runtime.subtitle_length_preset.clone(),
            // DP length-split is now always on. Previously it was disabled for
            // TRANSLATE mode because step5 (LLM split/align) handled long lines
            // downstream; step5 has been removed, so segmentation must produce
            // already-length-correct rows for the 1:1 translation.
            use_subtitle_layout_split: true,
            words: step2_words,
            vad_speech_segments: step1_exec.output.vad_speech_segments.clone(),
        },
        &step_context,
        store,
    )
    .await?;
    let step2_segments = step2_exec.output;
    let source_text = source_text_from_step2_segments(&step2_segments);
    let step2_srt = step2_segments_to_srt(&step2_segments);
    update_subtitle_preview(
        app,
        task_id,
        &source_text,
        workspace_subtitle_segments_from_step2_segments(&step2_segments),
    )
    .await?;

    let run_result: WorkspaceResult<()> = if intent == "TRANSCRIBE_TRANSLATE" {
        execute_translate_steps(
            app,
            task_id,
            &record,
            runtime,
            source_lang,
            target_lang,
            &step2_segments,
            source_text,
            store,
        )
        .await
    } else {
        finish_transcribe_only(
            app,
            task_id,
            &record.item.path,
            &step2_segments,
            step2_srt,
            source_text,
            runtime.enable_subtitle_beautify,
            &runtime.subtitle_length_preset,
            &target_lang,
        )
        .await
    };
    run_result?;
    Ok(())
}

async fn mark_task_failed_after_execution_error<F, Fut>(
    result: WorkspaceResult<()>,
    mut mark_failed: F,
) -> WorkspaceResult<()>
where
    F: FnMut(&WorkspaceError) -> Fut,
    Fut: std::future::Future<Output = WorkspaceResult<()>>,
{
    match result {
        Ok(()) => Ok(()),
        Err(err) => {
            mark_failed(&err).await?;
            Err(err)
        }
    }
}

async fn separate_vocals_for_task(
    app: &AppHandle,
    task_id: &str,
    audio_path: &str,
    demucs_model: &str,
) -> WorkspaceResult<String> {
    report_task_stage(app, task_id, TaskStage::Separating, "", 0, 100).await?;

    let request = crate::services::demucs::SeparateVocalsRequest {
        task_id: task_id.to_string(),
        audio_path: audio_path.to_string(),
        model: demucs_model.to_string(),
    };
    let app_for_progress = app.clone();
    let task_id_owned = task_id.to_string();

    let join = spawn_blocking(move || {
        crate::services::demucs::separate_vocals_blocking(request, move |percent| {
            let report = report_task_stage(
                &app_for_progress,
                &task_id_owned,
                TaskStage::Separating,
                format!("{percent}/100"),
                percent,
                100,
            );
            if let Ok(handle) = Handle::try_current() {
                if handle.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread {
                    let _ = tokio::task::block_in_place(|| {
                        handle.block_on(async {
                            let _ = report.await;
                        });
                    });
                }
            }
        })
    })
    .await
    .map_err(|err| err.to_string())??;

    Ok(join.vocals_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::error::WorkspaceError;

    #[tokio::test]
    async fn execution_error_marks_task_failed_and_returns_original_error() {
        let mut marked_error = String::new();

        let result = mark_task_failed_after_execution_error(
            Err(WorkspaceError::InvalidRequest(
                "missing runtime settings".to_string(),
            )),
            |err| {
                marked_error = err.to_string();
                async { Ok(()) }
            },
        )
        .await;

        let err = result.expect_err("execution error should be returned");
        assert_eq!(err.code(), "INVALID_REQUEST");
        assert_eq!(marked_error, "invalid request: missing runtime settings");
    }

    #[tokio::test]
    async fn execution_success_does_not_mark_task_failed() {
        let mut marked = false;

        let result = mark_task_failed_after_execution_error(Ok(()), |_| {
            marked = true;
            async { Ok(()) }
        })
        .await;

        assert!(result.is_ok());
        assert!(!marked);
    }
}
