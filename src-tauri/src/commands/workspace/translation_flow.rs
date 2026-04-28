use std::path::Path;

use tauri::AppHandle;

use crate::commands::translate_types::{
    BuildStep53TranslationPolishCommandResponse, BuildTranslationSegmentCommand,
};
use crate::services::pipeline::{StepContext, StepSource, execute_step};

use super::adapters::{
    map_step2_segments_for_translate, workspace_subtitle_segments_from_step51_parents,
    workspace_subtitle_segments_from_step52_parents,
    workspace_subtitle_segments_from_translation_segments,
};
use super::output_completion::finish_translate_with_step5;
use super::pipeline_runner::execute_workspace_step;
use super::pipeline_steps::{
    Step3TerminologyPipelineStep, Step4TranslationPipelineStep, Step6FinalCheckPipelineStep,
    Step51SourceSplitPipelineStep, Step52TranslationAlignPipelineStep,
    Step53TranslationPolishPipelineStep,
};
use super::preview::update_subtitle_preview;
use super::progress::report_task_stage;
use super::runtime_settings::PipelineRuntimeSettings;
use super::task_logs::{
    log_step6_final_check_error_to_main, log_step6_final_check_to_main,
    remove_step6_final_check_artifact,
};
use super::{STEP_05_03_TRANSLATION_POLISH_FILE, TaskStage, WorkspaceTaskRecord, json_files};

pub(super) async fn execute_translate_steps(
    app: &AppHandle,
    task_id: &str,
    record: &WorkspaceTaskRecord,
    runtime: PipelineRuntimeSettings,
    source_lang: String,
    target_lang: String,
    output_dir: &Path,
    step2_segments: &[crate::commands::transcription::GroupedSentenceSegmentCommandDto],
    source_text: String,
) -> Result<(), String> {
    let step_context = StepContext { output_dir };
    // Checkpoint contract:
    // Existing step53 layout is treated as user-selected input. Delete the file to rebuild it.
    if let Some(step5_existing) = json_files::read_json_file_if_exists::<
        BuildStep53TranslationPolishCommandResponse,
    >(&output_dir.join(STEP_05_03_TRANSLATION_POLISH_FILE))?
    {
        return finalize_translate_with_step5(
            app,
            task_id,
            record,
            &source_lang,
            &target_lang,
            output_dir,
            &step_context,
            &step5_existing.segments,
            source_text,
            runtime.enable_subtitle_beautify,
            runtime.subtitle_length_reference,
        )
        .await;
    }

    report_task_stage(app, task_id, TaskStage::Terminology, "", 0, 1)?;

    let terminology_segments = map_step2_segments_for_translate(step2_segments);
    let step3_exec = execute_workspace_step(
        app,
        task_id,
        &Step3TerminologyPipelineStep {
            task_id: task_id.to_string(),
            media_path: record.item.path.clone(),
            source_lang: source_lang.clone(),
            target_lang: target_lang.clone(),
            segments: terminology_segments.clone(),
            translate_api_key: runtime.translate_api_key.clone(),
            translate_base_url: runtime.translate_base_url.clone(),
            translate_model: runtime.translate_model.clone(),
            llm_concurrency: runtime.llm_concurrency,
            terminology_entries: runtime.terminology_entries.clone(),
        },
        &step_context,
    )
    .await?;
    let step3_response = step3_exec.output;
    report_task_stage(
        app,
        task_id,
        TaskStage::Terminology,
        if step3_exec.source == StepSource::Cache {
            "缓存命中"
        } else {
            ""
        },
        1,
        1,
    )?;

    let step4_exec = execute_workspace_step(
        app,
        task_id,
        &Step4TranslationPipelineStep {
            task_id: task_id.to_string(),
            media_path: record.item.path.clone(),
            source_lang: source_lang.clone(),
            target_lang: target_lang.clone(),
            segments: terminology_segments,
            theme_summary: step3_response.theme_summary.clone(),
            terminology_entries: step3_response.terminology_entries.clone(),
            translate_api_key: runtime.translate_api_key.clone(),
            translate_base_url: runtime.translate_base_url.clone(),
            translate_model: runtime.translate_model.clone(),
            llm_concurrency: runtime.llm_concurrency,
            app: app.clone(),
        },
        &step_context,
    )
    .await?;

    if step4_exec.source == StepSource::Cache {
        report_task_stage(app, task_id, TaskStage::Translating, "缓存命中", 1, 1)?;
    }
    update_subtitle_preview(
        app,
        task_id,
        &source_text,
        workspace_subtitle_segments_from_translation_segments(&step4_exec.output.segments),
    )?;

    let step51_exec = execute_workspace_step(
        app,
        task_id,
        &Step51SourceSplitPipelineStep {
            task_id: task_id.to_string(),
            media_path: record.item.path.clone(),
            source_lang: source_lang.clone(),
            target_lang: target_lang.clone(),
            segments: step4_exec.output.segments.clone(),
            translate_api_key: runtime.translate_api_key.clone(),
            translate_base_url: runtime.translate_base_url.clone(),
            translate_model: runtime.translate_model.clone(),
            llm_concurrency: runtime.llm_concurrency,
            subtitle_max_words_per_segment: runtime.subtitle_max_words_per_segment,
            subtitle_length_reference: runtime.subtitle_length_reference,
            app: app.clone(),
        },
        &step_context,
    )
    .await?;

    if step51_exec.source == StepSource::Cache {
        report_task_stage(
            app,
            task_id,
            TaskStage::SubtitleLayout,
            "原文切分缓存命中",
            1,
            1,
        )?;
    }
    update_subtitle_preview(
        app,
        task_id,
        &source_text,
        workspace_subtitle_segments_from_step51_parents(&step51_exec.output.parents),
    )?;

    let step52_exec = execute_workspace_step(
        app,
        task_id,
        &Step52TranslationAlignPipelineStep {
            task_id: task_id.to_string(),
            media_path: record.item.path.clone(),
            source_lang: source_lang.clone(),
            target_lang: target_lang.clone(),
            theme_summary: step3_response.theme_summary.clone(),
            parents: step51_exec.output.parents.clone(),
            terminology_entries: step3_response.terminology_entries.clone(),
            translate_api_key: runtime.translate_api_key.clone(),
            translate_base_url: runtime.translate_base_url.clone(),
            translate_model: runtime.translate_model.clone(),
            llm_concurrency: runtime.llm_concurrency,
            app: app.clone(),
        },
        &step_context,
    )
    .await?;

    if step52_exec.source == StepSource::Cache {
        report_task_stage(
            app,
            task_id,
            TaskStage::SubtitleLayout,
            "译文对齐缓存命中",
            1,
            1,
        )?;
    }
    update_subtitle_preview(
        app,
        task_id,
        &source_text,
        workspace_subtitle_segments_from_step52_parents(&step52_exec.output.parents),
    )?;

    let step53_exec = execute_workspace_step(
        app,
        task_id,
        &Step53TranslationPolishPipelineStep {
            task_id: task_id.to_string(),
            media_path: record.item.path.clone(),
            source_lang: source_lang.clone(),
            target_lang: target_lang.clone(),
            theme_summary: step3_response.theme_summary,
            parents: step52_exec.output.parents,
            terminology_entries: step3_response.terminology_entries,
            translate_api_key: runtime.translate_api_key.clone(),
            translate_base_url: runtime.translate_base_url.clone(),
            translate_model: runtime.translate_model.clone(),
            llm_concurrency: runtime.llm_concurrency,
            subtitle_length_reference: runtime.subtitle_length_reference,
            app: app.clone(),
        },
        &step_context,
    )
    .await?;
    if step53_exec.source == StepSource::Cache {
        report_task_stage(
            app,
            task_id,
            TaskStage::SubtitleLayout,
            "译文润色缓存命中",
            1,
            1,
        )?;
    }
    let step53_output = step53_exec.output;

    finalize_translate_with_step5(
        app,
        task_id,
        record,
        &source_lang,
        &target_lang,
        output_dir,
        &step_context,
        &step53_output.segments,
        source_text,
        runtime.enable_subtitle_beautify,
        runtime.subtitle_length_reference,
    )
    .await
}

async fn finalize_translate_with_step5(
    app: &AppHandle,
    task_id: &str,
    record: &WorkspaceTaskRecord,
    source_lang: &str,
    target_lang: &str,
    output_dir: &Path,
    step_context: &StepContext<'_>,
    segments: &[BuildTranslationSegmentCommand],
    source_text: String,
    enable_subtitle_beautify: bool,
    subtitle_length_reference: u32,
) -> Result<(), String> {
    run_step6_final_check(
        app,
        task_id,
        record,
        source_lang,
        target_lang,
        output_dir,
        step_context,
        segments.to_vec(),
    )
    .await;

    finish_translate_with_step5(
        app,
        task_id,
        &record.item.path,
        segments,
        source_text,
        enable_subtitle_beautify,
        subtitle_length_reference,
        target_lang,
    )
}

async fn run_step6_final_check(
    app: &AppHandle,
    task_id: &str,
    record: &WorkspaceTaskRecord,
    source_lang: &str,
    target_lang: &str,
    output_dir: &Path,
    step_context: &StepContext<'_>,
    segments: Vec<BuildTranslationSegmentCommand>,
) {
    remove_step6_final_check_artifact(output_dir);
    let step6_exec = match execute_step(
        &Step6FinalCheckPipelineStep {
            task_id: task_id.to_string(),
            media_path: record.item.path.clone(),
            source_lang: source_lang.to_string(),
            target_lang: target_lang.to_string(),
            segments,
            app: app.clone(),
        },
        step_context,
    )
    .await
    {
        Ok(value) => Some(value),
        Err(err) => {
            log_step6_final_check_error_to_main(task_id, &record.item.path, &err);
            None
        }
    };
    if let Some(step6_exec) = step6_exec {
        log_step6_final_check_to_main(
            task_id,
            &record.item.path,
            &step6_exec.output,
            step6_exec.source,
        );
    }
}
