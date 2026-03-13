use crate::services::translation::domain::{
    AlignedCue, QaSummary, StageReport, StageStatus, TranslationPipelineRequest,
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
    let summary = stages::summary::run(
        &llm_client,
        &request.cues,
        &request.source_language,
        &request.target_language,
        request.style.as_deref(),
        &term_candidates,
    )
    .await?;
    stage_reports.push(StageReport {
        stage: TranslationStage::Summary,
        status: StageStatus::Completed,
        message: format!(
            "summary ready: terminology_subset={}",
            summary.terminology_subset.len()
        ),
    });

    // Minimal path: translate directly on existing ASR cue segmentation (no sentence regrouping).
    let units = mapping::cues_to_sentence_units(&request.cues);
    emit_translation_phase(app_handle, &request.task_id, "translate");
    let translated_units = stages::translate::run(
        &llm_client,
        &request.source_language,
        &request.target_language,
        &summary,
        &units,
    )
    .await?;
    stage_reports.push(StageReport {
        stage: TranslationStage::Translate,
        status: StageStatus::Completed,
        message: format!("translated {} cue units", translated_units.len()),
    });

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

    let aligned = raw_aligned;
    stage_reports.push(StageReport {
        stage: TranslationStage::Align,
        status: StageStatus::Skipped,
        message: "stage disabled in current milestone".to_string(),
    });

    let final_cues = aligned;
    let qa = QaSummary {
        issue_total: 0,
        fixed_total: 0,
        unresolved_total: 0,
        issues: Vec::new(),
    };
    stage_reports.push(StageReport {
        stage: TranslationStage::Qa,
        status: StageStatus::Skipped,
        message: "stage disabled in current milestone".to_string(),
    });

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
