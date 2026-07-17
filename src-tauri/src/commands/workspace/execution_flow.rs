use tauri::{AppHandle, Manager};
use tauri::async_runtime::spawn_blocking;

use crate::db::store::TaskStore;
use crate::domain::error::{WorkspaceError, WorkspaceResult};
use crate::domain::language::LanguageTag;
use crate::domain::language_registry::LanguageRegistry;
use crate::services::pipeline::StepContext;
use crate::services::preferences_types::{AlignModel, AsrModel};
use crate::services::task_log::{TaskLogger, event};

use crate::domain::task::runtime_settings::resolve_runtime_settings;

use super::pipeline_steps::block_on_runtime_worker;

use super::output_completion::deliver_from_sot;
use super::pipeline_runner::execute_workspace_step;
use super::pipeline_steps::{Step1AsrPipelineStep, Step2SegmentsPipelineStep};
use super::preview::update_subtitle_preview;
use super::progress::{mark_task_failed, report_task_stage};
use super::review_flow::{
    enter_review_source, materialize_source_sot, read_task_review_flags,
};
use super::translation_flow::execute_translate_steps;
use super::{
    TaskStage, WorkspaceTaskRecord, get_task_record, normalize_intent, normalize_task_target_lang,
    patch_task_item,
};

pub(super) async fn execute_single_task(app: &AppHandle, task_id: &str) -> WorkspaceResult<()> {
    // Reject *before* the mark-failed wrapper so a parked review gate is never
    // torn down into `error` by an accidental full execute.
    let record = get_task_record(task_id)?;
    super::review_flow::reject_full_run_if_awaiting_review(&record.item.transcribe_status)?;

    // Mid-pipeline resume shares this entry with full runs (single-flight queue).
    if record.item.resume_from.trim() == super::review_flow::RESUME_FROM_TRANSLATE {
        return super::review_flow::execute_resume_translate_from_sot(app, task_id).await;
    }

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
    // Caller already rejected `review_*` (must not mark those tasks failed).
    let record = get_task_record(task_id)?;
    // Full pipeline must not carry a stale resume marker.
    if !record.item.resume_from.trim().is_empty() {
        patch_task_item(app, task_id, |task| {
            task.item.resume_from = String::new();
        })
        .await?;
    }
    let intent = normalize_intent(&record.intent).to_string();
    let is_srt_translate =
        intent == "TRANSLATE_SRT" || record.item.media_kind.trim() == "subtitle";

    if is_srt_translate {
        return execute_srt_translate_task(app, task_id, &record).await;
    }

    let store = app.state::<TaskStore>().inner();
    let runtime =
        resolve_runtime_settings(store, &record.frozen, intent == "TRANSCRIBE_TRANSLATE")?;

    let asr_model = AsrModel::parse(&runtime.asr_model);
    let align_model = AlignModel::parse(&runtime.align_model);
    let lang_tag: LanguageTag = record.source_lang.parse()
        .map_err(|e| WorkspaceError::InvalidRequest(format!("task {} has invalid source language '{}': {e}", record.item.id, record.source_lang)))?;
    LanguageRegistry::asr_code(asr_model, lang_tag)
        .map_err(|e| WorkspaceError::InvalidRequest(format!("task {} language incompatible with ASR model: {e}", record.item.id)))?;
    LanguageRegistry::align_code(align_model, lang_tag)
        .map_err(|e| WorkspaceError::InvalidRequest(format!("task {} language incompatible with align model: {e}", record.item.id)))?;

    let mut source_lang = record.source_lang.clone();
    let target_lang = normalize_task_target_lang(&record.target_lang);
    let step_context = StepContext { task_id, store };

    // Snapshot the full settings used for this run into main.log so any
    // task can be debugged/reproduced without guessing. API key is masked.
    let main_logger = TaskLogger::main_with_media(
        task_id.to_string(),
        record.item.path.clone(),
    );
    let api_key_masked = if runtime.translate_api_key.is_empty() {
        String::new()
    } else if runtime.translate_api_key.chars().count() > 6 {
        // Long key: show a prefix so it can be recognized without leaking it.
        let prefix: String = runtime.translate_api_key.chars().take(6).collect();
        format!("{prefix}…")
    } else {
        // Short secret: a prefix would reveal the whole thing, so mask fully.
        "••••".to_string()
    };
    // `enable_vision_assist` comes from `runtime` (read once in
    // resolve_runtime_settings) so the logged value matches the value that
    // translation actually applies — no second live DB read that could race
    // with a mid-run toggle.
    main_logger.event(
        event::TASK_STARTED,
        Some(&serde_json::json!({
            "taskId": task_id,
            "intent": intent,
            "mediaPath": record.item.path,
            "sourceLang": source_lang,
            "targetLang": target_lang,
            "terminologyGroupId": record.item.terminology_group_id,
            "runtime": {
                "provider": runtime.provider,
                "asrModel": runtime.asr_model,
                "alignModel": runtime.align_model,
                "demucsModel": runtime.demucs_model,
                "chunkTargetSeconds": runtime.chunk_target_seconds,
                "enableVocalSeparation": runtime.enable_vocal_separation,
                "translateBaseUrl": runtime.translate_base_url,
                "translateModel": runtime.translate_model,
                "translateApiKey": api_key_masked,
                "llmConcurrency": runtime.llm_concurrency,
                "subtitleLengthPreset": record.frozen.subtitle_length_preset,
                "enableSubtitleBeautify": record.frozen.enable_subtitle_beautify,
                "enableVisionAssist": runtime.enable_vision_assist,
                "terminologyEntriesCount": runtime.terminology_entries.len(),
            },
            "frozen": {
                "subtitleLengthPreset": record.frozen.subtitle_length_preset,
                "enableSubtitleBeautify": record.frozen.enable_subtitle_beautify,
                "terminologyGroupsCount": record.frozen.terminology_groups.len(),
            },
        })),
    );

    report_task_stage(app, task_id, TaskStage::Preparing, "", 1, 1).await?;
    // Do NOT clear subtitle SoT here. Full re-queue (`enqueue` → apply_enqueue_request)
    // is the only place that wipes generated cues for a deliberate re-run. Execute only
    // clears the error flag so a clean stage report can start.
    patch_task_item(app, task_id, |task| {
        task.item.transcribe_error = String::new();
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
            provider: runtime.provider.as_str().to_string(),
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
            subtitle_length_preset: record.frozen.subtitle_length_preset.as_str().to_string(),
            words: step2_words,
            vad_speech_segments: step1_exec.output.vad_speech_segments.clone(),
        },
        &step_context,
        store,
    )
    .await?;
    let step2_segments = step2_exec.output;

    // Materialize source SoT (beautify once). Publishes JSON + DB; no second preview emit.
    let (source_sot, source_text, step2_srt) = materialize_source_sot(
        app,
        task_id,
        &step2_segments,
        record.frozen.enable_subtitle_beautify,
        record.frozen.subtitle_length_preset.as_str(),
        &target_lang,
    )
    .await?;

    // Pure transcription: no review gates — deliver and mark done so the
    // subtitle editor is immediately editable. Source/target review only
    // apply to translate pipelines (TRANSCRIBE_TRANSLATE / TRANSLATE_SRT).
    if intent != "TRANSCRIBE_TRANSLATE" {
        deliver_from_sot(
            app,
            task_id,
            &record.item.path,
            &record.item.media_kind,
            &source_sot,
            false,
            &source_text,
        )
        .await?;
        patch_task_item(app, task_id, |task| {
            task.item.result_srt = step2_srt;
        })
        .await?;
        return Ok(());
    }

    // Checkpoint S (translate flow only): pause before terminology when
    // task.review_source is on so the user can fix source cues first.
    let (review_source, _) = read_task_review_flags(task_id).await?;
    if review_source {
        enter_review_source(app, task_id).await?;
        return Ok(());
    }

    // Translate the same SoT the editor/preview shows (not raw pre-beautify step2).
    let translate_step2 =
        crate::services::subtitle_import::workspace_segments_to_step2(&source_sot);
    execute_translate_steps(
        app,
        task_id,
        &record,
        runtime,
        source_lang,
        target_lang,
        &translate_step2,
        source_text,
        store,
    )
    .await
}

/// Translate an imported SRT task: skip ASR / Step2 segmentation entirely.
/// Cue boundaries come only from the imported subtitle file.
async fn execute_srt_translate_task(
    app: &AppHandle,
    task_id: &str,
    record: &WorkspaceTaskRecord,
) -> WorkspaceResult<()> {
    if record.item.media_kind.trim() != "subtitle"
        && !crate::services::subtitle_import::is_srt_path(&record.item.path)
    {
        return Err(WorkspaceError::InvalidRequest(
            "TRANSLATE_SRT requires a subtitle task".to_string(),
        ));
    }

    let store = app.state::<TaskStore>().inner();
    let mut runtime = resolve_runtime_settings(store, &record.frozen, true)?;
    // No video attached in MVP — never sample frames.
    runtime.enable_vision_assist = false;

    let source_lang = record.source_lang.clone();
    let target_lang = normalize_task_target_lang(&record.target_lang);

    let main_logger = TaskLogger::main_with_media(task_id.to_string(), record.item.path.clone());
    main_logger.event(
        event::TASK_STARTED,
        Some(&serde_json::json!({
            "taskId": task_id,
            "intent": "TRANSLATE_SRT",
            "mediaPath": record.item.path,
            "sourceLang": source_lang,
            "targetLang": target_lang,
            "mediaKind": record.item.media_kind,
        })),
    );

    report_task_stage(app, task_id, TaskStage::Preparing, "", 1, 1).await?;

    let workspace_segments = crate::services::subtitle_import::load_srt_segments_for_run(
        &record.item.subtitle_segments_json,
        &record.item.path,
    )
    .map_err(WorkspaceError::InvalidRequest)?;
    if workspace_segments.is_empty() {
        return Err(WorkspaceError::InvalidRequest(
            "SRT task has no subtitle cues to translate".to_string(),
        ));
    }

    // Clear prior translations in the live preview, keep source cues.
    let source_only: Vec<_> = workspace_segments
        .iter()
        .map(|segment| crate::services::workspace_subtitle::WorkspaceSubtitleSegment {
            start_ms: segment.start_ms,
            end_ms: segment.end_ms,
            source_text: segment.source_text.clone(),
            translated_text: String::new(),
            source_words: Vec::new(),
        })
        .collect();
    let source_text =
        crate::services::subtitle_import::source_text_from_workspace_segments(&source_only);
    update_subtitle_preview(app, task_id, &source_text, source_only.clone()).await?;

    // Persist source SoT so source review can edit before terminology.
    let source_json = crate::services::workspace_subtitle::serialize_segments(&source_only);
    super::output_completion::persist_workspace_segments(app, task_id, &source_json).await?;
    patch_task_item(app, task_id, |task| {
        task.item.subtitle_segments_json = source_json;
        task.item.result_text = source_text.clone();
    })
    .await?;

    let (review_source, _) = read_task_review_flags(task_id).await?;
    if review_source {
        enter_review_source(app, task_id).await?;
        return Ok(());
    }

    let step2_segments =
        crate::services::subtitle_import::workspace_segments_to_step2(&workspace_segments);

    // Force no beautify so imported cue boundaries stay intact.
    let mut record = record.clone();
    record.frozen.enable_subtitle_beautify = false;
    record.intent = "TRANSLATE_SRT".to_string();

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
            block_on_runtime_worker(report);
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
