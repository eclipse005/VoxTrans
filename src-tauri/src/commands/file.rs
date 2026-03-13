#[tauri::command]
pub fn save_srt(request: crate::services::file::SaveSrtRequest) -> Result<(), String> {
    crate::services::file::save_srt(request)
}

#[tauri::command]
pub fn get_file_size(path: String) -> Result<u64, String> {
    crate::services::file::get_file_size(path)
}

#[tauri::command]
pub fn get_task_translated_srt_path(
    request: crate::services::file::TaskTranslatedSrtPathRequest,
) -> Result<String, String> {
    crate::services::file::get_task_translated_srt_path(request)
}
