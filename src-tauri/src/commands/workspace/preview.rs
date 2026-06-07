use tauri::{AppHandle, Manager};

use crate::db::store::TaskStore;
use crate::domain::error::WorkspaceResult;
use crate::services::workspace_subtitle::{WorkspaceSubtitleSegment, serialize_segments};

use super::patch_task_item;

fn parse_segments(
    json: &str,
) -> Vec<crate::services::workspace_subtitle::WorkspaceSubtitleSegment> {
    serde_json::from_str(json).unwrap_or_default()
}

pub(super) async fn update_subtitle_preview(
    app: &AppHandle,
    task_id: &str,
    source_text: &str,
    segments: Vec<WorkspaceSubtitleSegment>,
) -> WorkspaceResult<()> {
    let subtitle_segments_json = serialize_segments(&segments);
    patch_task_item(app, task_id, |task| {
        task.item.result_text = source_text.to_string();
        task.item.subtitle_segments_json = subtitle_segments_json.clone();
    })
    .await?;
    let store = app.state::<TaskStore>().inner().clone();
    let parsed = parse_segments(&subtitle_segments_json);
    if let Err(e) = store.replace_segments(task_id, &parsed).await {
        eprintln!("warn: persist segments {task_id} failed: {e}");
    }
    Ok(())
}
