use super::types::{Step5DraftSegment, Step5Token};

#[derive(Debug, Clone)]
pub(super) struct Step5SplitTask {
    pub(super) task_id: usize,
    pub(super) parent_segment_id: usize,
    pub(super) part_sources: Vec<String>,
    pub(super) prompt: String,
}

#[derive(Debug, Clone)]
pub(super) struct Step51SplitWorkItem {
    pub(super) segment: Step5DraftSegment,
    pub(super) draft_translation: String,
    pub(super) mandatory_boundaries: Vec<usize>,
    pub(super) fallback_boundaries: Vec<usize>,
    pub(super) over_length: bool,
    pub(super) min_parts: usize,
}

#[derive(Debug, Clone)]
pub(super) struct Step51LlmSplitTask {
    pub(super) task_id: usize,
    pub(super) work_index: usize,
    pub(super) source_lang: String,
    pub(super) tokens: Vec<Step5Token>,
    pub(super) mandatory_boundaries: Vec<usize>,
    pub(super) fallback_boundaries: Vec<usize>,
    pub(super) min_parts: usize,
    pub(super) prompt: String,
}
