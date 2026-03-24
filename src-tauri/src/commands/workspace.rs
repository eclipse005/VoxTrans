use tauri::State;

use crate::app_state::AppState;
use crate::services::workspace::{self};

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceTaskCommandRequest {
    pub task_id: String,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueItemCommandRecord {
    pub id: String,
    pub path: String,
    pub name: String,
    pub media_kind: String,
    pub size_bytes: u64,
    pub transcribe_status: String,
    pub transcribe_progress: u32,
    pub transcribe_segment_current: u32,
    pub transcribe_segment_total: u32,
    #[serde(default)]
    pub transcribe_phase: String,
    #[serde(default)]
    pub transcribe_phase_detail: String,
    pub transcribe_error: String,
    pub result_text: String,
    pub result_srt: String,
    pub subtitle_segments_json: String,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceStateCommandResponse {
    pub queue: Vec<QueueItemCommandRecord>,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceTaskCommandResponse {
    pub item: QueueItemCommandRecord,
}

#[tauri::command]
pub async fn load_workspace_state(
    state: State<'_, AppState>,
) -> Result<WorkspaceStateCommandResponse, String> {
    let response = workspace::load_workspace_state(&state.pool).await?;
    Ok(WorkspaceStateCommandResponse {
        queue: response.queue.into_iter().map(from_service_queue_item).collect(),
    })
}

#[tauri::command]
pub async fn load_workspace_task(
    state: State<'_, AppState>,
    request: WorkspaceTaskCommandRequest,
) -> Result<WorkspaceTaskCommandResponse, String> {
    let response = workspace::load_workspace_task(
        &state.pool,
        crate::services::workspace::WorkspaceTaskRequest {
            task_id: request.task_id,
        },
    )
    .await?;
    Ok(WorkspaceTaskCommandResponse {
        item: from_service_queue_item(response.item),
    })
}

fn from_service_queue_item(
    item: crate::services::workspace::QueueItemRecord,
) -> QueueItemCommandRecord {
    QueueItemCommandRecord {
        id: item.id,
        path: item.path,
        name: item.name,
        media_kind: item.media_kind,
        size_bytes: item.size_bytes,
        transcribe_status: item.transcribe_status,
        transcribe_progress: item.transcribe_progress,
        transcribe_segment_current: item.transcribe_segment_current,
        transcribe_segment_total: item.transcribe_segment_total,
        transcribe_phase: item.transcribe_phase,
        transcribe_phase_detail: item.transcribe_phase_detail,
        transcribe_error: item.transcribe_error,
        result_text: item.result_text,
        result_srt: item.result_srt,
        subtitle_segments_json: item.subtitle_segments_json,
    }
}
