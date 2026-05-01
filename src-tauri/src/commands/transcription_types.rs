#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct WordTokenCommandDto {
    pub start: f64,
    pub end: f64,
    pub word: String,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SourceSentenceCommandDto {
    pub sentence_id: usize,
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
    pub word_start: usize,
    pub word_end: usize,
    pub chunk_start: usize,
    pub chunk_end: usize,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MicroChunkCommandDto {
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

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BoundaryDecisionCommandDto {
    pub left_chunk_id: usize,
    pub right_chunk_id: usize,
    pub gap_ms: u64,
    pub rule_decision: crate::services::transcription::BoundaryDecisionKind,
    pub llm_decision: crate::services::transcription::BoundaryDecisionKind,
    pub final_decision: crate::services::transcription::BoundaryDecisionKind,
    pub confidence: f64,
    pub reason_tag: String,
}

#[derive(Debug, serde::Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BuildSourceSentencesCommandRequest {
    pub task_id: String,
    pub audio_path: String,
    pub source_lang: String,
    pub subtitle_length_preset: String,
    #[serde(default = "default_use_subtitle_layout_split")]
    pub use_subtitle_layout_split: bool,
    pub words: Vec<WordTokenCommandDto>,
}

fn default_use_subtitle_layout_split() -> bool {
    true
}

#[derive(Debug, serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BuildSourceSentencesCommandResponse {
    pub hard_split_gap_ms: u64,
    pub micro_chunk_total: usize,
    pub boundary_total: usize,
    pub sentence_total: usize,
    pub micro_chunks: Vec<MicroChunkCommandDto>,
    pub boundaries: Vec<BoundaryDecisionCommandDto>,
    pub translation_sentences: Vec<SourceSentenceCommandDto>,
    pub segments: Vec<GroupedSentenceSegmentCommandDto>,
    pub srt: String,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GroupedSentenceTokenCommandDto {
    pub text: String,
    pub start: f64,
    pub end: f64,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GroupedSentenceSegmentCommandDto {
    pub segment: String,
    pub start: f64,
    pub end: f64,
    pub tokens: Vec<GroupedSentenceTokenCommandDto>,
}
