//! Model-identity vocabulary shared by the language registry, preferences and
//! the transcription engines. These enumerate which ASR / forced-aligner models
//! the app can run and are keyed by [`crate::domain::language_registry`] to
//! decide per-language support — a domain fact, not a transport concern.

use std::fmt;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default, TS)]
pub enum AsrModel {
    #[default]
    #[serde(rename = "Qwen3-ASR-0.6B")]
    Qwen3Asr06B,
    #[serde(rename = "Qwen3-ASR-1.7B")]
    Qwen3Asr17B,
    #[serde(rename = "cohere-transcribe-03-2026")]
    CohereTranscribe032026,
    #[serde(rename = "moss-transcribe-diarize")]
    MossTranscribeDiarize,
}

impl fmt::Display for AsrModel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl AsrModel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Qwen3Asr06B => "Qwen3-ASR-0.6B",
            Self::Qwen3Asr17B => "Qwen3-ASR-1.7B",
            Self::CohereTranscribe032026 => "cohere-transcribe-03-2026",
            Self::MossTranscribeDiarize => "moss-transcribe-diarize",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value.trim() {
            "Qwen3-ASR-1.7B" => Self::Qwen3Asr17B,
            "cohere-transcribe-03-2026" => Self::CohereTranscribe032026,
            "moss-transcribe-diarize" => Self::MossTranscribeDiarize,
            _ => Self::Qwen3Asr06B,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default, TS)]
pub enum AlignModel {
    #[serde(rename = "Qwen3-ForcedAligner-0.6B")]
    Qwen3ForcedAligner06B,
    /// MMS CTC forced aligner (romanize path; language fixed eng at engine).
    /// Default: lighter weights, good accuracy for word-level timestamps.
    #[default]
    #[serde(rename = "mms-300m-1130-forced-aligner")]
    MmsCtcForcedAligner300m,
}

impl fmt::Display for AlignModel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl AlignModel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Qwen3ForcedAligner06B => "Qwen3-ForcedAligner-0.6B",
            Self::MmsCtcForcedAligner300m => "mms-300m-1130-forced-aligner",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value.trim() {
            "Qwen3-ForcedAligner-0.6B" => Self::Qwen3ForcedAligner06B,
            "mms-300m-1130-forced-aligner" => Self::MmsCtcForcedAligner300m,
            _ => Self::MmsCtcForcedAligner300m,
        }
    }
}
