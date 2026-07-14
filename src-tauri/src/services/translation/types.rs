use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct TranslationToken {
    pub text: String,
    pub start: f64,
    pub end: f64,
}

#[derive(Debug, Clone)]
pub struct TranslationSegmentInput {
    pub segment: String,
    pub start: f64,
    pub end: f64,
    pub tokens: Vec<TranslationToken>,
}

#[derive(Debug, Clone)]
pub struct TranslationTerminologyEntry {
    pub source: String,
    pub target: String,
    pub note: String,
}

#[derive(Debug, Clone)]
pub struct BuildTranslationLayerRequest {
    pub task_id: String,
    pub media_path: String,
    pub source_lang: String,
    pub target_lang: String,
    pub segments: Vec<TranslationSegmentInput>,
    pub theme_summary: String,
    pub terminology_entries: Vec<TranslationTerminologyEntry>,
    pub translate_api_key: String,
    pub translate_base_url: String,
    pub translate_model: String,
    pub llm_concurrency: u32,
    pub batch_size: usize,
    pub unit_store: Option<crate::services::pipeline::UnitStore>,
    /// When true, sample video frames per batch and send as vision evidence.
    /// Live setting resolved from saved settings (not frozen at enqueue).
    pub enable_vision_assist: bool,
}

#[derive(Debug, Clone)]
pub struct TranslationSegmentOutput {
    pub segment_id: usize,
    pub start: f64,
    pub end: f64,
    pub source: String,
    pub translation: String,
    pub tokens: Vec<TranslationToken>,
}

#[derive(Debug, Clone)]
pub struct BuildTranslationLayerResponse {
    pub batch_size: usize,
    pub batch_total: usize,
    pub segment_total: usize,
    pub segments: Vec<TranslationSegmentOutput>,
}

/// Structured progress for translation.
///
/// Emitted mid-batch (token stream, throttled) and when a batch completes.
/// `partial_outputs` is a full snapshot of all segments rebuilt from the
/// cumulative translations so far: finished lines are complete; the line
/// currently streaming may grow character-by-character; the rest keep an
/// empty translation until their batch runs.
#[derive(Debug, Clone)]
pub struct TranslationProgress {
    pub done: usize,
    pub total: usize,
    pub partial_outputs: Vec<TranslationSegmentOutput>,
}

#[derive(Debug, Clone)]
pub(super) struct NormalizedSegment {
    pub(super) segment_id: usize,
    pub(super) start: f64,
    pub(super) end: f64,
    pub(super) source: String,
    pub(super) tokens: Vec<TranslationToken>,
}

#[derive(Debug, Clone)]
pub(super) struct BatchWindow {
    pub(super) batch_id: usize,
    pub(super) local_ids: Vec<usize>,
    pub(super) local_to_global: Vec<usize>,
    pub(super) prompt: String,
    /// base64 data URLs of sampled video frames for this batch's time range.
    /// Empty when vision assist is disabled or no frames could be extracted.
    pub(super) frames: Arc<[String]>,
    /// Cache filenames of the frames in `frames` (same order), used for
    /// logging which frames each batch's translation used. Empty iff `frames`
    /// is empty.
    pub(super) frame_names: Arc<[String]>,
}
