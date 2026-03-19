use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TranslateTerminologyEntry {
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub target: String,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub group: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslateToken {
    pub start: f64,
    pub end: f64,
    pub word: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslateSegment {
    pub start_ms: u64,
    pub end_ms: u64,
    pub source_text: String,
    pub translated_text: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslatePipelineRequest {
    pub task_id: String,
    pub media_path: String,
    pub source_lang: String,
    pub target_lang: String,
    pub tokens: Vec<TranslateToken>,
    #[serde(default)]
    pub translate_api_key: String,
    #[serde(default)]
    pub translate_base_url: String,
    #[serde(default)]
    pub translate_model: String,
    #[serde(default = "default_llm_concurrency")]
    pub llm_concurrency: u32,
    #[serde(default)]
    pub terminology_entries: Vec<TranslateTerminologyEntry>,
}

fn default_llm_concurrency() -> u32 {
    4
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslatePipelineResponse {
    pub source_srt: String,
    pub target_srt: String,
    pub bilingual_srt_source_first: String,
    pub bilingual_srt_target_first: String,
    pub segments: Vec<TranslateSegment>,
    #[serde(default)]
    pub style_topic_summary: String,
    #[serde(default)]
    pub style_tone_strategy: String,
}
