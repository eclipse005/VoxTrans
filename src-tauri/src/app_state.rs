use sqlx::SqlitePool;
use std::process::Child;
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

pub struct ModelDownloadRuntime {
    pub cancel_flag: Option<Arc<AtomicBool>>,
    pub active_model: Option<String>,
    pub snapshot: ModelDownloadStateSnapshot,
}

impl Default for ModelDownloadRuntime {
    fn default() -> Self {
        Self {
            cancel_flag: None,
            active_model: None,
            snapshot: ModelDownloadStateSnapshot::default(),
        }
    }
}

pub struct TaskWorkerRuntime {
    pub running_task_id: Option<String>,
    pub child: Option<Child>,
    pub stderr_tail: Option<Arc<Mutex<String>>>,
}

impl Default for TaskWorkerRuntime {
    fn default() -> Self {
        Self {
            running_task_id: None,
            child: None,
            stderr_tail: None,
        }
    }
}

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub asr_model_download: Arc<Mutex<ModelDownloadRuntime>>,
    pub demucs_model_download: Arc<Mutex<ModelDownloadRuntime>>,
    pub task_worker_runtime: Arc<Mutex<TaskWorkerRuntime>>,
}
