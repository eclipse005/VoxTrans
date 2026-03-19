use tauri::State;

use crate::app_state::AppState;
use crate::services::evaluation::{EvaluateTaskRequest, EvaluateTaskResponse};

#[tauri::command]
pub async fn evaluate_task(
    state: State<'_, AppState>,
    request: EvaluateTaskRequest,
) -> Result<EvaluateTaskResponse, String> {
    crate::services::evaluation::evaluate_task(&state.pool, request).await
}
