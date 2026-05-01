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
}

#[derive(Debug, Clone)]
pub(super) struct Step51LlmSplitTask {
    pub(super) task_id: usize,
    pub(super) work_index: usize,
    pub(super) source_lang: String,
    pub(super) tokens: Vec<Step5Token>,
    pub(super) range: (usize, usize),
    pub(super) source_text: String,
    pub(super) require_split: bool,
    pub(super) prompt: String,
}
