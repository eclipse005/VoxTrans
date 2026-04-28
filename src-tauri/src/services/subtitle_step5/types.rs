#[derive(Debug, Clone)]
pub struct Step5Token {
    pub text: String,
    pub start: f64,
    pub end: f64,
}

#[derive(Debug, Clone)]
pub struct Step5DraftSegment {
    pub segment_id: usize,
    pub start: f64,
    pub end: f64,
    pub source: String,
    pub draft_translation: String,
    pub tokens: Vec<Step5Token>,
}

#[derive(Debug, Clone)]
pub struct Step5TerminologyEntry {
    pub source: String,
    pub target: String,
    pub note: String,
}

#[derive(Debug, Clone)]
pub struct Step5SplitPart {
    pub part_id: usize,
    pub start: f64,
    pub end: f64,
    pub source: String,
    pub tokens: Vec<Step5Token>,
}

#[derive(Debug, Clone)]
pub struct Step5SplitParent {
    pub parent_segment_id: usize,
    pub draft_translation: String,
    pub parts: Vec<Step5SplitPart>,
}

#[derive(Debug, Clone)]
pub struct BuildStep5SourceSplitRequest {
    pub task_id: String,
    pub media_path: String,
    pub source_lang: String,
    pub target_lang: String,
    pub segments: Vec<Step5DraftSegment>,
    pub subtitle_max_words_per_segment: u32,
    pub subtitle_length_reference: u32,
    pub translate_api_key: String,
    pub translate_base_url: String,
    pub translate_model: String,
    pub llm_concurrency: u32,
}

#[derive(Debug, Clone)]
pub struct BuildStep5SourceSplitResponse {
    pub subtitle_max_words_per_segment: u32,
    pub subtitle_length_reference: u32,
    pub parent_total: usize,
    pub part_total: usize,
    pub parents: Vec<Step5SplitParent>,
}

#[derive(Debug, Clone)]
pub struct Step5AlignedPart {
    pub part_id: usize,
    pub start: f64,
    pub end: f64,
    pub source: String,
    pub translation: String,
    pub tokens: Vec<Step5Token>,
}

#[derive(Debug, Clone)]
pub struct Step5AlignedParent {
    pub parent_segment_id: usize,
    pub parts: Vec<Step5AlignedPart>,
}

#[derive(Debug, Clone)]
pub struct BuildStep5TranslationAlignRequest {
    pub task_id: String,
    pub media_path: String,
    pub source_lang: String,
    pub target_lang: String,
    pub theme_summary: String,
    pub terminology_entries: Vec<Step5TerminologyEntry>,
    pub parents: Vec<Step5SplitParent>,
    pub translate_api_key: String,
    pub translate_base_url: String,
    pub translate_model: String,
    pub llm_concurrency: u32,
}

#[derive(Debug, Clone)]
pub struct BuildStep5TranslationAlignResponse {
    pub parent_total: usize,
    pub part_total: usize,
    pub parents: Vec<Step5AlignedParent>,
}

#[derive(Debug, Clone)]
pub struct Step5FinalSegment {
    pub segment_id: usize,
    pub start: f64,
    pub end: f64,
    pub source: String,
    pub translation: String,
    pub tokens: Vec<Step5Token>,
}

#[derive(Debug, Clone)]
pub struct BuildStep5TranslationPolishRequest {
    pub task_id: String,
    pub media_path: String,
    pub source_lang: String,
    pub target_lang: String,
    pub terminology_entries: Vec<Step5TerminologyEntry>,
    pub parents: Vec<Step5AlignedParent>,
    pub translate_api_key: String,
    pub translate_base_url: String,
    pub translate_model: String,
    pub llm_concurrency: u32,
    pub subtitle_length_reference: u32,
    pub batch_size: usize,
}

#[derive(Debug, Clone)]
pub struct BuildStep5TranslationPolishResponse {
    pub batch_size: usize,
    pub batch_total: usize,
    pub segment_total: usize,
    pub segments: Vec<Step5FinalSegment>,
}

#[derive(Debug, Clone)]
pub struct Step5QualityIssue {
    pub rule_id: String,
    pub severity: String,
    pub segment_id: usize,
    pub part_id: usize,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct Step6FinalCheckMetrics {
    pub segment_total: usize,
    pub empty_count: usize,
    pub ellipsis_tail_count: usize,
    pub numeric_drift_count: usize,
    pub cross_line_leak_count: usize,
    pub gt25_count: usize,
    pub gt32_count: usize,
}

#[derive(Debug, Clone)]
pub struct BuildStep6FinalCheckRequest {
    pub target_lang: String,
    pub segments: Vec<Step5FinalSegment>,
}

#[derive(Debug, Clone)]
pub struct BuildStep6FinalCheckResponse {
    pub passed: bool,
    pub hard_fail_count: usize,
    pub soft_score: f64,
    pub issue_count: usize,
    pub issues: Vec<Step5QualityIssue>,
    pub metrics: Step6FinalCheckMetrics,
}
