#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Step5Token {
    pub text: String,
    pub start: f64,
    pub end: f64,
}

/// Internal representation used by `watchability_merge` for merging
/// adjacent subtitle lines that violate length/CPS budgets. Despite the
/// "Step5" name (kept for historical reasons — the full Step5
/// split/align pipeline was removed; see translation_flow.rs), this
/// type is still the working struct for the merge pass.
#[derive(Debug, Clone)]
pub struct Step5FinalSegment {
    pub segment_id: usize,
    pub start: f64,
    pub end: f64,
    pub source: String,
    pub translation: String,
    pub tokens: Vec<Step5Token>,
}
