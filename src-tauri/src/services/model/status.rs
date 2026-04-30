use super::{ModelTarget, model_definition, runtime_for_target};
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
    let definition = model_definition(target, model.as_deref())?;
    let runtime = runtime_for_target(state, target);

    let (snapshot_in_memory, active_model) = {
        let guard = runtime
            .lock()
            .map_err(|_| "model download state lock poisoned".to_string())?;
        (guard.snapshot.clone(), guard.active_model.clone())
    };

    let missing_files = definition
        .required_files
        .iter()
        .filter(|name| !definition.model_dir.join(name).exists())
        .cloned()
        .collect::<Vec<_>>();
    let ready = missing_files.is_empty();
    let file_size = definition
        .required_files
        .iter()
        .map(|name| {
            std::fs::metadata(definition.model_dir.join(name))
                .map(|m| m.len())
                .unwrap_or(0)
        })
        .sum::<u64>();
    let expected_size = definition
        .download_files
        .iter()
        .map(|file| file.expected_size)
        .sum::<u64>();
    let downloading_this_model = snapshot_in_memory.phase == ModelDownloadPhase::Downloading
        && active_model.as_deref() == Some(definition.model.as_str());
    let downloaded_bytes = if ready {
        file_size.max(expected_size)
    } else if downloading_this_model {
        snapshot_in_memory.downloaded_bytes
    } else {
        file_size
    };
    let total_bytes = if ready {
        file_size.max(expected_size)
    } else if downloading_this_model {
        snapshot_in_memory
            .total_bytes
            .max(expected_size)
            .max(downloaded_bytes)
    } else {
        expected_size.max(downloaded_bytes)
    };

    let phase =
        if snapshot_in_memory.phase == ModelDownloadPhase::Downloading && downloading_this_model {
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
        speed_bytes_per_sec: if phase == ModelDownloadPhase::Downloading {
            snapshot_in_memory.speed_bytes_per_sec
        } else {
            0
        },
        message: snapshot_in_memory.message,
    };

    Ok(ModelStatusResponse {
        target,
        model: definition.model,
        model_dir: definition.model_dir.display().to_string(),
        required_files: definition.required_files,
        missing_files,
        ready,
        download: snapshot,
    })
}
