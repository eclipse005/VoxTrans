use tauri::State;

use crate::app_state::AppState;
use crate::services::preferences::{
    self, SaveAppSettingsRequest, SaveHotwordCorrectionRequest, SaveTermsRequest,
    UserPreferencesResponse,
};

#[tauri::command]
pub async fn load_user_preferences(
    state: State<'_, AppState>,
) -> Result<UserPreferencesResponse, String> {
    preferences::load_user_preferences(&state.pool).await
}

#[tauri::command]
pub async fn save_app_settings(
    state: State<'_, AppState>,
    request: SaveAppSettingsRequest,
) -> Result<(), String> {
    preferences::save_app_settings(&state.pool, request).await
}

#[tauri::command]
pub async fn save_terms(
    state: State<'_, AppState>,
    request: SaveTermsRequest,
) -> Result<(), String> {
    preferences::save_terms(&state.pool, request).await
}

#[tauri::command]
pub async fn save_hotword_correction(
    state: State<'_, AppState>,
    request: SaveHotwordCorrectionRequest,
) -> Result<(), String> {
    preferences::save_hotword_correction(&state.pool, request).await
}
