use serde::{Deserialize, Serialize};

use super::translate::{
    BuildTranslationLayerCommandResponse, BuildTranslationSegmentCommand,
    SourceSegmentForTerminologyCommand, TranslateTerminologyEntryCommand,
};

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum Step2SegmentsArtifactForInput {
    Flat(Vec<SourceSegmentForTerminologyCommand>),
    Wrapped(Step2SegmentsWrappedForInput),
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Step2SegmentsWrappedForInput {
    #[serde(default)]
    segments: Vec<SourceSegmentForTerminologyCommand>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Step3TerminologyArtifactForInput {
    #[serde(default)]
    pub task_id: String,
    #[serde(default)]
    pub media_path: String,
    #[serde(default)]
    pub source_lang: String,
    #[serde(default)]
    pub target_lang: String,
    #[serde(default)]
    pub theme_summary: String,
    #[serde(default)]
    pub terminology_entries: Vec<TranslateTerminologyEntryCommand>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Step3TerminologyArtifactForCli {
    pub task_id: String,
    pub media_path: String,
    pub source_lang: String,
    pub target_lang: String,
    pub source_segment_total: usize,
    pub source_token_total: usize,
    pub theme_summary: String,
    pub terminology_entries: Vec<TranslateTerminologyEntryCommand>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Step4TranslationArtifactForCli {
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

pub fn artifact_dir_from_file_path(path: &str) -> Result<std::path::PathBuf, String> {
    let parent = std::path::PathBuf::from(path)
        .parent()
        .ok_or_else(|| "input path has no parent directory".to_string())?
        .to_path_buf();
    Ok(normalize_artifact_dir(&parent))
}

pub fn normalize_artifact_dir(path: &std::path::Path) -> std::path::PathBuf {
    if path
        .file_name()
        .and_then(|v| v.to_str())
        .map(|name| name.eq_ignore_ascii_case(crate::services::task_path::ARTIFACTS_DIR_NAME))
        .unwrap_or(false)
    {
        path.to_path_buf()
    } else {
        path.join(crate::services::task_path::ARTIFACTS_DIR_NAME)
    }
}

pub fn parse_step2_segments_artifact_for_input(
    raw: &str,
) -> Result<Vec<SourceSegmentForTerminologyCommand>, String> {
    let parsed: Step2SegmentsArtifactForInput = serde_json::from_str(raw)
        .map_err(|err| format!("failed to parse step2 segments json: {err}"))?;
    let segments = match parsed {
        Step2SegmentsArtifactForInput::Flat(items) => items,
        Step2SegmentsArtifactForInput::Wrapped(wrapper) => wrapper.segments,
    };
    Ok(segments)
}

pub fn parse_step3_terminology_artifact_for_input(
    raw: &str,
) -> Result<Step3TerminologyArtifactForInput, String> {
    serde_json::from_str::<Step3TerminologyArtifactForInput>(raw)
        .map_err(|err| format!("failed to parse step3 terminology json: {err}"))
}

pub fn parse_step4_translation_artifact_for_input(
    raw: &str,
) -> Result<BuildTranslationLayerCommandResponse, String> {
    serde_json::from_str::<BuildTranslationLayerCommandResponse>(raw)
        .map_err(|err| format!("failed to parse step4 translation json: {err}"))
}

pub fn default_task_id_from_path(path: &str) -> String {
    std::path::Path::new(path)
        .file_stem()
        .and_then(|name| name.to_str())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or("task")
        .to_string()
}
