use serde::Serialize;

use crate::services::transcribe::WordTokenDto;

#[derive(Debug, Clone)]
pub struct StageResult<T> {
    pub executed: bool,
    pub metrics: T,
    pub warnings: Vec<String>,
}

impl<T: Default> StageResult<T> {
    pub fn skipped() -> Self {
        Self {
            executed: false,
            metrics: T::default(),
            warnings: Vec::new(),
        }
    }

    pub fn skipped_with(metrics: T) -> Self {
        Self {
            executed: false,
            metrics,
            warnings: Vec::new(),
        }
    }
}

impl<T> StageResult<T> {
    pub fn executed(metrics: T) -> Self {
        Self {
            executed: true,
            metrics,
            warnings: Vec::new(),
        }
    }
}

#[derive(Debug, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PunctuationStats {
    pub sentence_total: usize,
    pub suspicious_count: usize,
    pub restored_count: usize,
    pub accepted_count: usize,
    pub rejected_count: usize,
}

#[derive(Debug, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct HotwordStats {
    pub changed_count: usize,
    pub summary: String,
    pub replacement_stats: Vec<ReplacementStat>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ReplacementStat {
    pub old_text: String,
    pub new_text: String,
    pub count: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct TemporarySentence {
    pub start_word: usize,
    pub end_word_exclusive: usize,
    pub text: String,
}

#[derive(Debug, Clone)]
pub(crate) struct TimedHotwordSegment {
    pub start_ms: i64,
    pub end_ms: i64,
    pub source_text: String,
    pub words: Vec<WordTokenDto>,
}
