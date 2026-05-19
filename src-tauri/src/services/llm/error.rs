use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LlmErrorKind {
    Http,
    InvalidJson,
    InvalidSchema,
    InvalidSemantic,
    Config,
}

impl LlmErrorKind {
    pub fn as_str(self) -> &'static str {
        match self {
            LlmErrorKind::Http => "http",
            LlmErrorKind::InvalidJson => "invalid_json",
            LlmErrorKind::InvalidSchema => "invalid_schema",
            LlmErrorKind::InvalidSemantic => "invalid_semantic",
            LlmErrorKind::Config => "config",
        }
    }
}

#[derive(Debug, Clone, Error)]
#[error("{message}")]
pub struct LlmError {
    pub kind: LlmErrorKind,
    pub message: String,
}

impl LlmError {
    pub fn new(kind: LlmErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

impl From<LlmError> for String {
    fn from(err: LlmError) -> Self {
        err.to_string()
    }
}
