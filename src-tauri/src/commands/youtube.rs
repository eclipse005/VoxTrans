use serde::Deserialize;

use crate::services::youtube::{
    DownloadYoutubeTaskResponse, UpdateYtDlpResponse, YoutubeDownloadProgressResponse,
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadYoutubeTaskRequest {
    pub url: String,
    #[serde(default)]
    pub task_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct YoutubeDownloadProgressRequest {
    pub task_id: String,
}

#[tauri::command]
pub async fn download_youtube_to_task_run(
    app: tauri::AppHandle,
    request: DownloadYoutubeTaskRequest,
) -> Result<DownloadYoutubeTaskResponse, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::services::youtube::download_youtube_to_task(&app, request.url, request.task_id)
    })
    .await
    .map_err(|err| format!("YouTube 下载任务异常: {err}"))?
}

#[tauri::command]
pub fn get_youtube_download_progress(
    request: YoutubeDownloadProgressRequest,
) -> Result<YoutubeDownloadProgressResponse, String> {
    crate::services::youtube::get_download_progress(&request.task_id)
}

#[tauri::command]
pub fn list_youtube_download_progress() -> Result<Vec<YoutubeDownloadProgressResponse>, String> {
    crate::services::youtube::list_download_progress()
}

#[tauri::command]
pub fn cancel_youtube_download(request: YoutubeDownloadProgressRequest) -> Result<(), String> {
    crate::services::youtube::request_cancel(&request.task_id);
    Ok(())
}

#[tauri::command]
pub fn get_yt_dlp_version() -> Result<String, String> {
    crate::services::youtube::get_yt_dlp_version()
}

#[tauri::command]
pub async fn update_yt_dlp() -> Result<UpdateYtDlpResponse, String> {
    tauri::async_runtime::spawn_blocking(crate::services::youtube::update_yt_dlp)
        .await
        .map_err(|err| format!("yt-dlp 更新任务异常: {err}"))?
}
