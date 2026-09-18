use serde::Deserialize;
use std::path::Path;

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
    #[serde(default)]
    pub media_path: Option<String>,
}

fn open_output_dir() -> Result<(), String> {
    let output_dir = crate::services::output::resolve_output_dir();
    std::fs::create_dir_all(&output_dir).map_err(|err| err.to_string())?;
    crate::services::system::open_path(&output_dir)
}

#[tauri::command]
pub fn open_task_output_dir(request: OpenTaskOutputDirRequest) -> Result<(), String> {
    crate::services::app_log::guard("system", "OPEN_TASK_OUTPUT_DIR_FAILED", || {
        if request.task_id.trim().is_empty() || request.media_path.trim().is_empty() {
            return open_output_dir();
        }

        let task_dir = crate::services::task_path::task_output_dir(
            &request.task_id,
            Path::new(&request.media_path),
        );
        std::fs::create_dir_all(&task_dir).map_err(|err| err.to_string())?;
        crate::services::system::open_path(&task_dir)
    })
}

#[tauri::command]
pub fn open_task_log_dir(request: OpenTaskLogDirRequest) -> Result<(), String> {
    crate::services::app_log::guard("system", "OPEN_TASK_LOG_DIR_FAILED", || {
        if request.task_id.trim().is_empty() {
            return open_output_dir();
        }

        let media_path = request
            .media_path
            .as_deref()
            .map(|v| v.trim())
            .filter(|v| !v.is_empty())
            .map(Path::new);
        let log_dir = crate::services::task_path::task_log_dir(&request.task_id, media_path);
        std::fs::create_dir_all(&log_dir).map_err(|err| err.to_string())?;
        crate::services::system::open_path(&log_dir)
    })
}

/// Open the app-wide error log directory (`<output root>/logs`). Unlike the task
/// log, it exists even when no task has ever run, so support can ask for it
/// before knowing which task failed.
#[tauri::command]
pub fn open_app_log_dir() -> Result<(), String> {
    crate::services::app_log::guard("system", "OPEN_APP_LOG_DIR_FAILED", || {
        let log_dir = crate::services::app_log::log_dir();
        std::fs::create_dir_all(&log_dir).map_err(|err| err.to_string())?;
        crate::services::system::open_path(&log_dir)
    })
}

#[tauri::command]
pub fn list_system_fonts() -> Result<Vec<String>, String> {
    crate::services::app_log::guard("system", "LIST_SYSTEM_FONTS_FAILED", || {
        let source = font_kit::source::SystemSource::new();
        let mut families = source
            .all_families()
            .map_err(|err| format!("Failed to read system fonts: {err}"))?;
        families.sort();
        families.dedup();
        Ok(families)
    })
}
