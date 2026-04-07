use serde::{Deserialize, Serialize};
use tauri::State;

use crate::app_state::AppState;
use crate::services::updater;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCheckResponse {
    pub current_version: String,
    pub latest_version: String,
    pub release_name: String,
    pub published_at: String,
    pub notes: String,
    pub html_url: String,
    pub download_url: String,
    pub download_size: u64,
    pub has_update: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadUpdateRequest {
    pub download_url: String,
    pub task_id: String,
}

#[tauri::command]
pub async fn check_update(_state: State<'_, AppState>) -> Result<UpdateCheckResponse, String> {
    let current_version = env!("CARGO_PKG_VERSION").to_string();
    let result = updater::check_update(&current_version).await?;

    Ok(UpdateCheckResponse {
        current_version: result.current_version,
        latest_version: result.latest_version,
        release_name: result.release_name,
        published_at: result.published_at,
        notes: result.notes,
        html_url: result.html_url,
        download_url: result.download_url,
        download_size: result.download_size,
        has_update: result.has_update,
    })
}

#[tauri::command]
pub async fn download_update(
    app: tauri::AppHandle,
    _state: State<'_, AppState>,
    request: DownloadUpdateRequest,
) -> Result<(), String> {
    let app_for_thread = app.clone();
    let url = request.download_url;
    let task_id = request.task_id;

    tauri::async_runtime::spawn_blocking(move || {
        updater::download_and_install(url, task_id, app_for_thread)
    })
    .await
    .map_err(|e| format!("下载任务异常: {}", e))??;

    Ok(())
}

#[tauri::command]
pub fn cancel_update(task_id: String) -> Result<bool, String> {
    Ok(updater::request_cancel(&task_id))
}

#[tauri::command]
pub fn open_external_url(url: String) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/c", "start", &url])
            .spawn()
            .map_err(|e| format!("打开链接失败: {}", e))?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&url)
            .spawn()
            .map_err(|e| format!("打开链接失败: {}", e))?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(&url)
            .spawn()
            .map_err(|e| format!("打开链接失败: {}", e))?;
    }
    Ok(())
}
