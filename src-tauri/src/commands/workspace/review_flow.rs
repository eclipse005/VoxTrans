//! Stage-review gates: enter review, resume, and SoT-driven deliver helpers.

use tauri::{AppHandle, Manager};

use crate::db::store::TaskStore;
use crate::domain::error::{WorkspaceError, WorkspaceResult};
use crate::domain::task::adapters::{
    step2_segments_to_srt, workspace_subtitle_segments_from_step2_segments,
    workspace_subtitle_segments_from_translation_segments,
};
use crate::services::workspace_subtitle::{
    WorkspaceSubtitleSegment, serialize_segments,
};

use super::output_completion::{deliver_from_sot, persist_workspace_segments};
use super::progress::mark_task_failed;
use super::translation_flow::execute_translate_steps_from_step2;
use super::{
    WorkspaceTaskProgressState, WorkspaceTaskStageState, get_task_record, patch_task_item,
    require_task_id,
};

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateTaskReviewFlagsRequest {
    pub task_id: String,
    #[serde(default)]
    pub review_source: Option<bool>,
    #[serde(default)]
    pub review_target: Option<bool>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResumeTaskAfterReviewRequest {
    pub task_id: String,
    /// `continue` | `finalize` | `finalize_source_only`
    pub action: String,
    #[serde(default)]
    pub subtitle_segments_json: Option<String>,
}

pub(super) fn review_progress_state(code: &str, order: u32) -> WorkspaceTaskProgressState {
    WorkspaceTaskProgressState {
        stage: WorkspaceTaskStageState {
            code: code.to_string(),
            label: String::new(),
            order,
            detail: String::new(),
            current: 0,
            total: 0,
        },
    }
}

/// Task is parked at a human review gate — start/execute/enqueue-full must not run.
pub(super) fn is_awaiting_review_status(status: &str) -> bool {
    matches!(status.trim(), "review_source" | "review_target")
}

/// Reject full pipeline start while a task awaits human review.
/// SoT must stay intact; callers should use `resume_task_after_review` instead.
pub(super) fn reject_full_run_if_awaiting_review(status: &str) -> WorkspaceResult<()> {
    if is_awaiting_review_status(status) {
        return Err(WorkspaceError::InvalidRequest(
            "task is awaiting review; use resume (continue/finalize), not full re-run".to_string(),
        ));
    }
    Ok(())
}

pub(super) async fn read_task_review_flags(
    task_id: &str,
) -> WorkspaceResult<(bool, bool)> {
    let record = get_task_record(task_id)?;
    Ok((record.item.review_source, record.item.review_target))
}

/// Materialize source cues into SoT (beautify optional). Does not mark done or burn.
/// Returns (segments, source_text from those segments, legacy step2 srt from raw step2).
pub(super) async fn materialize_source_sot(
    app: &AppHandle,
    task_id: &str,
    step2_segments: &[crate::commands::transcription::GroupedSentenceSegmentCommandDto],
    enable_subtitle_beautify: bool,
    subtitle_length_preset: &str,
    target_lang: &str,
) -> WorkspaceResult<(Vec<WorkspaceSubtitleSegment>, String, String)> {
    let mut workspace_segments = workspace_subtitle_segments_from_step2_segments(step2_segments);
    if enable_subtitle_beautify {
        crate::services::subtitle_beautify::beautify_workspace_segments(
            &mut workspace_segments,
            subtitle_length_preset,
            target_lang,
        );
    }
    let subtitle_segments_json = serialize_segments(&workspace_segments);
    // Source text follows the (possibly beautified) SoT the user/editor sees.
    let source_text = source_text_from_workspace_segments(&workspace_segments);
    let step2_srt = step2_segments_to_srt(step2_segments);
    persist_workspace_segments(app, task_id, &subtitle_segments_json).await?;
    patch_task_item(app, task_id, |task| {
        task.item.result_text = source_text.clone();
        task.item.result_srt = step2_srt.clone();
        task.item.subtitle_segments_json = subtitle_segments_json.clone();
        task.item.transcribe_error = String::new();
    })
    .await?;
    Ok((workspace_segments, source_text, step2_srt))
}

pub(super) async fn materialize_target_sot(
    app: &AppHandle,
    task_id: &str,
    segments: &[crate::commands::translate_types::BuildTranslationSegmentCommand],
    source_text: &str,
    enable_subtitle_beautify: bool,
    subtitle_length_preset: &str,
    target_lang: &str,
) -> WorkspaceResult<Vec<WorkspaceSubtitleSegment>> {
    let mut workspace_segments = workspace_subtitle_segments_from_translation_segments(segments);
    if enable_subtitle_beautify {
        crate::services::subtitle_beautify::beautify_workspace_segments(
            &mut workspace_segments,
            subtitle_length_preset,
            target_lang,
        );
    }
    let subtitle_segments_json = serialize_segments(&workspace_segments);
    persist_workspace_segments(app, task_id, &subtitle_segments_json).await?;
    patch_task_item(app, task_id, |task| {
        task.item.result_text = source_text.to_string();
        task.item.result_srt = String::new();
        task.item.subtitle_segments_json = subtitle_segments_json.clone();
        task.item.transcribe_error = String::new();
    })
    .await?;
    Ok(workspace_segments)
}

pub(super) async fn enter_review_source(app: &AppHandle, task_id: &str) -> WorkspaceResult<()> {
    patch_task_item(app, task_id, |task| {
        task.item.transcribe_status = "review_source".to_string();
        task.item.task_progress = review_progress_state("awaitingReviewSource", 45);
        task.item.transcribe_error = String::new();
    })
    .await
}

pub(super) async fn enter_review_target(app: &AppHandle, task_id: &str) -> WorkspaceResult<()> {
    patch_task_item(app, task_id, |task| {
        task.item.transcribe_status = "review_target".to_string();
        task.item.task_progress = review_progress_state("awaitingReviewTarget", 85);
        task.item.transcribe_error = String::new();
    })
    .await
}

pub(super) async fn update_task_review_flags_internal(
    app: &AppHandle,
    request: UpdateTaskReviewFlagsRequest,
) -> WorkspaceResult<crate::commands::workspace::WorkspaceQueueItem> {
    let task_id = require_task_id(&request.task_id)?;
    if request.review_source.is_none() && request.review_target.is_none() {
        return Err(WorkspaceError::InvalidRequest(
            "at least one of reviewSource/reviewTarget is required".to_string(),
        ));
    }
    patch_task_item(app, task_id, |task| {
        if let Some(v) = request.review_source {
            task.item.review_source = v;
        }
        if let Some(v) = request.review_target {
            task.item.review_target = v;
        }
    })
    .await?;
    Ok(get_task_record(task_id)?.item)
}

pub(super) async fn resume_task_after_review_internal(
    app: &AppHandle,
    request: ResumeTaskAfterReviewRequest,
) -> WorkspaceResult<()> {
    let task_id = require_task_id(&request.task_id)?.to_string();
    let action = request.action.trim().to_ascii_lowercase();

    // 1) Validate status × action *before* any SoT mutation.
    let record = get_task_record(&task_id)?;
    let status = record.item.transcribe_status.as_str();
    validate_resume_action(status, &action)?;

    // 2) Flush editor draft only while parked at a review gate.
    if let Some(json) = request.subtitle_segments_json.as_deref() {
        let trimmed = json.trim();
        if !trimmed.is_empty() {
            flush_review_sot(app, &task_id, trimmed).await?;
        }
    }

    match action.as_str() {
        "continue" => continue_translation_from_source_review(app, &task_id).await,
        // Bilingual deliver after translation review only.
        "finalize" => finalize_from_sot(app, &task_id, true).await,
        // Abandon translate at source gate: write source-only outputs.
        "finalize_source_only" => finalize_from_sot(app, &task_id, false).await,
        other => Err(WorkspaceError::InvalidRequest(format!(
            "unknown resume action: {other}"
        ))),
    }
}

fn validate_resume_action(status: &str, action: &str) -> WorkspaceResult<()> {
    match action {
        "continue" => {
            if status != "review_source" {
                return Err(WorkspaceError::InvalidRequest(
                    "REVIEW_RESUME_INVALID_STATE: continue requires review_source".to_string(),
                ));
            }
            Ok(())
        }
        "finalize" => {
            if status != "review_target" {
                return Err(WorkspaceError::InvalidRequest(
                    "REVIEW_RESUME_INVALID_STATE: finalize requires review_target".to_string(),
                ));
            }
            Ok(())
        }
        "finalize_source_only" => {
            if status != "review_source" {
                return Err(WorkspaceError::InvalidRequest(
                    "REVIEW_RESUME_INVALID_STATE: finalize_source_only requires review_source"
                        .to_string(),
                ));
            }
            Ok(())
        }
        other => Err(WorkspaceError::InvalidRequest(format!(
            "unknown resume action: {other}"
        ))),
    }
}

async fn flush_review_sot(app: &AppHandle, task_id: &str, json: &str) -> WorkspaceResult<()> {
    // Reject malformed JSON rather than silently writing [].
    let segments: Vec<WorkspaceSubtitleSegment> = serde_json::from_str(json).map_err(|e| {
        WorkspaceError::InvalidRequest(format!("invalid subtitleSegmentsJson: {e}"))
    })?;
    if segments.is_empty() {
        return Err(WorkspaceError::InvalidRequest(
            "subtitleSegmentsJson has no cues".to_string(),
        ));
    }
    let normalized = serialize_segments(&segments);
    persist_workspace_segments(app, task_id, &normalized).await?;
    patch_task_item(app, task_id, |task| {
        task.item.subtitle_segments_json = normalized;
    })
    .await
}

/// Marker written on the task so the shared worker runs translation-only.
pub(super) const RESUME_FROM_TRANSLATE: &str = "translate";

/// Prepare continue after source review: flush is already applied; invalidate
/// caches, clear stale translations, mark `queued` + `resume_from=translate`,
/// and prioritize the task. The heavy translate leg is run by the normal
/// execute worker (same single-flight queue as other jobs).
async fn continue_translation_from_source_review(
    app: &AppHandle,
    task_id: &str,
) -> WorkspaceResult<()> {
    let mut workspace_segments = load_sot_segments(app, task_id).await?;
    if workspace_segments.is_empty() {
        return Err(WorkspaceError::InvalidRequest(
            "REVIEW_EMPTY_SOURCE: no source cues to translate".to_string(),
        ));
    }

    invalidate_translation_caches(app, task_id).await?;

    for seg in &mut workspace_segments {
        seg.translated_text.clear();
    }
    let cleared_json = serialize_segments(&workspace_segments);
    persist_workspace_segments(app, task_id, &cleared_json).await?;

    // Head-of-line: stay the current job when the worker picks next.
    let store = app.state::<TaskStore>().inner();
    store
        .prioritize_task_enqueue(task_id)
        .await
        .map_err(WorkspaceError::TaskFailed)?;

    patch_task_item(app, task_id, |task| {
        task.item.subtitle_segments_json = cleared_json;
        task.item.transcribe_status = "queued".to_string();
        task.item.task_progress = WorkspaceTaskProgressState::default();
        task.item.transcribe_error = String::new();
        task.item.resume_from = RESUME_FROM_TRANSLATE.to_string();
    })
    .await?;
    Ok(())
}

/// Worker entry: translation-only resume after source review (or equivalent).
pub(super) async fn execute_resume_translate_from_sot(
    app: &AppHandle,
    task_id: &str,
) -> WorkspaceResult<()> {
    // Clear the marker first so a crash mid-run does not loop forever on resume.
    patch_task_item(app, task_id, |task| {
        task.item.resume_from = String::new();
        task.item.transcribe_status = "processing".to_string();
        task.item.transcribe_error = String::new();
    })
    .await?;

    let mut workspace_segments = load_sot_segments(app, task_id).await?;
    if workspace_segments.is_empty() {
        let err = WorkspaceError::InvalidRequest(
            "RESUME_TRANSLATE: no source cues to translate".to_string(),
        );
        let _ = mark_task_failed(app, task_id, &err.to_string()).await;
        return Err(err);
    }
    for seg in &mut workspace_segments {
        seg.translated_text.clear();
    }
    let step2 = crate::services::subtitle_import::workspace_segments_to_step2(&workspace_segments);
    let source_text = source_text_from_workspace_segments(&workspace_segments);

    match execute_translate_steps_from_step2(app, task_id, &step2, source_text).await {
        Ok(()) => Ok(()),
        Err(err) => {
            let msg = err.to_string();
            let _ = mark_task_failed(app, task_id, &msg).await;
            Err(err)
        }
    }
}

async fn invalidate_translation_caches(app: &AppHandle, task_id: &str) -> WorkspaceResult<()> {
    let store = app.state::<TaskStore>().inner();
    store
        .delete_translation_batches(task_id)
        .await
        .map_err(|e| WorkspaceError::TaskFailed(format!("invalidate translation batches: {e}")))?;
    store
        .delete_artifact(task_id, "step_03_terminology")
        .await
        .map_err(|e| WorkspaceError::TaskFailed(format!("invalidate step3 artifact: {e}")))?;
    store
        .delete_artifact(task_id, "step_04_translation")
        .await
        .map_err(|e| WorkspaceError::TaskFailed(format!("invalidate step4 artifact: {e}")))?;
    Ok(())
}

async fn finalize_from_sot(
    app: &AppHandle,
    task_id: &str,
    include_translation_variants: bool,
) -> WorkspaceResult<()> {
    let record = get_task_record(task_id)?;
    let segments = load_sot_segments(app, task_id).await?;
    if segments.is_empty() {
        return Err(WorkspaceError::InvalidRequest(
            "no subtitle segments to deliver".to_string(),
        ));
    }
    let source_text = source_text_from_workspace_segments(&segments);
    let source_text = if source_text.is_empty() {
        record.item.result_text.clone()
    } else {
        source_text
    };
    deliver_from_sot(
        app,
        task_id,
        &record.item.path,
        &record.item.media_kind,
        &segments,
        include_translation_variants,
        &source_text,
    )
    .await
}

/// Prefer in-memory/task JSON SoT; fall back to structured segments table.
async fn load_sot_segments(
    app: &AppHandle,
    task_id: &str,
) -> WorkspaceResult<Vec<WorkspaceSubtitleSegment>> {
    let record = get_task_record(task_id)?;
    let from_json: Result<Vec<WorkspaceSubtitleSegment>, _> =
        serde_json::from_str(&record.item.subtitle_segments_json);
    if let Ok(segs) = from_json {
        if !segs.is_empty() {
            return Ok(segs);
        }
    }
    let store = app.state::<TaskStore>().inner();
    store
        .load_segments(task_id)
        .await
        .map_err(|e| WorkspaceError::TaskFailed(e))
}

fn source_text_from_workspace_segments(segments: &[WorkspaceSubtitleSegment]) -> String {
    segments
        .iter()
        .map(|s| s.source_text.trim())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::{
        is_awaiting_review_status, reject_full_run_if_awaiting_review, validate_resume_action,
    };

    #[test]
    fn review_statuses_are_detected() {
        assert!(is_awaiting_review_status("review_source"));
        assert!(is_awaiting_review_status("review_target"));
        assert!(!is_awaiting_review_status("processing"));
        assert!(!is_awaiting_review_status("done"));
        assert!(!is_awaiting_review_status("queued"));
    }

    #[test]
    fn full_run_rejected_only_while_awaiting_review() {
        assert!(reject_full_run_if_awaiting_review("review_source").is_err());
        assert!(reject_full_run_if_awaiting_review("review_target").is_err());
        assert!(reject_full_run_if_awaiting_review("queued").is_ok());
        assert!(reject_full_run_if_awaiting_review("processing").is_ok());
        assert!(reject_full_run_if_awaiting_review("done").is_ok());
    }

    #[test]
    fn resume_action_validation_matrix() {
        assert!(validate_resume_action("review_source", "continue").is_ok());
        assert!(validate_resume_action("review_target", "continue").is_err());
        assert!(validate_resume_action("review_target", "finalize").is_ok());
        assert!(validate_resume_action("review_source", "finalize").is_err());
        assert!(validate_resume_action("review_source", "finalize_source_only").is_ok());
        assert!(validate_resume_action("done", "continue").is_err());
        assert!(validate_resume_action("processing", "finalize").is_err());
    }
}
