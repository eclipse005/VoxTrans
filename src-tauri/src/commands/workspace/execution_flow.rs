use std::path::Path;

use tauri::AppHandle;

use crate::services::pipeline::StepContext;

use crate::domain::task::adapters::{
    source_text_from_step2_segments, step2_segments_to_srt,
    workspace_subtitle_segments_from_step2_segments,
};
use crate::domain::task::runtime_settings::resolve_runtime_settings;

use super::artifact_migration::migrate_legacy_artifacts;
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

pub(super) async fn execute_single_task(app: &AppHandle, task_id: &str) -> Result<(), String> {
    let record = get_task_record(task_id)?;
    let intent = normalize_intent(&record.intent).to_string();
    let runtime =
        resolve_runtime_settings(&record.settings_snapshot, intent == "TRANSCRIBE_TRANSLATE")?;
    let mut source_lang = normalize_task_source_lang(&record.source_lang);
    let target_lang = normalize_task_target_lang(&record.target_lang);
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

    let step1_exec = execute_workspace_step(
        app,
        task_id,
        &Step1AsrPipelineStep {
            task_id: task_id.to_string(),
            media_path: record.item.path.clone(),
            source_lang: source_lang.clone(),
            asr_model: runtime.asr_model.clone(),
            align_model: runtime.align_model.clone(),
            provider: runtime.provider.clone(),
            chunk_target_seconds: runtime.chunk_target_seconds,
            app: app.clone(),
        },
        &step_context,
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

    report_task_stage(app, task_id, TaskStage::Segmenting, "", 0, 0)?;

    let step2_exec = execute_workspace_step(
        app,
        task_id,
        &Step2SegmentsPipelineStep {
            task_id: task_id.to_string(),
            media_path: record.item.path.clone(),
            source_lang: source_lang.clone(),
            subtitle_length_preset: runtime.subtitle_length_preset.clone(),
            use_subtitle_layout_split: intent != "TRANSCRIBE_TRANSLATE",
            words: step2_words,
        },
        &step_context,
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
    )?;

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
            &runtime.subtitle_length_preset,
            &target_lang,
        )
    };
    if let Err(err) = run_result {
        mark_task_failed(app, task_id, &err)?;
        return Err(err);
    }
    Ok(())
}
