use tauri::Emitter;
use tauri::async_runtime::spawn_blocking;

use super::transcribe_mapping::from_transcribe_response;
pub use super::transcribe_types::{
    SeparateVocalsCommandRequest, SeparateVocalsCommandResponse, TranscribeCommandRequest,
    TranscribeCommandResponse,
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
                source_lang: request.source_lang,
                asr_model: request.asr_model,
                align_model: request.align_model,
                provider: request.provider,
                chunk_target_seconds: request.chunk_target_seconds,
                model_dir: request.model_dir,
                precomputed_asr_segments: vec![],
                precomputed_alignment: vec![],
            },
            move |stage, current, total, _fresh_result| {
                let _ = app_handle.emit(
                    "transcribe-progress",
                    TranscribeProgressEvent {
                        task_id: task_id.clone(),
                        phase: match stage {
                            crate::services::transcribe::TranscribeProgressStage::Asr => {
                                "asr".to_string()
                            }
                            crate::services::transcribe::TranscribeProgressStage::Align => {
                                "align".to_string()
                            }
                        },
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
