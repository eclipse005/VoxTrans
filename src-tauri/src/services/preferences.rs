use std::fs;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

use super::preferences_normalize::{default_settings, normalize_saved_settings};
pub use super::preferences_types::{
    SaveAppSettingsRequest, SavedSettings, SubtitleLayoutStyle, SubtitleLineStyle,
    SubtitleRenderStyle, TerminologyGroup, TerminologyTerm, UserPreferencesResponse,
};

const SETTINGS_FILE_NAME: &str = "settings.json";

pub async fn load_user_preferences(app: &AppHandle) -> Result<UserPreferencesResponse, String> {
    let settings = load_settings(app)?;
    Ok(UserPreferencesResponse { settings })
}

pub fn load_saved_settings_from_default_path() -> Result<SavedSettings, String> {
    let path = default_settings_path()?;
    load_settings_from_path(&path)
}

pub async fn save_app_settings(
    app: &AppHandle,
    request: &SaveAppSettingsRequest,
) -> Result<(), String> {
    let normalized = normalize_saved_settings(request.settings.clone());
    let path = settings_path(app)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let payload = serde_json::to_string_pretty(&normalized).map_err(|err| err.to_string())?;
    fs::write(path, payload).map_err(|e| e.to_string())
}

fn load_settings(app: &AppHandle) -> Result<SavedSettings, String> {
    let path = settings_path(app)?;
    load_settings_from_path(&path)
}

fn load_settings_from_path(path: &PathBuf) -> Result<SavedSettings, String> {
    if !path.exists() {
        return Ok(default_settings());
    }
    let raw = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let parsed = serde_json::from_str::<SavedSettings>(&raw).map_err(|e| e.to_string())?;
    Ok(normalize_saved_settings(parsed))
}

fn settings_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    Ok(dir.join(SETTINGS_FILE_NAME))
}

fn default_settings_path() -> Result<PathBuf, String> {
    let app_data =
        std::env::var_os("APPDATA").ok_or_else(|| "APPDATA is not available".to_string())?;
    Ok(PathBuf::from(app_data)
        .join("com.voxtrans.desktop")
        .join(SETTINGS_FILE_NAME))
}
