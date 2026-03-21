use tauri::State;

use crate::app_state::AppState;
use crate::services::workspace::{self, WorkspaceStateResponse};

#[tauri::command]
pub async fn load_workspace_state(
    state: State<'_, AppState>,
) -> Result<WorkspaceStateResponse, String> {
    workspace::load_workspace_state(&state.pool).await
}
