use std::io;

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

impl From<String> for WorkspaceError {
    fn from(message: String) -> Self {
        WorkspaceError::TaskFailed(message)
    }
}

impl From<WorkspaceError> for String {
    fn from(err: WorkspaceError) -> Self {
        err.to_string()
    }
}

pub type WorkspaceResult<T> = Result<T, WorkspaceError>;
