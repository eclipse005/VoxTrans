use serde::Deserialize;
use tauri::{AppHandle, Manager};

use crate::db::store::TaskStore;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportTaskSrtsCommandRequest {
    pub task_id: String,
    pub target_dir: String,
    #[serde(default)]
    pub task_name: Option<String>,
    pub items: Vec<crate::services::subtitle_srt::ExportSrtItem>,
}

#[tauri::command]
pub fn export_task_srts(
    app: AppHandle,
    request: ExportTaskSrtsCommandRequest,
) -> Result<Vec<String>, String> {
    crate::services::app_log::report("file", "EXPORT_TASK_SRTS_FAILED", {
        let store = app.state::<TaskStore>().inner();
        crate::services::file::export_task_srts(
            store,
            crate::services::file::ExportTaskSrtsRequest {
                task_id: request.task_id,
                target_dir: request.target_dir,
                task_name: request.task_name,
                items: request.items,
            },
        )
    })
}

#[tauri::command]
pub fn get_file_size(path: String) -> Result<u64, String> {
    crate::services::app_log::report(
        "file",
        "GET_FILE_SIZE_FAILED",
        crate::services::file::get_file_size(path),
    )
}
