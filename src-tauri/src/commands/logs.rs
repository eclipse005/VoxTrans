use tauri::{AppHandle, Manager};

use crate::db::store::TaskStore;
use crate::services::logs;
use crate::services::task_usage;

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppendTaskLogCommandRequest {
    pub task_id: String,
    #[serde(default)]
    pub media_path: Option<String>,
    pub channel: String,
    pub message: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadTaskLogCommandRequest {
    pub task_id: String,
    #[serde(default)]
    pub media_path: Option<String>,
    pub channel: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClearTaskLogsCommandRequest {
    pub task_id: String,
    #[serde(default)]
    pub media_path: Option<String>,
    pub channel: Option<String>,
}

#[tauri::command]
pub fn append_task_log(request: AppendTaskLogCommandRequest) -> Result<(), String> {
    logs::append_task_log(logs::AppendTaskLogRequest {
        task_id: request.task_id,
        media_path: request.media_path,
        channel: request.channel,
        message: request.message,
    })
}

#[tauri::command]
pub fn read_task_log(request: ReadTaskLogCommandRequest) -> Result<String, String> {
    logs::read_task_log(logs::ReadTaskLogRequest {
        task_id: request.task_id,
        media_path: request.media_path,
        channel: request.channel,
    })
}

#[tauri::command]
pub fn clear_task_logs(request: ClearTaskLogsCommandRequest) -> Result<(), String> {
    logs::clear_task_logs(logs::ClearTaskLogsRequest {
        task_id: request.task_id,
        media_path: request.media_path,
        channel: request.channel,
    })
}

#[tauri::command]
pub async fn get_task_total_tokens(app: AppHandle, task_id: String) -> Result<u64, String> {
    let store = app.state::<TaskStore>().inner().clone();
    task_usage::get_task_total_tokens(&task_id, &store).await
}
