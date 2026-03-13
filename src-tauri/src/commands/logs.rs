use crate::services::logs::{self, AppendTaskLogRequest, ClearTaskLogsRequest, ReadTaskLogRequest};

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
