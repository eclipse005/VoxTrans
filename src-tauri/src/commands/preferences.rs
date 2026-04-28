use crate::services::preferences;
use tauri::AppHandle;

use super::preferences_mapping::{from_service_settings, to_service_settings};
use super::preferences_types::{SaveAppSettingsCommandRequest, UserPreferencesCommandResponse};

#[tauri::command]
pub async fn load_user_preferences(
    app: AppHandle,
) -> Result<UserPreferencesCommandResponse, String> {
    let response = preferences::load_user_preferences(&app).await?;
    Ok(UserPreferencesCommandResponse {
        settings: from_service_settings(response.settings),
    })
}

#[tauri::command]
pub async fn save_app_settings(
    app: AppHandle,
    request: SaveAppSettingsCommandRequest,
) -> Result<(), String> {
    preferences::save_app_settings(
        &app,
        &crate::services::preferences::SaveAppSettingsRequest {
            settings: to_service_settings(request.settings),
        },
    )
    .await
}
