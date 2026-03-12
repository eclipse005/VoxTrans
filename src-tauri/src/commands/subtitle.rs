#[tauri::command]
pub fn load_subtitle_editor(
    request: crate::services::subtitle::SubtitleLoadRequest,
) -> Result<crate::services::subtitle::SubtitleLoadResponse, String> {
    crate::services::subtitle::load_subtitle_editor(request)
}

#[tauri::command]
pub fn save_subtitle_editor(
    request: crate::services::subtitle::SubtitleSaveRequest,
) -> Result<crate::services::subtitle::SubtitleSaveResponse, String> {
    crate::services::subtitle::save_subtitle_editor(request)
}
