use tauri::State;

use crate::app_state::AppState;
use crate::services::logs::{self, AppendTaskLogRequest, ClearTaskLogsRequest, ReadTaskLogRequest};
use crate::services::task_usage;

#[tauri::command]
pub fn append_task_log(request: AppendTaskLogRequest) -> Result<(), String> {
    logs::append_task_log(request)
}

#[tauri::command]
pub fn read_task_log(request: ReadTaskLogRequest) -> Result<String, String> {
    logs::read_task_log(request)
}

#[tauri::command]
pub fn clear_task_logs(request: ClearTaskLogsRequest) -> Result<(), String> {
    logs::clear_task_logs(request)
}

#[tauri::command]
pub async fn get_task_total_tokens(
    _state: State<'_, AppState>,
    task_id: String,
) -> Result<u64, String> {
    task_usage::get_task_total_tokens(&task_id).await
}
