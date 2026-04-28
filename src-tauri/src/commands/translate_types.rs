use serde::{Deserialize, Serialize};

use super::translate_defaults::{
    default_batch_size, default_llm_concurrency, default_subtitle_length_reference,
    default_subtitle_max_words_per_segment,
};

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

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Step5ArtifactMetaCommand {
    #[serde(default)]
    pub schema_version: u32,
    #[serde(default)]
    pub pipeline_version: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Step5QualityIssueCommand {
    #[serde(default)]
    pub rule_id: String,
    #[serde(default)]
    pub severity: String,
    #[serde(default)]
    pub segment_id: usize,
    #[serde(default)]
    pub part_id: usize,
    #[serde(default)]
    pub message: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Step5QualitySummaryCommand {
    #[serde(default)]
    pub passed: bool,
    #[serde(default)]
    pub hard_fail_count: usize,
    #[serde(default)]
    pub issue_count: usize,
    #[serde(default)]
    pub soft_score: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Step6FinalCheckMetricsCommand {
    #[serde(default)]
    pub segment_total: usize,
    #[serde(default)]
    pub empty_count: usize,
    #[serde(default)]
    pub ellipsis_tail_count: usize,
    #[serde(default)]
    pub numeric_drift_count: usize,
    #[serde(default)]
    pub cross_line_leak_count: usize,
    #[serde(default)]
    pub gt25_count: usize,
    #[serde(default)]
    pub gt32_count: usize,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildStep51SourceSplitCommandRequest {
    pub task_id: String,
    pub media_path: String,
    pub source_lang: String,
    pub target_lang: String,
    pub segments: Vec<BuildTranslationSegmentCommand>,
    #[serde(default)]
    pub translate_api_key: String,
    #[serde(default)]
    pub translate_base_url: String,
    #[serde(default)]
    pub translate_model: String,
    #[serde(default = "default_llm_concurrency")]
    pub llm_concurrency: u32,
    #[serde(default = "default_subtitle_max_words_per_segment")]
    pub subtitle_max_words_per_segment: u32,
    #[serde(default = "default_subtitle_length_reference")]
    pub subtitle_length_reference: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Step5SplitPartCommand {
    pub part_id: usize,
    pub start: f64,
    pub end: f64,
    pub source: String,
    pub tokens: Vec<SegmentTokenForTerminologyCommand>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Step5SplitParentCommand {
    pub parent_segment_id: usize,
    pub draft_translation: String,
    pub parts: Vec<Step5SplitPartCommand>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildStep51SourceSplitCommandResponse {
    pub task_id: String,
    pub media_path: String,
    pub source_lang: String,
    pub target_lang: String,
    #[serde(default)]
    pub schema_version: u32,
    #[serde(default)]
    pub pipeline_version: String,
    #[serde(default)]
    pub artifact_meta: Step5ArtifactMetaCommand,
    #[serde(default)]
    pub quality_summary: Step5QualitySummaryCommand,
    #[serde(default = "default_subtitle_max_words_per_segment")]
    pub subtitle_max_words_per_segment: u32,
    pub subtitle_length_reference: u32,
    pub parent_total: usize,
    pub part_total: usize,
    pub parents: Vec<Step5SplitParentCommand>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildStep52TranslationAlignCommandRequest {
    pub task_id: String,
    pub media_path: String,
    pub source_lang: String,
    pub target_lang: String,
    pub theme_summary: String,
    pub parents: Vec<Step5SplitParentCommand>,
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
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Step5AlignedPartCommand {
    pub part_id: usize,
    pub start: f64,
    pub end: f64,
    pub source: String,
    pub translation: String,
    pub tokens: Vec<SegmentTokenForTerminologyCommand>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Step5AlignedParentCommand {
    pub parent_segment_id: usize,
    pub parts: Vec<Step5AlignedPartCommand>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildStep52TranslationAlignCommandResponse {
    pub task_id: String,
    pub media_path: String,
    pub source_lang: String,
    pub target_lang: String,
    #[serde(default)]
    pub schema_version: u32,
    #[serde(default)]
    pub pipeline_version: String,
    #[serde(default)]
    pub artifact_meta: Step5ArtifactMetaCommand,
    #[serde(default)]
    pub quality_summary: Step5QualitySummaryCommand,
    pub theme_summary: String,
    pub terminology_entries: Vec<TranslateTerminologyEntryCommand>,
    pub parent_total: usize,
    pub part_total: usize,
    pub parents: Vec<Step5AlignedParentCommand>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildStep53TranslationPolishCommandRequest {
    pub task_id: String,
    pub media_path: String,
    pub source_lang: String,
    pub target_lang: String,
    pub theme_summary: String,
    #[serde(default)]
    pub terminology_entries: Vec<TranslateTerminologyEntryCommand>,
    pub parents: Vec<Step5AlignedParentCommand>,
    #[serde(default)]
    pub translate_api_key: String,
    #[serde(default)]
    pub translate_base_url: String,
    #[serde(default)]
    pub translate_model: String,
    #[serde(default = "default_llm_concurrency")]
    pub llm_concurrency: u32,
    #[serde(default = "default_subtitle_length_reference")]
    pub subtitle_length_reference: u32,
    #[serde(default = "default_batch_size")]
    pub batch_size: usize,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildStep53TranslationPolishCommandResponse {
    pub task_id: String,
    pub media_path: String,
    pub source_lang: String,
    pub target_lang: String,
    #[serde(default)]
    pub schema_version: u32,
    #[serde(default)]
    pub pipeline_version: String,
    #[serde(default)]
    pub artifact_meta: Step5ArtifactMetaCommand,
    #[serde(default)]
    pub quality_summary: Step5QualitySummaryCommand,
    pub batch_size: usize,
    pub batch_total: usize,
    pub segment_total: usize,
    pub theme_summary: String,
    pub terminology_entries: Vec<TranslateTerminologyEntryCommand>,
    pub segments: Vec<BuildTranslationSegmentCommand>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildStep6FinalCheckCommandRequest {
    pub task_id: String,
    pub media_path: String,
    pub source_lang: String,
    pub target_lang: String,
    pub segments: Vec<BuildTranslationSegmentCommand>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildStep6FinalCheckCommandResponse {
    pub task_id: String,
    pub media_path: String,
    pub source_lang: String,
    pub target_lang: String,
    #[serde(default)]
    pub schema_version: u32,
    #[serde(default)]
    pub pipeline_version: String,
    #[serde(default)]
    pub artifact_meta: Step5ArtifactMetaCommand,
    #[serde(default)]
    pub quality_summary: Step5QualitySummaryCommand,
    #[serde(default)]
    pub metrics: Step6FinalCheckMetricsCommand,
    #[serde(default)]
    pub issues: Vec<Step5QualityIssueCommand>,
    #[serde(default)]
    pub segments: Vec<BuildTranslationSegmentCommand>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestTranslateLlmRequest {
    pub api_key: String,
    pub base_url: String,
    pub model: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestTranslateLlmResponse {
    pub ok: bool,
    pub message: String,
    pub model: String,
}
