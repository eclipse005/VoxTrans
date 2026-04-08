use super::{
    DEMUCS_MODEL_DOWNLOAD_FILES, ModelTarget, REQUIRED_ASR_MODEL_FILES, compute_asr_download_bytes,
    resolve_engine_model_dir, runtime_for_target,
};
use crate::app_state::{AppState, ModelDownloadPhase, ModelDownloadStateSnapshot};
use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelStatusResponse {
    pub target: ModelTarget,
    pub model: String,
    pub model_dir: String,
    pub required_files: Vec<String>,
    pub missing_files: Vec<String>,
    pub ready: bool,
    pub download: ModelDownloadStateSnapshot,
}

pub fn get_model_status(
    state: &AppState,
    target: ModelTarget,
    model: Option<String>,
) -> Result<ModelStatusResponse, String> {
    let model_dir = resolve_engine_model_dir(target);
    let runtime = runtime_for_target(state, target);
    let model = normalize_model_name(target, model);

    let (snapshot_in_memory, active_model) = {
        let guard = runtime
            .lock()
            .map_err(|_| "model download state lock poisoned".to_string())?;
        (guard.snapshot.clone(), guard.active_model.clone())
    };

    let (required_files, missing_files, ready, downloaded_bytes, total_bytes) = match target {
        ModelTarget::Asr => {
            let required_files: Vec<String> = REQUIRED_ASR_MODEL_FILES
                .iter()
                .map(|s| s.to_string())
                .collect();
            let missing_files: Vec<String> = REQUIRED_ASR_MODEL_FILES
                .iter()
                .filter(|name| !model_dir.join(name).exists())
                .map(|s| s.to_string())
                .collect();
            let (downloaded_bytes, total_bytes) = compute_asr_download_bytes(&model_dir);
            (
                required_files,
                missing_files.clone(),
                missing_files.is_empty(),
                downloaded_bytes,
                total_bytes,
            )
        }
        ModelTarget::Demucs => {
            let weights_name = format!("{}.safetensors", model);
            let weights = model_dir.join(&weights_name);
            let required_files = vec![weights_name.clone()];
            let missing_files = if weights.exists() {
                Vec::new()
            } else {
                vec![weights_name]
            };
            let ready = missing_files.is_empty();
            let expected_size = DEMUCS_MODEL_DOWNLOAD_FILES
                .iter()
                .find_map(|(name, _url, size)| {
                    if *name == format!("{}.safetensors", model) {
                        Some(*size)
                    } else {
                        None
                    }
                })
                .unwrap_or(0);
            let file_size = std::fs::metadata(&weights).map(|m| m.len()).unwrap_or(0);
            let downloading_this_model = snapshot_in_memory.phase
                == ModelDownloadPhase::Downloading
                && active_model.as_deref() == Some(model.as_str());
            let downloaded_bytes = if ready {
                file_size.max(expected_size)
            } else if downloading_this_model {
                snapshot_in_memory.downloaded_bytes
            } else {
                0
            };
            let total_bytes = if ready {
                file_size.max(expected_size)
            } else if downloading_this_model {
                snapshot_in_memory
                    .total_bytes
                    .max(expected_size)
                    .max(downloaded_bytes)
            } else {
                expected_size
            };
            (
                required_files,
                missing_files,
                ready,
                downloaded_bytes,
                total_bytes,
            )
        }
    };

    let phase = if snapshot_in_memory.phase == ModelDownloadPhase::Downloading {
        ModelDownloadPhase::Downloading
    } else if ready {
        ModelDownloadPhase::Completed
    } else if downloaded_bytes > 0 {
        if snapshot_in_memory.phase == ModelDownloadPhase::Failed {
            ModelDownloadPhase::Failed
        } else if snapshot_in_memory.phase == ModelDownloadPhase::Cancelled {
            ModelDownloadPhase::Cancelled
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
        target,
        model,
        model_dir: model_dir.display().to_string(),
        required_files,
        missing_files,
        ready,
        download: snapshot,
    })
}

fn normalize_model_name(target: ModelTarget, model: Option<String>) -> String {
    match target {
        ModelTarget::Asr => "parakeet-tdt-0.6b-v2".to_string(),
        ModelTarget::Demucs => model.unwrap_or_else(|| "htdemucs_ft".to_string()),
    }
}
