use super::{REQUIRED_MODEL_FILES, compute_model_download_bytes, resolve_model_dir};
use crate::app_state::{AppState, ModelDownloadPhase, ModelDownloadStateSnapshot};
use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelStatusResponse {
    pub model_dir: String,
    pub required_files: Vec<String>,
    pub missing_files: Vec<String>,
    pub ready: bool,
    pub download: ModelDownloadStateSnapshot,
}

pub fn get_model_status(state: &AppState) -> Result<ModelStatusResponse, String> {
    let model_dir = resolve_model_dir();
    let required_files: Vec<String> = REQUIRED_MODEL_FILES.iter().map(|s| s.to_string()).collect();
    let missing_files: Vec<String> = REQUIRED_MODEL_FILES
        .iter()
        .filter(|name| !model_dir.join(name).exists())
        .map(|s| s.to_string())
        .collect();

    let snapshot_in_memory = state
        .model_download
        .lock()
        .map_err(|_| "model download state lock poisoned".to_string())?
        .snapshot
        .clone();
    let (downloaded_bytes, total_bytes) = compute_model_download_bytes(&model_dir);
    let phase = if snapshot_in_memory.phase == ModelDownloadPhase::Downloading {
        ModelDownloadPhase::Downloading
    } else if missing_files.is_empty() {
        ModelDownloadPhase::Completed
    } else if downloaded_bytes > 0 {
        if snapshot_in_memory.phase == ModelDownloadPhase::Cancelled {
            ModelDownloadPhase::Cancelled
        } else if snapshot_in_memory.phase == ModelDownloadPhase::Failed {
            ModelDownloadPhase::Failed
        } else {
            ModelDownloadPhase::Idle
        }
    } else {
        ModelDownloadPhase::Idle
    };
    let snapshot = ModelDownloadStateSnapshot {
        phase,
        downloaded_bytes,
        total_bytes,
        speed_bytes_per_sec: if snapshot_in_memory.phase == ModelDownloadPhase::Downloading {
            snapshot_in_memory.speed_bytes_per_sec
        } else {
            0
        },
        message: snapshot_in_memory.message,
    };

    Ok(ModelStatusResponse {
        model_dir: model_dir.display().to_string(),
        required_files,
        missing_files: missing_files.clone(),
        ready: missing_files.is_empty(),
        download: snapshot,
    })
}
