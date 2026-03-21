use tauri::State;

use crate::app_state::AppState;
use crate::services::youtube::{
    DownloadYoutubeRequest,
    DownloadYoutubeResponse,
    UpdateYtDlpResponse,
    YoutubeDownloadProgressEvent,
    get_youtube_download_progress as get_youtube_download_progress_service,
    get_yt_dlp_version as get_yt_dlp_version_service,
    list_youtube_download_progress as list_youtube_download_progress_service,
    request_cancel_youtube_download as request_cancel_youtube_download_service,
    update_yt_dlp as update_yt_dlp_service,
    download_youtube_to_task,
};

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetYoutubeDownloadProgressRequest {
    pub task_id: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CancelYoutubeDownloadRequest {
    pub task_id: String,
}

#[tauri::command]
pub async fn download_youtube_to_task_run(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    request: DownloadYoutubeRequest,
) -> Result<DownloadYoutubeResponse, String> {
    download_youtube_to_task(&state.pool, Some(app), request).await
}

#[tauri::command]
pub fn get_youtube_download_progress(request: GetYoutubeDownloadProgressRequest) -> YoutubeDownloadProgressEvent {
    get_youtube_download_progress_service(request.task_id.trim())
}

#[tauri::command]
pub fn list_youtube_download_progress() -> Vec<YoutubeDownloadProgressEvent> {
    list_youtube_download_progress_service()
}

#[tauri::command]
pub fn cancel_youtube_download(request: CancelYoutubeDownloadRequest) -> Result<(), String> {
    if request.task_id.trim().is_empty() {
        return Err("taskId is required".to_string());
    }
    if request_cancel_youtube_download_service(request.task_id.trim()) {
        Ok(())
    } else {
        Err("任务未在下载中".to_string())
    }
}

#[tauri::command]
pub fn get_yt_dlp_version() -> Result<String, String> {
    get_yt_dlp_version_service()
}

#[tauri::command]
pub fn update_yt_dlp() -> Result<UpdateYtDlpResponse, String> {
    update_yt_dlp_service()
}
