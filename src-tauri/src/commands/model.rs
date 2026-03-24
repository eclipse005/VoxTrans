use tauri::State;

use crate::app_state::AppState;
use crate::services::model;

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelTargetCommandRequest {
    pub target: String,
    pub model: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelDownloadPhaseCommand {
    Idle,
    Downloading,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelDownloadStateCommand {
    pub phase: ModelDownloadPhaseCommand,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub speed_bytes_per_sec: u64,
    pub message: String,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelStatusCommandResponse {
    pub target: String,
    pub model: String,
    pub model_dir: String,
    pub required_files: Vec<String>,
    pub missing_files: Vec<String>,
    pub ready: bool,
    pub download: ModelDownloadStateCommand,
}

#[tauri::command]
pub fn get_model_status(
    state: State<'_, AppState>,
    request: ModelTargetCommandRequest,
) -> Result<ModelStatusCommandResponse, String> {
    let target = to_service_target(&request.target)?;
    model::get_model_status(&state, target, request.model).map(from_service_model_status)
}

#[tauri::command]
pub fn start_model_download(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    request: ModelTargetCommandRequest,
) -> Result<(), String> {
    let target = to_service_target(&request.target)?;
    model::start_model_download(app, &state, target, request.model)
}

#[tauri::command]
pub fn cancel_model_download(
    state: State<'_, AppState>,
    request: ModelTargetCommandRequest,
) -> Result<(), String> {
    let target = to_service_target(&request.target)?;
    model::cancel_model_download(&state, target, request.model)
}

#[tauri::command]
pub fn open_model_dir(request: ModelTargetCommandRequest) -> Result<(), String> {
    let target = to_service_target(&request.target)?;
    model::open_model_dir(target)
}

fn to_service_target(target: &str) -> Result<model::ModelTarget, String> {
    match target.trim().to_ascii_lowercase().as_str() {
        "asr" => Ok(model::ModelTarget::Asr),
        "demucs" => Ok(model::ModelTarget::Demucs),
        _ => Err("target must be asr or demucs".to_string()),
    }
}

fn from_service_model_status(
    response: model::ModelStatusResponse,
) -> ModelStatusCommandResponse {
    ModelStatusCommandResponse {
        target: match response.target {
            model::ModelTarget::Asr => "asr".to_string(),
            model::ModelTarget::Demucs => "demucs".to_string(),
        },
        model: response.model,
        model_dir: response.model_dir,
        required_files: response.required_files,
        missing_files: response.missing_files,
        ready: response.ready,
        download: ModelDownloadStateCommand {
            phase: match response.download.phase {
                crate::app_state::ModelDownloadPhase::Idle => ModelDownloadPhaseCommand::Idle,
                crate::app_state::ModelDownloadPhase::Downloading => {
                    ModelDownloadPhaseCommand::Downloading
                }
                crate::app_state::ModelDownloadPhase::Completed => {
                    ModelDownloadPhaseCommand::Completed
                }
                crate::app_state::ModelDownloadPhase::Failed => ModelDownloadPhaseCommand::Failed,
                crate::app_state::ModelDownloadPhase::Cancelled => {
                    ModelDownloadPhaseCommand::Cancelled
                }
            },
            downloaded_bytes: response.download.downloaded_bytes,
            total_bytes: response.download.total_bytes,
            speed_bytes_per_sec: response.download.speed_bytes_per_sec,
            message: response.download.message,
        },
    }
}
