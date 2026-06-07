use std::path::Path;

use tauri::AppHandle;

use crate::commands::translate_types::BuildTranslationSegmentCommand;
use crate::domain::error::WorkspaceResult;
use crate::domain::task::adapters::{
    map_step2_segments_for_translate, translation_segments_from_step52_parents,
    workspace_subtitle_segments_from_step51_parents,
    workspace_subtitle_segments_from_step52_parents,
    workspace_subtitle_segments_from_translation_segments,
};
use crate::domain::task::runtime_settings::PipelineRuntimeSettings;
use crate::services::pipeline::{StepContext, StepSource};

use super::output_completion::finish_translate_with_step5;
use super::pipeline_runner::execute_workspace_step;
use super::pipeline_steps::{
    Step3TerminologyPipelineStep, Step4TranslationPipelineStep, Step51SourceSplitPipelineStep,
    Step52TranslationAlignPipelineStep,
};
use super::preview::update_subtitle_preview;
use super::progress::report_task_stage;
use super::{TaskStage, WorkspaceTaskRecord};

#[allow(clippy::too_many_arguments)]
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
) -> WorkspaceResult<()> {
    let step_context = StepContext { output_dir };
    report_task_stage(app, task_id, TaskStage::Terminology, "", 0, 1).await?;

    let terminology_segments = map_step2_segments_for_translate(step2_segments);
    let step3_exec = execute_workspace_step(
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
            app: app.clone(),
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
    )
    .await?;

    let step4_exec = execute_workspace_step(
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
        report_task_stage(app, task_id, TaskStage::Translating, "缓存命中", 1, 1).await?;
    }
    update_subtitle_preview(
        app,
        task_id,
        &source_text,
        workspace_subtitle_segments_from_translation_segments(&step4_exec.output.segments),
    )
    .await?;

    let step51_exec = execute_workspace_step(
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
            subtitle_length_preset: runtime.subtitle_length_preset.clone(),
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
        )
        .await?;
    }
    update_subtitle_preview(
        app,
        task_id,
        &source_text,
        workspace_subtitle_segments_from_step51_parents(&step51_exec.output.parents),
    )
    .await?;

    let step52_exec = execute_workspace_step(
        &Step52TranslationAlignPipelineStep {
            task_id: task_id.to_string(),
            media_path: record.item.path.clone(),
            source_lang: source_lang.clone(),
            target_lang: target_lang.clone(),
            theme_summary: step3_response.theme_summary.clone(),
            parents: step51_exec.output.parents.clone(),
            terminology_entries: step3_response.terminology_entries.clone(),
            subtitle_length_preset: runtime.subtitle_length_preset.clone(),
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
        )
        .await?;
    }
    update_subtitle_preview(
        app,
        task_id,
        &source_text,
        workspace_subtitle_segments_from_step52_parents(&step52_exec.output.parents),
    )
    .await?;
    let step5_segments = translation_segments_from_step52_parents(&step52_exec.output.parents);

    finalize_translate_with_step5(
        app,
        task_id,
        record,
        &target_lang,
        &step5_segments,
        source_text,
        runtime.enable_subtitle_beautify,
        &runtime.subtitle_length_preset,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn finalize_translate_with_step5(
    app: &AppHandle,
    task_id: &str,
    record: &WorkspaceTaskRecord,
    target_lang: &str,
    segments: &[BuildTranslationSegmentCommand],
    source_text: String,
    enable_subtitle_beautify: bool,
    subtitle_length_preset: &str,
) -> WorkspaceResult<()> {
    finish_translate_with_step5(
        app,
        task_id,
        &record.item.path,
        segments,
        source_text,
        enable_subtitle_beautify,
        subtitle_length_preset,
        target_lang,
    )
    .await
}
