use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelDownloadPhase {
    Idle,
    Downloading,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelDownloadStateSnapshot {
    pub phase: ModelDownloadPhase,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub speed_bytes_per_sec: u64,
    pub message: String,
}

impl Default for ModelDownloadStateSnapshot {
    fn default() -> Self {
        Self {
            phase: ModelDownloadPhase::Idle,
            downloaded_bytes: 0,
            total_bytes: 0,
            speed_bytes_per_sec: 0,
            message: String::new(),
        }
    }
}

#[derive(Default)]
pub struct ModelDownloadRuntime {
    pub cancel_flag: Option<Arc<AtomicBool>>,
    pub active_model: Option<String>,
    pub snapshot: ModelDownloadStateSnapshot,
}

#[derive(Clone)]
pub struct AppState {
    pub asr_model_download: Arc<Mutex<ModelDownloadRuntime>>,
    pub align_model_download: Arc<Mutex<ModelDownloadRuntime>>,
    pub demucs_model_download: Arc<Mutex<ModelDownloadRuntime>>,
}
