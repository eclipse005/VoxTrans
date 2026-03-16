use tauri::Emitter;
use tauri::async_runtime::spawn_blocking;

#[derive(Debug, serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct TranscribeProgressEvent {
    task_id: String,
    current_segment: usize,
    total_segments: usize,
}

#[derive(Debug, serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct SeparateProgressEvent {
    task_id: String,
    percent: u32,
}

#[tauri::command]
pub async fn transcribe(
    app: tauri::AppHandle,
    request: crate::services::transcribe::TranscribeRequest,
) -> Result<crate::services::transcribe::TranscribeResponse, String> {
    spawn_blocking(move || {
        let task_id = request.task_id.clone();
        let app_handle = app.clone();
        crate::services::transcribe::transcribe_blocking(request, move |current, total| {
            let _ = app_handle.emit(
                "transcribe-progress",
                TranscribeProgressEvent {
                    task_id: task_id.clone(),
                    current_segment: current,
                    total_segments: total,
                },
            );
        })
    })
    .await
    .map_err(|err| err.to_string())?
}

#[tauri::command]
pub fn build_segments_from_words(
    request: crate::services::transcribe::BuildSegmentsRequest,
) -> Result<crate::services::transcribe::BuildSegmentsResponse, String> {
    crate::services::transcribe::build_segments_from_words(request)
}

#[tauri::command]
pub async fn separate_vocals(
    app: tauri::AppHandle,
    request: crate::services::demucs::SeparateVocalsRequest,
) -> Result<crate::services::demucs::SeparateVocalsResponse, String> {
    spawn_blocking(move || {
        let task_id = request.task_id.clone();
        let app_handle = app.clone();
        crate::services::demucs::separate_vocals_blocking(request, move |percent| {
            let _ = app_handle.emit(
                "separate-progress",
                SeparateProgressEvent {
                    task_id: task_id.clone(),
                    percent,
                },
            );
        })
    })
    .await
    .map_err(|err| err.to_string())?
}
