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

/// Why the windowed extraction loop ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EndReason {
    /// Every cue was processed.
    Completed,
    /// Stopped early: consecutive windows produced nothing new — the domain
    /// vocabulary has saturated.
    EarlyExit,
    /// Aborted: the endpoint kept failing even at the minimum window width.
    LlmError,
    Skipped,
}

impl EndReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::EarlyExit => "early_exit",
            Self::LlmError => "llm_error",
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

/// One ASR-mishear verdict from a window: `wrong` is the transcript spelling,
/// `right` the corrected one (or the same term when adjudicated as intended).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MisheardPair {
    pub wrong: String,
    pub right: String,
    #[serde(default)]
    pub note: String,
}

pub fn normalize_term_key(source: &str) -> String {
    source
        .trim()
        .to_lowercase()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect()
}
