use crate::services::translation::domain::{
    AlignedCue, StageReport, StageResult, StageStatus, TranslationPipelineRequest,
    TranslationPipelineResponse, TranslationStage,
};
use crate::services::translation::{guardrails, llm, mapping, stages};
use tauri::Emitter;

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationProgressEvent {
    pub task_id: String,
    pub phase: String,
}

pub async fn run_translation_pipeline(
    request: TranslationPipelineRequest,
    llm_settings: crate::services::preferences::LlmSettings,
    terms: Vec<crate::services::preferences::TermEntry>,
    usage_pool: sqlx::SqlitePool,
    app_handle: Option<&tauri::AppHandle>,
) -> Result<TranslationPipelineResponse, String> {
    let max_concurrency = request.threads.unwrap_or(4).clamp(1, 16) as usize;
    let llm_client = llm::TranslationLlmClient::new(llm::TranslationLlmRuntimeConfig {
        api_key: llm_settings.api_key,
        base_url: if llm_settings.api_base.trim().is_empty() {
            None
        } else {
            Some(llm_settings.api_base)
        },
        model: llm_settings.api_model,
        max_concurrency,
        timeout_secs: Some(180),
        max_retries: Some(3),
        log_task_id: Some(request.task_id.clone()),
        log_media_path: Some(request.media_path.clone()),
        usage_pool: Some(usage_pool),
    });
    let mut stage_reports = Vec::new();

    let term_candidates = terms
        .into_iter()
        .map(|term| crate::services::translation::domain::TranslationTerm {
            source: term.source,
            target: term.target,
            note: term.note,
        })
        .collect::<Vec<_>>();

    emit_translation_phase(app_handle, &request.task_id, "summary");
    let summary_result = stages::summary::run_stage(
        &llm_client,
        &request.cues,
        &request.source_language,
        &request.target_language,
        request.style.as_deref(),
        &term_candidates,
    )
    .await?;
    stage_reports.push(stage_report_from_result(
        TranslationStage::Summary,
        &summary_result,
        format!(
            "summary ready: terminology_subset={}",
            summary_result.metrics.terminology_subset.len()
        ),
        "summary skipped",
    ));
    let summary = summary_result.metrics;

    // Minimal path: translate directly on existing ASR cue segmentation (no sentence regrouping).
    let units = mapping::cues_to_sentence_units(&request.cues);
    emit_translation_phase(app_handle, &request.task_id, "translate");
    let translate_result = stages::translate::run_stage(
        &llm_client,
        &request.source_language,
        &request.target_language,
        &summary,
        None,
        &units,
    )
    .await?;
    stage_reports.push(stage_report_from_result(
        TranslationStage::Translate,
        &translate_result,
        format!("translated {} cue units", translate_result.metrics.len()),
        "translate skipped",
    ));
    let translated_units = translate_result.metrics;

    let raw_aligned = mapping::align_translated_units_to_cues(&request.cues, &translated_units);
    let source_snapshot = request
        .cues
        .iter()
        .map(|cue| AlignedCue {
            cue_id: cue.cue_id.clone(),
            source_text: cue.source_text.clone(),
            translated_text: String::new(),
        })
        .collect::<Vec<_>>();
    guardrails::assert_source_immutable(&source_snapshot, &raw_aligned)?;

    let align_result = stages::align::run_stage(false, raw_aligned);
    stage_reports.push(stage_report_from_result(
        TranslationStage::Align,
        &align_result,
        format!("aligned {} cues", align_result.metrics.len()),
        "align skipped",
    ));
    let final_cues = align_result.metrics;

    let qa_result = stages::qa::run_stage(false, final_cues);
    stage_reports.push(stage_report_from_result(
        TranslationStage::Qa,
        &qa_result,
        format!(
            "qa completed: issues={}, fixed={}, unresolved={}",
            qa_result.metrics.qa.issue_total,
            qa_result.metrics.qa.fixed_total,
            qa_result.metrics.qa.unresolved_total
        ),
        "qa skipped",
    ));
    let final_cues = qa_result.metrics.cues;
    let qa = qa_result.metrics.qa;

    Ok(TranslationPipelineResponse {
        task_id: request.task_id,
        summary,
        stages: stage_reports,
        cues: final_cues,
        qa,
    })
}

fn emit_translation_phase(app_handle: Option<&tauri::AppHandle>, task_id: &str, phase: &str) {
    if let Some(handle) = app_handle {
        let _ = handle.emit(
            "translate-progress",
            TranslationProgressEvent {
                task_id: task_id.to_string(),
                phase: phase.to_string(),
            },
        );
    }
}

fn stage_report_from_result<T>(
    stage: TranslationStage,
    result: &StageResult<T>,
    success_message: String,
    skipped_message: &str,
) -> StageReport {
    let mut message = if result.executed {
        success_message
    } else {
        skipped_message.to_string()
    };
    if !result.warnings.is_empty() {
        message.push_str(&format!(" | warnings: {}", result.warnings.join("; ")));
    }
    StageReport {
        stage,
        status: if result.executed {
            StageStatus::Completed
        } else {
            StageStatus::Skipped
        },
        message,
    }
}
