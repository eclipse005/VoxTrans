use crate::db::store::TaskStore;
use crate::services::preferences;
use crate::services::preferences_types::{
    DefaultSettingsResponse, SaveAppSettingsRequest, UserPreferencesResponse,
};
use tauri::{AppHandle, Manager};

#[tauri::command]
pub async fn load_user_preferences(app: AppHandle) -> Result<UserPreferencesResponse, String> {
    let store = app.state::<TaskStore>().inner();
    preferences::load_user_preferences(store).await
}

#[tauri::command]
pub async fn save_app_settings(
    app: AppHandle,
    request: SaveAppSettingsRequest,
) -> Result<(), String> {
    let store = app.state::<TaskStore>().inner();
    preferences::save_app_settings(store, &request).await
}

#[tauri::command]
pub fn get_default_settings() -> DefaultSettingsResponse {
    DefaultSettingsResponse {
        settings: crate::services::preferences_normalize::default_settings(),
    }
}
