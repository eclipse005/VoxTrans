use tauri::State;

use crate::app_state::AppState;

#[tauri::command]
pub fn save_srt(request: crate::services::file::SaveSrtRequest) -> Result<(), String> {
    crate::services::file::save_srt(request)
}

#[tauri::command]
pub fn export_srt(request: crate::services::file::ExportSrtRequest) -> Result<String, String> {
    crate::services::file::export_srt(request)
}

#[tauri::command]
pub async fn export_task_srts(
    state: State<'_, AppState>,
    request: crate::services::file::ExportTaskSrtsRequest,
) -> Result<Vec<String>, String> {
    crate::services::file::export_task_srts(&state.pool, request).await
}

#[tauri::command]
pub fn get_file_size(path: String) -> Result<u64, String> {
    crate::services::file::get_file_size(path)
}
