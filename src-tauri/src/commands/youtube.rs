use tauri::State;

use crate::app_state::AppState;
use crate::commands::dto::common::{TaskRunCommandRecord, from_service_task_run};
use crate::commands::dto::youtube::{
    YoutubeDownloadProgressCommandEvent, from_service_youtube_progress,
};
use crate::services::youtube::{
    self, download_youtube_to_task,
    get_youtube_download_progress as get_youtube_download_progress_service,
    get_yt_dlp_version as get_yt_dlp_version_service,
    list_youtube_download_progress as list_youtube_download_progress_service,
    request_cancel_youtube_download as request_cancel_youtube_download_service,
    update_yt_dlp as update_yt_dlp_service,
};

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadYoutubeCommandRequest {
    pub url: String,
    #[serde(default)]
    pub task_id: String,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadYoutubeCommandResponse {
    pub task: TaskRunCommandRecord,
    pub output_dir: String,
    pub downloaded_path: String,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateYtDlpCommandResponse {
    pub from_version: String,
    pub to_version: String,
    pub updated: bool,
    pub output: String,
}

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
    request: DownloadYoutubeCommandRequest,
) -> Result<DownloadYoutubeCommandResponse, String> {
    download_youtube_to_task(
        &state.pool,
        Some(app),
        youtube::DownloadYoutubeRequest {
            url: request.url,
            task_id: request.task_id,
        },
    )
    .await
    .map(|response| DownloadYoutubeCommandResponse {
        task: from_service_task_run(response.task),
        output_dir: response.output_dir,
        downloaded_path: response.downloaded_path,
    })
}

#[tauri::command]
pub fn get_youtube_download_progress(
    request: GetYoutubeDownloadProgressRequest,
) -> YoutubeDownloadProgressCommandEvent {
    from_service_youtube_progress(get_youtube_download_progress_service(
        request.task_id.trim(),
    ))
}

#[tauri::command]
pub fn list_youtube_download_progress() -> Vec<YoutubeDownloadProgressCommandEvent> {
    list_youtube_download_progress_service()
        .into_iter()
        .map(from_service_youtube_progress)
        .collect()
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
pub fn update_yt_dlp() -> Result<UpdateYtDlpCommandResponse, String> {
    update_yt_dlp_service().map(|response| UpdateYtDlpCommandResponse {
        from_version: response.from_version,
        to_version: response.to_version,
        updated: response.updated,
        output: response.output,
    })
}
