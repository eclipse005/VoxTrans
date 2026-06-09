#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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


#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Step5AlignedPart {
    pub part_id: usize,
    pub start: f64,
    pub end: f64,
    pub source: String,
    pub translation: String,
    pub tokens: Vec<Step5Token>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Step5AlignedParent {
    pub parent_segment_id: usize,
    pub parts: Vec<Step5AlignedPart>,
    #[serde(default)]
    pub rounds_used: usize,
}


#[derive(Debug, Clone)]
pub struct BuildStep5SplitAlignRequest {
    pub task_id: String,
    pub media_path: String,
    pub source_lang: String,
    pub target_lang: String,
    pub theme_summary: String,
    pub terminology_entries: Vec<Step5TerminologyEntry>,
    pub segments: Vec<Step5DraftSegment>,
    pub subtitle_length_preset: String,
    pub translate_api_key: String,
    pub translate_base_url: String,
    pub translate_model: String,
    pub llm_concurrency: u32,
    pub unit_store: Option<crate::services::pipeline::UnitStore>,
}

#[derive(Debug, Clone)]
pub struct BuildStep5SplitAlignResponse {
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
