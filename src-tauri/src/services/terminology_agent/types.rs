use serde::{Deserialize, Serialize};

use crate::commands::translate_types::TranslateTerminologyEntryCommand;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GlossaryEntry {
    pub source: String,
    pub target: String,
    #[serde(default)]
    pub note: String,
}

impl GlossaryEntry {
    pub fn new(source: impl Into<String>, target: impl Into<String>, note: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            target: target.into(),
            note: note.into(),
        }
    }

    pub fn into_translate_entry(self) -> TranslateTerminologyEntryCommand {
        TranslateTerminologyEntryCommand {
            source: self.source,
            target: self.target,
            note: self.note,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptCue {
    pub index: usize,
    pub start_ms: u64,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EndReason {
    SubmitOk,
    MaxRounds,
    LlmError,
    NoToolCalls,
    Skipped,
}

impl EndReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SubmitOk => "submit_ok",
            Self::MaxRounds => "max_rounds",
            Self::LlmError => "llm_error",
            Self::NoToolCalls => "no_tool_calls",
            Self::Skipped => "skipped",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminologyBriefing {
    pub glossary: Vec<GlossaryEntry>,
    pub style_guide: String,
    pub windows: usize,
    pub end_reason: EndReason,
    pub skipped: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skip_reason: Option<String>,
}

impl TerminologyBriefing {
    pub fn skipped(reason: impl Into<String>) -> Self {
        Self {
            glossary: Vec::new(),
            style_guide: String::new(),
            windows: 0,
            end_reason: EndReason::Skipped,
            skipped: true,
            skip_reason: Some(reason.into()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolKind {
    Ok,
    Error,
    SubmitOk,
    Blocked,
}

#[derive(Debug, Clone)]
pub struct ToolOutcome {
    pub ok: bool,
    pub terminate: bool,
    pub kind: ToolKind,
    pub repairable: bool,
    pub message: String,
}

impl ToolOutcome {
    pub fn ok_msg(message: impl Into<String>) -> Self {
        Self {
            ok: true,
            terminate: false,
            kind: ToolKind::Ok,
            repairable: false,
            message: message.into(),
        }
    }

    pub fn err(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            terminate: false,
            kind: ToolKind::Error,
            repairable: true,
            message: message.into(),
        }
    }

    pub fn submit_ok(message: impl Into<String>) -> Self {
        Self {
            ok: true,
            terminate: true,
            kind: ToolKind::SubmitOk,
            repairable: false,
            message: message.into(),
        }
    }

    pub fn blocked(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            terminate: false,
            kind: ToolKind::Blocked,
            repairable: false,
            message: message.into(),
        }
    }
}

pub fn normalize_term_key(source: &str) -> String {
    source
        .trim()
        .to_lowercase()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect()
}
