use sqlx::SqlitePool;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, RwLock};

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelDownloadStateSnapshot {
    pub phase: String,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub speed_bytes_per_sec: u64,
    pub message: String,
}

impl Default for ModelDownloadStateSnapshot {
    fn default() -> Self {
        Self {
            phase: "idle".to_string(),
            downloaded_bytes: 0,
            total_bytes: 0,
            speed_bytes_per_sec: 0,
            message: String::new(),
        }
    }
}

pub struct ModelDownloadRuntime {
    pub cancel_flag: Option<Arc<AtomicBool>>,
    pub snapshot: ModelDownloadStateSnapshot,
}

impl Default for ModelDownloadRuntime {
    fn default() -> Self {
        Self {
            cancel_flag: None,
            snapshot: ModelDownloadStateSnapshot::default(),
        }
    }
}

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub model_download: Arc<Mutex<ModelDownloadRuntime>>,
    pub llm_settings: Arc<RwLock<crate::services::preferences::LlmSettings>>,
}
