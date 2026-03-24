use tauri::State;

use crate::app_state::AppState;

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveSubtitleEditorCommandRequest {
    pub task_id: String,
    pub content: String,
    #[serde(default)]
    pub subtitle_segments_json: Option<String>,
}

#[tauri::command]
pub async fn save_subtitle_editor(
    state: State<'_, AppState>,
    request: SaveSubtitleEditorCommandRequest,
) -> Result<(), String> {
    crate::services::subtitle::save_subtitle_editor(
        &state.pool,
        crate::services::subtitle::SubtitleSaveRequest {
            task_id: request.task_id,
            content: request.content,
            subtitle_segments_json: request.subtitle_segments_json,
        },
    )
    .await
}
