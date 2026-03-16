use tauri::State;

use crate::app_state::AppState;
use crate::services::model;
use crate::services::model::{ModelStatusResponse, ModelTarget};

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelTargetRequest {
    pub target: ModelTarget,
    pub model: Option<String>,
}

#[tauri::command]
pub fn get_model_status(
    state: State<'_, AppState>,
    request: ModelTargetRequest,
) -> Result<ModelStatusResponse, String> {
    model::get_model_status(&state, request.target, request.model)
}

#[tauri::command]
pub fn start_model_download(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    request: ModelTargetRequest,
) -> Result<(), String> {
    model::start_model_download(app, &state, request.target, request.model)
}

#[tauri::command]
pub fn cancel_model_download(
    state: State<'_, AppState>,
    request: ModelTargetRequest,
) -> Result<(), String> {
    model::cancel_model_download(&state, request.target, request.model)
}

#[tauri::command]
pub fn open_model_dir(request: ModelTargetRequest) -> Result<(), String> {
    model::open_model_dir(request.target)
}
