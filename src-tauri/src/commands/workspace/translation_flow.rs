use tauri::AppHandle;

use crate::commands::translate_types::BuildTranslationSegmentCommand;
use crate::db::store::TaskStore;
use crate::domain::error::WorkspaceResult;
use crate::domain::task::adapters::{
    map_step2_segments_for_translate,
    workspace_subtitle_segments_from_translation_segments,
};
use crate::domain::task::runtime_settings::PipelineRuntimeSettings;
use crate::services::pipeline::{StepContext, StepSource};

use super::output_completion::finish_translate_with_step5;
use super::pipeline_runner::execute_workspace_step;
use super::pipeline_steps::{
    Step3TerminologyPipelineStep, Step4TranslationPipelineStep,
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
    step2_segments: &[crate::commands::transcription::GroupedSentenceSegmentCommandDto],
    source_text: String,
    store: &TaskStore,
) -> WorkspaceResult<()> {
    let step_context = StepContext { task_id, store };
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
        store,
    )
    .await?;
    let step3_response = step3_exec.output;
    report_task_stage(
        app,
        task_id,
        TaskStage::Terminology,
        if step3_exec.source == StepSource::Cache {
            "step_cache_hit"
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
            enable_vision_assist: runtime.enable_vision_assist,
            app: app.clone(),
        },
        &step_context,
        store,
    )
    .await?;

    if step4_exec.source == StepSource::Cache {
        report_task_stage(app, task_id, TaskStage::Translating, "step_cache_hit", 1, 1).await?;
    }
    update_subtitle_preview(
        app,
        task_id,
        &source_text,
        workspace_subtitle_segments_from_translation_segments(&step4_exec.output.segments),
    )
    .await?;

    // Step 5 (LLM split/align) was removed: sentence_boundary already
    // segments to subtitle length and step4 translates each 1:1, so rows
    // are already the right length and zero-leak. Eval confirmed step5
    // had no measurable quality effect (identical row count and
    // length/CPS distribution on/off). The dead code, DB table, and
    // UnitStore methods were removed; `finalize_translate_with_step5`
    // kept its name for minimal diff churn but only does beautify + SRT.
    finalize_translate_with_step5(
        app,
        task_id,
        record,
        &target_lang,
        &step4_exec.output.segments,
        source_text,
        record.frozen.enable_subtitle_beautify,
        record.frozen.subtitle_length_preset.as_str(),
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
        &record.item.media_kind,
        segments,
        source_text,
        enable_subtitle_beautify,
        subtitle_length_preset,
        target_lang,
    )
    .await
}
