use serde::{Deserialize, Serialize};


#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TranslateTerminologyEntryCommand {
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub target: String,
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SegmentTokenForTerminologyCommand {
    #[serde(default, alias = "word")]
    pub text: String,
    pub start: f64,
    pub end: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SourceSegmentForTerminologyCommand {
    #[serde(default, alias = "text")]
    pub segment: String,
    pub start: f64,
    pub end: f64,
    #[serde(default)]
    pub tokens: Vec<SegmentTokenForTerminologyCommand>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildTerminologyLayerCommandRequest {
    pub task_id: String,
    pub media_path: String,
    pub source_lang: String,
    pub target_lang: String,
    pub segments: Vec<SourceSegmentForTerminologyCommand>,
    #[serde(default)]
    pub translate_api_key: String,
    #[serde(default)]
    pub translate_base_url: String,
    #[serde(default)]
    pub translate_model: String,
    #[serde(default = "default_llm_concurrency")]
    pub llm_concurrency: u32,
    #[serde(default)]
    pub terminology_entries: Vec<TranslateTerminologyEntryCommand>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildTerminologyLayerCommandResponse {
    pub task_id: String,
    pub media_path: String,
    pub source_lang: String,
    pub target_lang: String,
    pub source_segment_total: usize,
    pub source_token_total: usize,
    pub theme_summary: String,
    pub terminology_entries: Vec<TranslateTerminologyEntryCommand>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildTranslationLayerCommandRequest {
    pub task_id: String,
    pub media_path: String,
    pub source_lang: String,
    pub target_lang: String,
    pub segments: Vec<SourceSegmentForTerminologyCommand>,
    #[serde(default)]
    pub theme_summary: String,
    #[serde(default)]
    pub terminology_entries: Vec<TranslateTerminologyEntryCommand>,
    #[serde(default)]
    pub translate_api_key: String,
    #[serde(default)]
    pub translate_base_url: String,
    #[serde(default)]
    pub translate_model: String,
    #[serde(default = "default_llm_concurrency")]
    pub llm_concurrency: u32,
    #[serde(default = "default_batch_size")]
    pub batch_size: usize,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildTranslationSegmentCommand {
    pub segment_id: usize,
    pub start: f64,
    pub end: f64,
    pub source: String,
    pub translation: String,
    pub tokens: Vec<SegmentTokenForTerminologyCommand>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildTranslationLayerCommandResponse {
    pub task_id: String,
    pub media_path: String,
    pub source_lang: String,
    pub target_lang: String,
    pub batch_size: usize,
    pub batch_total: usize,
    pub segment_total: usize,
    pub theme_summary: String,
    pub terminology_entries: Vec<TranslateTerminologyEntryCommand>,
    pub segments: Vec<BuildTranslationSegmentCommand>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestTranslateLlmRequest {
    pub api_key: String,
    pub base_url: String,
    pub model: String,
    #[serde(default)]
    pub enable_vision_assist: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestTranslateLlmResponse {
    pub ok: bool,
    pub message: String,
    pub model: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListLlmModelsRequest {
    pub api_key: String,
    pub base_url: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmModelInfoDto {
    pub id: String,
    /// chat | image | video | audio | embedding | other
    pub kind: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListLlmModelsResponse {
    pub chat_models: Vec<LlmModelInfoDto>,
    pub excluded_models: Vec<LlmModelInfoDto>,
    pub all_models: Vec<LlmModelInfoDto>,
}

pub fn default_llm_concurrency() -> u32 {
    4
}

pub fn default_batch_size() -> usize {
    20
}

pub fn step5_schema_version() -> u32 {
    2
}

pub fn step5_pipeline_version() -> &'static str {
    "step5.v3"
}
