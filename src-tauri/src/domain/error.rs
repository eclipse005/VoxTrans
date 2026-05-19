use std::io;

use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum WorkspaceError {
    #[error("task not found: {0}")]
    TaskNotFound(String),

    #[error("task is processing or queued")]
    TaskBusy,

    #[error("workspace store lock poisoned")]
    LockPoisoned,

    #[error("invalid request: {0}")]
    InvalidRequest(String),

    #[error("task failed: {0}")]
    TaskFailed(String),

    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    #[error("serialization error: {0}")]
    Serialization(String),
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CommandErrorPayload {
    code: &'static str,
    message: String,
}

impl WorkspaceError {
    pub fn code(&self) -> &'static str {
        match self {
            WorkspaceError::TaskNotFound(_) => "TASK_NOT_FOUND",
            WorkspaceError::TaskBusy => "TASK_BUSY",
            WorkspaceError::LockPoisoned => "WORKSPACE_LOCK_POISONED",
            WorkspaceError::InvalidRequest(_) => "INVALID_REQUEST",
            WorkspaceError::TaskFailed(_) => "TASK_FAILED",
            WorkspaceError::Io(_) => "IO_ERROR",
            WorkspaceError::Serialization(_) => "SERIALIZATION_ERROR",
        }
    }

    pub fn to_command_error(&self) -> String {
        serde_json::to_string(&CommandErrorPayload {
            code: self.code(),
            message: self.to_string(),
        })
        .unwrap_or_else(|_| self.to_string())
    }
}

impl From<String> for WorkspaceError {
    fn from(message: String) -> Self {
        WorkspaceError::TaskFailed(message)
    }
}

impl From<WorkspaceError> for String {
    fn from(err: WorkspaceError) -> Self {
        err.to_command_error()
    }
}

pub type WorkspaceResult<T> = Result<T, WorkspaceError>;

#[cfg(test)]
mod tests {
    use super::WorkspaceError;

    #[test]
    fn workspace_error_serializes_stable_code_and_message() {
        let encoded = WorkspaceError::TaskNotFound("task-1".to_string()).to_command_error();
        let value: serde_json::Value = serde_json::from_str(&encoded).expect("json error");

        assert_eq!(value["code"], "TASK_NOT_FOUND");
        assert_eq!(value["message"], "task not found: task-1");
    }

    #[test]
    fn invalid_request_serializes_stable_code_and_message() {
        let encoded =
            WorkspaceError::InvalidRequest("taskId is required".to_string()).to_command_error();
        let value: serde_json::Value = serde_json::from_str(&encoded).expect("json error");

        assert_eq!(value["code"], "INVALID_REQUEST");
        assert_eq!(value["message"], "invalid request: taskId is required");
    }
}
