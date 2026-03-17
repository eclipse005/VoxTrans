use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenTaskOutputDirRequest {
    pub task_id: String,
    pub media_path: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenTaskLogDirRequest {
    pub task_id: String,
}

#[tauri::command]
pub fn open_in_explorer(path: String) -> Result<(), String> {
    let target = PathBuf::from(path);
    crate::services::system::open_path(&target)
}

#[tauri::command]
pub fn open_output_dir() -> Result<(), String> {
    let output_dir = crate::services::output::resolve_output_dir();
    std::fs::create_dir_all(&output_dir).map_err(|err| err.to_string())?;
    crate::services::system::open_path(&output_dir)
}

#[tauri::command]
pub fn open_task_output_dir(request: OpenTaskOutputDirRequest) -> Result<(), String> {
    if request.task_id.trim().is_empty() || request.media_path.trim().is_empty() {
        return open_output_dir();
    }

    let task_dir = crate::services::task_path::task_output_dir(
        &request.task_id,
        Path::new(&request.media_path),
    );
    std::fs::create_dir_all(&task_dir).map_err(|err| err.to_string())?;
    crate::services::system::open_path(&task_dir)
}

#[tauri::command]
pub fn open_task_log_dir(request: OpenTaskLogDirRequest) -> Result<(), String> {
    if request.task_id.trim().is_empty() {
        return open_output_dir();
    }

    let log_dir = crate::services::task_path::task_log_dir(&request.task_id);
    std::fs::create_dir_all(&log_dir).map_err(|err| err.to_string())?;
    crate::services::system::open_path(&log_dir)
}
