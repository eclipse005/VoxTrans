use tauri::{AppHandle, Manager};

use crate::db::store::TaskStore;
use crate::services::logs;
use crate::services::task_usage;

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
pub fn read_task_log(request: ReadTaskLogCommandRequest) -> Result<String, String> {
    crate::services::app_log::report(
        "logs",
        "READ_TASK_LOG_FAILED",
        logs::read_task_log(logs::ReadTaskLogRequest {
            task_id: request.task_id,
            media_path: request.media_path,
            channel: request.channel,
        }),
    )
}

#[tauri::command]
pub fn clear_task_logs(request: ClearTaskLogsCommandRequest) -> Result<(), String> {
    crate::services::app_log::report(
        "logs",
        "CLEAR_TASK_LOGS_FAILED",
        logs::clear_task_logs(logs::ClearTaskLogsRequest {
            task_id: request.task_id,
            media_path: request.media_path,
            channel: request.channel,
        }),
    )
}

#[tauri::command]
pub async fn get_task_total_tokens(app: AppHandle, task_id: String) -> Result<u64, String> {
    crate::services::app_log::report("logs", "GET_TASK_TOTAL_TOKENS_FAILED", async {
        let store = app.state::<TaskStore>().inner().clone();
        task_usage::get_task_total_tokens(&task_id, &store).await
    }.await)
}
