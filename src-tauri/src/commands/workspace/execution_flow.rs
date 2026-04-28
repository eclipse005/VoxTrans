use std::path::Path;

use tauri::AppHandle;

use crate::services::pipeline::{StepContext, execute_step};
use crate::services::workspace_subtitle::serialize_segments;

use super::adapters::{
    source_text_from_step2_segments, step2_segments_to_srt,
    workspace_subtitle_segments_from_step2_segments,
};
use super::artifact_migration::migrate_legacy_artifacts;
use super::output_completion::finish_transcribe_only;
use super::pipeline_steps::{Step1AsrPipelineStep, Step2SegmentsPipelineStep};
use super::progress::{mark_task_failed, report_task_stage};
use super::runtime_settings::resolve_runtime_settings;
use super::translation_flow::execute_translate_steps;
use super::{TaskStage, get_task_record, normalize_intent, patch_task_item};

pub(super) async fn execute_single_task(app: &AppHandle, task_id: &str) -> Result<(), String> {
    let record = get_task_record(task_id)?;
    let intent = normalize_intent(&record.intent).to_string();
    let runtime =
        resolve_runtime_settings(&record.settings_snapshot, intent == "TRANSCRIBE_TRANSLATE")?;
    let mut source_lang = if record.source_lang.trim().is_empty() {
        "auto".to_string()
    } else {
        record.source_lang.trim().to_string()
    };
    let target_lang = if record.target_lang.trim().is_empty() {
        "zh-CN".to_string()
    } else {
        record.target_lang.trim().to_string()
    };
    let task_output_dir =
        crate::services::task_path::task_output_dir(task_id, Path::new(&record.item.path));
    std::fs::create_dir_all(&task_output_dir).map_err(|err| err.to_string())?;
    let artifact_dir =
        crate::services::task_path::task_artifacts_dir(task_id, Path::new(&record.item.path));
    std::fs::create_dir_all(&artifact_dir).map_err(|err| err.to_string())?;
    migrate_legacy_artifacts(&task_output_dir, &artifact_dir)?;
    let step_context = StepContext {
        output_dir: &artifact_dir,
    };

    report_task_stage(app, task_id, TaskStage::Preparing, "", 1, 1)?;
    patch_task_item(app, task_id, |task| {
        task.item.transcribe_error = String::new();
        task.item.result_text = String::new();
        task.item.result_srt = String::new();
        task.item.subtitle_segments_json = "[]".to_string();
    })?;

    let step1_exec = match execute_step(
        &Step1AsrPipelineStep {
            task_id: task_id.to_string(),
            media_path: record.item.path.clone(),
            source_lang: source_lang.clone(),
            provider: runtime.provider.clone(),
            chunk_target_seconds: runtime.chunk_target_seconds,
            app: app.clone(),
        },
        &step_context,
    )
    .await
    {
        Ok(value) => value,
        Err(err) => {
            mark_task_failed(app, task_id, &err)?;
            return Err(err);
        }
    };

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

    report_task_stage(app, task_id, TaskStage::Segmenting, "", 0, 1)?;

    let step2_exec = match execute_step(
        &Step2SegmentsPipelineStep {
            task_id: task_id.to_string(),
            media_path: record.item.path.clone(),
            source_lang: source_lang.clone(),
            words: step2_words,
            subtitle_max_words_per_segment: runtime.subtitle_max_words_per_segment,
            translate_api_key: runtime.translate_api_key.clone(),
            translate_base_url: runtime.translate_base_url.clone(),
            translate_model: runtime.translate_model.clone(),
            llm_concurrency: runtime.llm_concurrency,
            app: app.clone(),
        },
        &step_context,
    )
    .await
    {
        Ok(value) => value,
        Err(err) => {
            mark_task_failed(app, task_id, &err)?;
            return Err(err);
        }
    };
    let step2_segments = step2_exec.output;
    let source_text = source_text_from_step2_segments(&step2_segments);
    let step2_srt = step2_segments_to_srt(&step2_segments);
    update_processing_preview_from_step2(app, task_id, &step2_segments, &source_text)?;

    let run_result = if intent == "TRANSCRIBE_TRANSLATE" {
        execute_translate_steps(
            app,
            task_id,
            &record,
            runtime,
            source_lang,
            target_lang,
            &artifact_dir,
            &step2_segments,
            source_text,
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
            runtime.subtitle_length_reference,
            &target_lang,
        )
    };
    if let Err(err) = run_result {
        mark_task_failed(app, task_id, &err)?;
        return Err(err);
    }
    Ok(())
}

fn update_processing_preview_from_step2(
    app: &AppHandle,
    task_id: &str,
    step2_segments: &[crate::commands::transcription::GroupedSentenceSegmentCommandDto],
    source_text: &str,
) -> Result<(), String> {
    let subtitle_segments_json = serialize_segments(
        &workspace_subtitle_segments_from_step2_segments(step2_segments),
    );
    patch_task_item(app, task_id, |task| {
        task.item.result_text = source_text.to_string();
        task.item.subtitle_segments_json = subtitle_segments_json.clone();
    })
}
