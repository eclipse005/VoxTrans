use tauri::AppHandle;

use crate::domain::error::WorkspaceResult;
use crate::services::workspace_subtitle::{WorkspaceSubtitleSegment, serialize_segments};

use super::patch_task_item;

/// Update the in-flight subtitle preview for a task: writes the segment
/// snapshot into the task item's `subtitle_segments_json` field (which
/// `patch_task_item` persists to the task table and emits via
/// `task-state-changed`).
///
/// This is a *preview* path — it deliberately does NOT touch the structured
/// `subtitle_segments` table. That table is the authoritative persisted
/// state and is written exactly once, by the terminal `finish_*` functions,
/// when a task reaches its final state. Intermediate previews (after
/// segmentation, or per-batch during translation) are transient: streaming
/// them into the structured table would mean a full DELETE + re-INSERT of
/// every segment row and word per batch — pure write amplification for data
/// that is about to be overwritten. On crash/restart, recovery only ever
/// shows finalized state, so leaving the table untouched mid-flight is also
/// the more honest representation.
pub(super) async fn update_subtitle_preview(
    app: &AppHandle,
    task_id: &str,
    source_text: &str,
    segments: Vec<WorkspaceSubtitleSegment>,
) -> WorkspaceResult<()> {
    let subtitle_segments_json = serialize_segments(&segments);
    patch_task_item(app, task_id, |task| {
        task.item.result_text = source_text.to_string();
        task.item.subtitle_segments_json = subtitle_segments_json;
    })
    .await
}
