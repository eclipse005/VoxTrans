use serde::Deserialize;

use crate::services::youtube::{DownloadYoutubeTaskResponse, UpdateYtDlpResponse};

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
    crate::services::app_log::report("youtube", "DOWNLOAD_YOUTUBE_TO_TASK_RUN_FAILED", async {
        tauri::async_runtime::spawn_blocking(move || {
            crate::services::youtube::download_youtube_to_task(&app, request.url, request.task_id)
        })
        .await
        .map_err(|err| format!("YouTube download task failed: {err}"))?
    }.await)
}

#[tauri::command]
pub fn cancel_youtube_download(request: YoutubeDownloadProgressRequest) -> Result<(), String> {
    crate::services::app_log::report("youtube", "CANCEL_YOUTUBE_DOWNLOAD_FAILED", {
        crate::services::youtube::request_cancel(&request.task_id);
        Ok(())
    })
}

#[tauri::command]
pub fn get_yt_dlp_version() -> Result<String, String> {
    crate::services::app_log::report(
        "youtube",
        "GET_YT_DLP_VERSION_FAILED",
        crate::services::youtube::get_yt_dlp_version(),
    )
}

#[tauri::command]
pub async fn update_yt_dlp() -> Result<UpdateYtDlpResponse, String> {
    crate::services::app_log::report("youtube", "UPDATE_YT_DLP_FAILED", async {
        tauri::async_runtime::spawn_blocking(crate::services::youtube::update_yt_dlp)
            .await
            .map_err(|err| format!("yt-dlp update task error: {err}"))?
    }.await)
}
