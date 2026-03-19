use tauri::State;

use crate::app_state::AppState;

#[tauri::command]
pub async fn save_subtitle_editor(
    state: State<'_, AppState>,
    request: crate::services::subtitle::SubtitleSaveRequest,
) -> Result<(), String> {
    crate::services::subtitle::save_subtitle_editor(&state.pool, request).await
}
