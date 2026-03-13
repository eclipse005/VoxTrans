use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StageResult<T> {
    pub executed: bool,
    pub metrics: T,
    pub warnings: Vec<String>,
}

impl<T> StageResult<T> {
    pub fn skipped_with(metrics: T) -> Self {
        Self {
            executed: false,
            metrics,
            warnings: Vec::new(),
        }
    }
}

impl<T> StageResult<T> {
    pub fn executed(metrics: T) -> Self {
        Self {
            executed: true,
            metrics,
            warnings: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationPipelineRequest {
    pub task_id: String,
    pub media_path: String,
    pub source_language: String,
    pub target_language: String,
    pub style: Option<String>,
    pub threads: Option<u32>,
    pub terminology_group_ids: Vec<String>,
    pub cues: Vec<SourceCue>,
    pub words: Vec<WordToken>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationPipelineResponse {
    pub task_id: String,
    pub summary: TranslationProfile,
    pub stages: Vec<StageReport>,
    pub cues: Vec<AlignedCue>,
    pub qa: QaSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceCue {
    pub cue_id: String,
    pub start_ms: u64,
    pub end_ms: u64,
    pub source_text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WordToken {
    pub start: f64,
    pub end: f64,
    pub word: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SentenceUnit {
    pub sentence_id: String,
    pub source_text: String,
    pub cue_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslatedUnit {
    pub sentence_id: String,
    pub translated_text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlignedCue {
    pub cue_id: String,
    pub source_text: String,
    pub translated_text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationProfile {
    pub topic_summary: String,
    pub content_style: String,
    pub translation_style: String,
    pub terminology_subset: Vec<TranslationTerm>,
    #[serde(default)]
    pub primary_terms: Vec<TranslationTerm>,
    #[serde(default)]
    pub supporting_terms: Vec<TranslationTerm>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationTerm {
    pub source: String,
    pub target: String,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StageReport {
    pub stage: TranslationStage,
    pub status: StageStatus,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QaIssue {
    pub code: String,
    pub severity: String,
    pub cue_id: Option<String>,
    pub message: String,
    pub fixable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QaSummary {
    pub issue_total: usize,
    pub fixed_total: usize,
    pub unresolved_total: usize,
    pub issues: Vec<QaIssue>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QaStageMetrics {
    pub cues: Vec<AlignedCue>,
    pub qa: QaSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TranslationStage {
    Summary,
    Translate,
    Align,
    Qa,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StageStatus {
    Skipped,
    Completed,
    Failed,
}
