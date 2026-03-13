use tauri::Emitter;
use tauri::State;

use crate::app_state::AppState;

#[derive(Debug, serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct TranscribePhaseEvent {
    task_id: String,
    phase: String,
}

#[tauri::command]
pub async fn run_post_asr_pipeline(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    request: crate::services::transcription::RunPostAsrPipelineRequest,
) -> Result<crate::services::transcription::RunPostAsrPipelineResponse, String> {
    let task_id = request.task_id.clone();

    let response =
        crate::services::transcription::run_post_asr_pipeline(request, &state.pool, move |phase| {
            let _ = app.emit(
                "transcribe-phase",
                TranscribePhaseEvent {
                    task_id: task_id.clone(),
                    phase: phase.to_string(),
                },
            );
        })
        .await?;

    Ok(response)
}
