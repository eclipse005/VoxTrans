#[tauri::command]
pub fn save_srt(request: crate::services::file::SaveSrtRequest) -> Result<(), String> {
    crate::services::file::save_srt(request)
}

#[tauri::command]
pub fn get_file_size(path: String) -> Result<u64, String> {
    crate::services::file::get_file_size(path)
}
