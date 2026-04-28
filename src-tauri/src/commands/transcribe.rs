use tauri::Emitter;
use tauri::async_runtime::spawn_blocking;

use super::transcribe_mapping::{
    from_build_segments_response, from_transcribe_response, to_service_word,
};
pub use super::transcribe_types::{
    BuildSegmentsCommandRequest, BuildSegmentsCommandResponse, SeparateVocalsCommandRequest,
    SeparateVocalsCommandResponse, TranscribeCommandRequest, TranscribeCommandResponse,
};
use super::transcribe_types::{SeparateProgressEvent, TranscribeProgressEvent};

#[tauri::command]
pub async fn transcribe(
    app: tauri::AppHandle,
    request: TranscribeCommandRequest,
) -> Result<TranscribeCommandResponse, String> {
    spawn_blocking(move || {
        let task_id = request.task_id.clone();
        let app_handle = app.clone();
        crate::services::transcribe::transcribe_blocking(
            crate::services::transcribe::TranscribeRequest {
                task_id: request.task_id,
                audio_path: request.audio_path,
                provider: request.provider,
                chunk_target_seconds: request.chunk_target_seconds,
                model_dir: request.model_dir,
            },
            move |current, total| {
                let _ = app_handle.emit(
                    "transcribe-progress",
                    TranscribeProgressEvent {
                        task_id: task_id.clone(),
                        current_segment: current,
                        total_segments: total,
                    },
                );
            },
        )
        .map(from_transcribe_response)
    })
    .await
    .map_err(|err| err.to_string())?
}

#[tauri::command]
pub fn build_segments_from_words(
    request: BuildSegmentsCommandRequest,
) -> Result<BuildSegmentsCommandResponse, String> {
    crate::services::transcribe::build_segments_from_words(
        crate::services::transcribe::BuildSegmentsRequest {
            task_id: request.task_id,
            audio_path: request.audio_path,
            words: request.words.into_iter().map(to_service_word).collect(),
            subtitle_max_words_per_segment: request.subtitle_max_words_per_segment,
            segment_mode: request.segment_mode,
        },
    )
    .map(from_build_segments_response)
}

#[tauri::command]
pub async fn separate_vocals(
    app: tauri::AppHandle,
    request: SeparateVocalsCommandRequest,
) -> Result<SeparateVocalsCommandResponse, String> {
    spawn_blocking(move || {
        let task_id = request.task_id.clone();
        let app_handle = app.clone();
        crate::services::demucs::separate_vocals_blocking(
            crate::services::demucs::SeparateVocalsRequest {
                task_id: request.task_id,
                audio_path: request.audio_path,
                model: request.model,
            },
            move |percent| {
                let _ = app_handle.emit(
                    "separate-progress",
                    SeparateProgressEvent {
                        task_id: task_id.clone(),
                        percent,
                    },
                );
            },
        )
        .map(|response| SeparateVocalsCommandResponse {
            vocals_path: response.vocals_path,
        })
    })
    .await
    .map_err(|err| err.to_string())?
}
