use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::services::transcribe::WordTokenDto;

#[derive(Debug, Clone)]
pub struct SentenceBoundaryRequest {
    pub task_id: String,
    pub media_path: String,
    pub source_lang: String,
    pub words: Vec<WordTokenDto>,
    pub subtitle_max_words_per_segment: u32,
    pub translate_api_key: String,
    pub translate_base_url: String,
    pub translate_model: String,
    pub llm_concurrency: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceSentenceStep2 {
    pub task_id: String,
    pub media_path: String,
    pub source_lang: String,
    pub hard_split_gap_ms: u64,
    pub micro_chunk_total: usize,
    pub boundary_total: usize,
    pub sentence_total: usize,
    pub micro_chunks: Vec<MicroChunk>,
    pub boundaries: Vec<BoundaryDecision>,
    pub translation_sentences: Vec<SourceSentence>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MicroChunk {
    pub chunk_id: usize,
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
    pub word_start: usize,
    pub word_end: usize,
    pub gap_before_ms: u64,
    pub gap_after_ms: u64,
    pub hard_split_before: bool,
    pub hard_split_after: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BoundaryDecision {
    pub left_chunk_id: usize,
    pub right_chunk_id: usize,
    pub gap_ms: u64,
    pub rule_decision: BoundaryDecisionKind,
    pub llm_decision: BoundaryDecisionKind,
    pub final_decision: BoundaryDecisionKind,
    pub confidence: f64,
    pub reason_tag: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceSentence {
    pub sentence_id: usize,
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
    pub word_start: usize,
    pub word_end: usize,
    pub chunk_start: usize,
    pub chunk_end: usize,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BoundaryDecisionKind {
    HardSplit,
    Split,
    Merge,
    Unsure,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SplitReason {
    TerminalPunctuation,
    HardPause,
    LengthFallback,
    LlmSemanticRefinement,
}

#[derive(Debug, Clone)]
pub(super) struct SemanticBoundaryCandidate {
    pub(super) id: usize,
    pub(super) split_after: usize,
    pub(super) reason: String,
    pub(super) score: f64,
}

#[derive(Debug, Clone)]
pub(super) struct SemanticRefinementTask {
    pub(super) task_id: usize,
    pub(super) span_index: usize,
    pub(super) span_start: usize,
    pub(super) span_end: usize,
    pub(super) desired_parts: usize,
    pub(super) fallback_splits: Vec<usize>,
    pub(super) candidates: Vec<SemanticBoundaryCandidate>,
    pub(super) prompt: String,
}
