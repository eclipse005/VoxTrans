use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskRuntimeStatus {
    Queued,
    Running,
    Failed,
    Completed,
}

impl TaskRuntimeStatus {
    pub fn as_db_status(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Failed => "failed",
            Self::Completed => "completed",
        }
    }
}

pub fn runtime_status_from_db(status: &str) -> TaskRuntimeStatus {
    match status.trim().to_ascii_lowercase().as_str() {
        "running" => TaskRuntimeStatus::Running,
        "failed" => TaskRuntimeStatus::Failed,
        "completed" => TaskRuntimeStatus::Completed,
        _ => TaskRuntimeStatus::Queued,
    }
}

pub fn workspace_status_from_db(status: &str) -> &'static str {
    match status.trim().to_ascii_lowercase().as_str() {
        "queued" => "queued",
        "running" => "processing",
        "completed" => "done",
        "failed" => "error",
        _ => "queued",
    }
}
