use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::Emitter;

use crate::app_state::{ModelDownloadPhase, ModelDownloadRuntime, ModelDownloadStateSnapshot};

use super::ModelTarget;

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct ModelDownloadProgressEvent {
    target: ModelTarget,
    model: String,
    phase: ModelDownloadPhase,
    downloaded_bytes: u64,
    total_bytes: u64,
    speed_bytes_per_sec: u64,
    message: String,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn set_model_download_snapshot(
    app: &tauri::AppHandle,
    runtime: &Arc<Mutex<ModelDownloadRuntime>>,
    target: ModelTarget,
    model: &str,
    phase: ModelDownloadPhase,
    downloaded_bytes: u64,
    total_bytes: u64,
    speed_bytes_per_sec: u64,
    message: &str,
    clear_cancel_flag: bool,
) -> Result<(), String> {
    let snapshot = ModelDownloadStateSnapshot {
        phase,
        downloaded_bytes,
        total_bytes,
        speed_bytes_per_sec,
        message: message.to_string(),
    };
    {
        let mut guard = runtime
            .lock()
            .map_err(|_| "model download state lock poisoned".to_string())?;
        guard.snapshot = snapshot.clone();
        guard.active_model = if clear_cancel_flag {
            None
        } else {
            Some(model.to_string())
        };
        if clear_cancel_flag {
            guard.cancel_flag = None;
        }
    }
    emit_model_download_progress(app, target, model, &snapshot);
    Ok(())
}

fn emit_model_download_progress(
    app: &tauri::AppHandle,
    target: ModelTarget,
    model: &str,
    snapshot: &ModelDownloadStateSnapshot,
) {
    let _ = app.emit(
        "model-download-progress",
        ModelDownloadProgressEvent {
            target,
            model: model.to_string(),
            phase: snapshot.phase,
            downloaded_bytes: snapshot.downloaded_bytes,
            total_bytes: snapshot.total_bytes,
            speed_bytes_per_sec: snapshot.speed_bytes_per_sec,
            message: snapshot.message.clone(),
        },
    );
}
