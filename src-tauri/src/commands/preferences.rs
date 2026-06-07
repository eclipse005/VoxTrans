use crate::db::store::TaskStore;
use crate::services::preferences;
use tauri::{AppHandle, Manager};

use super::preferences_mapping::{from_service_settings, to_service_settings};
use super::preferences_types::{SaveAppSettingsCommandRequest, UserPreferencesCommandResponse};

#[tauri::command]
pub async fn load_user_preferences(
    app: AppHandle,
) -> Result<UserPreferencesCommandResponse, String> {
    let store = app.state::<TaskStore>().inner();
    let response = preferences::load_user_preferences(store).await?;
    Ok(UserPreferencesCommandResponse {
        settings: from_service_settings(response.settings),
    })
}

#[tauri::command]
pub async fn save_app_settings(
    app: AppHandle,
    request: SaveAppSettingsCommandRequest,
) -> Result<(), String> {
    let store = app.state::<TaskStore>().inner();
    preferences::save_app_settings(
        store,
        &crate::services::preferences::SaveAppSettingsRequest {
            settings: to_service_settings(request.settings),
        },
    )
    .await
}
