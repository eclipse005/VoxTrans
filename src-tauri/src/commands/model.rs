use tauri::State;

use crate::app_state::AppState;
use crate::services::model;
use crate::services::model::ModelStatusResponse;

#[tauri::command]
pub fn get_model_status(state: State<'_, AppState>) -> Result<ModelStatusResponse, String> {
    model::get_model_status(&state)
}

#[tauri::command]
pub fn start_model_download(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    model::start_model_download(app, &state)
}

#[tauri::command]
pub fn cancel_model_download(state: State<'_, AppState>) -> Result<(), String> {
    model::cancel_model_download(&state)
}

#[tauri::command]
pub fn open_model_dir() -> Result<(), String> {
    model::open_model_dir()
}
