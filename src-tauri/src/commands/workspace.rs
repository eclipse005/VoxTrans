use tauri::State;

use crate::app_state::AppState;
use crate::services::workspace::{self, SaveQueueStateRequest, WorkspaceStateResponse};

#[tauri::command]
pub async fn load_workspace_state(
    state: State<'_, AppState>,
) -> Result<WorkspaceStateResponse, String> {
    workspace::load_workspace_state(&state.pool).await
}

#[tauri::command]
pub async fn save_queue_state(
    state: State<'_, AppState>,
    request: SaveQueueStateRequest,
) -> Result<(), String> {
    workspace::save_queue_state(&state.pool, request).await
}
