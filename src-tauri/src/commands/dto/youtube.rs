#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct YoutubeDownloadProgressCommandEvent {
    pub task_id: String,
    pub phase: String,
    pub progress_percent: u32,
    pub title: String,
    pub speed: String,
    pub total_size: String,
    pub downloaded_size: String,
    pub eta: String,
    pub message: String,
}

pub fn from_service_youtube_progress(
    event: crate::services::youtube::YoutubeDownloadProgressEvent,
) -> YoutubeDownloadProgressCommandEvent {
    YoutubeDownloadProgressCommandEvent {
        task_id: event.task_id,
        phase: event.phase,
        progress_percent: event.progress_percent,
        title: event.title,
        speed: event.speed,
        total_size: event.total_size,
        downloaded_size: event.downloaded_size,
        eta: event.eta,
        message: event.message,
    }
}
