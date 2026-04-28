use tauri::AppHandle;

use crate::services::workspace_subtitle::{WorkspaceSubtitleSegment, serialize_segments};

use super::patch_task_item;

pub(super) fn update_subtitle_preview(
    app: &AppHandle,
    task_id: &str,
    source_text: &str,
    segments: Vec<WorkspaceSubtitleSegment>,
) -> Result<(), String> {
    let subtitle_segments_json = serialize_segments(&segments);
    patch_task_item(app, task_id, |task| {
        task.item.result_text = source_text.to_string();
        task.item.subtitle_segments_json = subtitle_segments_json.clone();
    })
}
