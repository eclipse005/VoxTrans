//! Engine-agnostic forced-alignment spans (word/char level).
//!
//! Downstream pipeline, DB cache, and punctuation mapping use this type only —
//! never `qwen_forced_aligner_rs::ForcedAlignItem` / CTC items directly.

use serde::{Deserialize, Serialize};

/// One aligned token with times in **seconds** relative to the segment start
/// (callers add segment offset when building global timeline).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AlignedSpan {
    pub text: String,
    /// Accepts legacy Qwen cache JSON (`start_time`) and new field name.
    #[serde(alias = "start_time")]
    pub start: f64,
    #[serde(alias = "end_time")]
    pub end: f64,
}

impl AlignedSpan {
    pub fn new(text: impl Into<String>, start: f64, end: f64) -> Self {
        Self {
            text: text.into(),
            start,
            end,
        }
    }
}
