use tauri::{AppHandle, Manager};

use crate::db::store::TaskStore;
use crate::domain::error::WorkspaceResult;
use crate::domain::task::adapters::map_step2_segments_for_translate;
use crate::domain::task::runtime_settings::{resolve_runtime_settings, PipelineRuntimeSettings};
use crate::services::pipeline::{StepContext, StepSource};

use super::output_completion::deliver_from_sot;
use super::pipeline_runner::execute_workspace_step;
use super::pipeline_steps::{
    Step3TerminologyPipelineStep, Step4TranslationPipelineStep,
};
use super::progress::report_task_stage;
use super::review_flow::{
    enter_review_target, materialize_target_sot, read_task_review_flags,
};
use super::{TaskStage, WorkspaceTaskRecord, get_task_record, normalize_task_target_lang};

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

    // Materialize target SoT once (publishes JSON+DB), then checkpoint T or deliver.
    let workspace_segments = materialize_target_sot(
        app,
        task_id,
        &step4_exec.output.segments,
        &source_text,
        record.frozen.enable_subtitle_beautify,
        record.frozen.subtitle_length_preset.as_str(),
        &target_lang,
    )
    .await?;

    let (_, review_target) = read_task_review_flags(task_id).await?;
    if review_target {
        enter_review_target(app, task_id).await?;
        return Ok(());
    }

    deliver_from_sot(
        app,
        task_id,
        &record.item.path,
        &record.item.media_kind,
        &workspace_segments,
        true,
        &source_text,
    )
    .await
}

/// Resume translation from current SoT-derived step2 segments (after source review).
pub(super) async fn execute_translate_steps_from_step2(
    app: &AppHandle,
    task_id: &str,
    step2_segments: &[crate::commands::transcription::GroupedSentenceSegmentCommandDto],
    source_text: String,
) -> WorkspaceResult<()> {
    let record = get_task_record(task_id)?;
    let store = app.state::<TaskStore>().inner();
    let runtime = resolve_runtime_settings(store, &record.frozen, true)?;
    let source_lang = record.source_lang.clone();
    let target_lang = normalize_task_target_lang(&record.target_lang);
    execute_translate_steps(
        app,
        task_id,
        &record,
        runtime,
        source_lang,
        target_lang,
        step2_segments,
        source_text,
        store,
    )
    .await
}

