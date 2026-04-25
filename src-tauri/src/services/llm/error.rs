use std::fmt::{Display, Formatter};

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

#[derive(Debug, Clone)]
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

impl Display for LlmError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for LlmError {}
