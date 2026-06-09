use tokio::runtime::Handle;

pub use super::preferences_types::{
    SaveAppSettingsRequest, SavedSettings, SubtitleLayoutStyle, SubtitleLineStyle,
    SubtitleRenderStyle, TerminologyGroup, TerminologyTerm, UserPreferencesResponse,
};

pub async fn load_user_preferences(
    store: &crate::db::store::TaskStore,
) -> Result<UserPreferencesResponse, String> {
    let settings = store.load_settings().await?;
    Ok(UserPreferencesResponse { settings })
}

pub async fn save_app_settings(
    store: &crate::db::store::TaskStore,
    request: &SaveAppSettingsRequest,
) -> Result<(), String> {
    let normalized = super::preferences_normalize::normalize_saved_settings(request.settings.clone());
    store.save_settings(&normalized).await
}

/// Synchronous wrapper for legacy callers (sync helpers inside pipeline hot paths).
///
/// Safe to call from inside an async context: when a tokio multi-thread runtime is
/// active we use `block_in_place` so the worker thread is freed up; otherwise we
/// fall back to `tauri::async_runtime::block_on` which is safe from sync code.
pub fn load_saved_settings_from_default_path(
    store: &crate::db::store::TaskStore,
) -> Result<SavedSettings, String> {
    let load = store.load_settings();

    let result = match Handle::try_current() {
        Ok(handle) if handle.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread => {
            tokio::task::block_in_place(|| handle.block_on(load))
        }
        _ => tauri::async_runtime::block_on(load),
    };

    result.or_else(|_| Ok(super::preferences_normalize::default_settings()))
}
