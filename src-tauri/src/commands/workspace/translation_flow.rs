use tauri::{AppHandle, Manager};

use crate::db::store::TaskStore;
use crate::domain::error::WorkspaceResult;
use crate::domain::task::adapters::map_step2_segments_for_translate;
use crate::domain::task::runtime_settings::{resolve_runtime_settings, PipelineRuntimeSettings};
use crate::services::pipeline::{StepContext, StepSource};

use crate::services::terminology_agent::{
    merge_glossary_user_priority, run_terminology_briefing, GlossaryEntry, TerminologyAgentInput,
    TranscriptCue,
};
use crate::services::time_utils::seconds_to_millis;

use super::output_completion::deliver_from_sot;
use super::pipeline_runner::execute_workspace_step;
use super::pipeline_steps::{block_on_runtime_worker, Step4TranslationPipelineStep};
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

    // Frozen user glossary is always-on. The optional terminology Agent
    // (fail-open) may add grounded rows + a style_guide for this video.
    let (terminology_entries, style_guide) = resolve_terminology_for_translation(
        app,
        task_id,
        record,
        &runtime,
        &source_lang,
        &target_lang,
        step2_segments,
        store,
    )
    .await;

    let terminology_segments = map_step2_segments_for_translate(step2_segments);
    let step4_exec = execute_workspace_step(
        &Step4TranslationPipelineStep {
            task_id: task_id.to_string(),
            media_path: record.item.path.clone(),
            source_lang: source_lang.clone(),
            target_lang: target_lang.clone(),
            segments: terminology_segments,
            style_guide,
            terminology_entries,
            translate_api_key: runtime.translate_api_key.clone(),
            translate_base_url: runtime.translate_base_url.clone(),
            translate_model: runtime.translate_model.clone(),
            llm_concurrency: runtime.llm_concurrency,
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

#[allow(clippy::too_many_arguments)]
async fn resolve_terminology_for_translation(
    app: &AppHandle,
    task_id: &str,
    record: &WorkspaceTaskRecord,
    runtime: &PipelineRuntimeSettings,
    source_lang: &str,
    target_lang: &str,
    step2_segments: &[crate::commands::transcription::GroupedSentenceSegmentCommandDto],
    store: &TaskStore,
) -> (
    Vec<crate::commands::translate_types::TranslateTerminologyEntryCommand>,
    String,
) {
    let user_entries = runtime.terminology_entries.clone();
    if !record.frozen.enable_terminology_agent {
        return (user_entries, String::new());
    }

    let _ = report_task_stage(app, task_id, TaskStage::Terminology, "", 0, 0).await;
    let cues: Vec<TranscriptCue> = step2_segments
        .iter()
        .enumerate()
        .map(|(i, segment)| TranscriptCue {
            index: i + 1,
            start_ms: seconds_to_millis(segment.start),
            text: segment.segment.clone(),
        })
        .collect();
    let user_terms: Vec<GlossaryEntry> = user_entries
        .iter()
        .map(|e| GlossaryEntry::new(&e.source, &e.target, &e.note))
        .collect();
    let hay: String = cues
        .iter()
        .map(|c| c.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");

    let app_for_progress = app.clone();
    let task_id_owned = task_id.to_string();
    let briefing = run_terminology_briefing(
        TerminologyAgentInput {
            task_id: task_id.to_string(),
            media_path: record.item.path.clone(),
            title: record.item.name.clone(),
            source_lang: source_lang.to_string(),
            target_lang: target_lang.to_string(),
            cues,
            user_terms: user_terms.clone(),
            api_key: runtime.translate_api_key.clone(),
            base_url: runtime.translate_base_url.clone(),
            model: runtime.translate_model.clone(),
            anysearch_api_key: runtime.anysearch_api_key.clone(),
            store: Some(store.clone()),
        },
        move |win, nwin, round, max, _tool| {
            let detail = if nwin > 1 {
                format!("{win}/{nwin} {round}/{max}")
            } else {
                format!("{round}/{max}")
            };
            let report = report_task_stage(
                &app_for_progress,
                &task_id_owned,
                TaskStage::Terminology,
                detail,
                round,
                max,
            );
            block_on_runtime_worker(report);
        },
    )
    .await;

    if briefing.skipped {
        return (user_entries, String::new());
    }

    let merged = merge_glossary_user_priority(&user_terms, &briefing.glossary, &hay);
    (
        merged.into_iter().map(|e| e.into_translate_entry()).collect(),
        briefing.style_guide,
    )
}
