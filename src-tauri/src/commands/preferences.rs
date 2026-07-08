use crate::db::store::TaskStore;
use crate::services::model::set_models_root_override;
use crate::services::preferences;
use crate::services::preferences_types::{
    DefaultSettingsResponse, SaveAppSettingsRequest, UserPreferencesResponse,
};
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

#[tauri::command]
pub async fn load_user_preferences(app: AppHandle) -> Result<UserPreferencesResponse, String> {
    let store = app.state::<TaskStore>().inner();
    let response = preferences::load_user_preferences(store).await?;
    sync_models_root(&response.settings.models_dir);
    Ok(response)
}

#[tauri::command]
pub async fn save_app_settings(
    app: AppHandle,
    request: SaveAppSettingsRequest,
) -> Result<(), String> {
    let store = app.state::<TaskStore>().inner();
    preferences::save_app_settings(store, &request).await?;
    sync_models_root(&request.settings.models_dir);
    Ok(())
}

#[tauri::command]
pub fn get_default_settings() -> DefaultSettingsResponse {
    DefaultSettingsResponse {
        settings: crate::services::preferences_normalize::default_settings(),
    }
}

fn sync_models_root(models_dir: &Option<String>) {
    let path = models_dir
        .as_ref()
        .map(|d| d.trim())
        .filter(|d| !d.is_empty())
        .map(PathBuf::from);
    set_models_root_override(path);
}
