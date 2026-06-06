use serde::Deserialize;

use super::translate::SourceSegmentForTerminologyCommand;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
#[allow(dead_code)]
enum Step2SegmentsArtifactForInput {
    Flat(Vec<SourceSegmentForTerminologyCommand>),
    Wrapped(Step2SegmentsWrappedForInput),
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
struct Step2SegmentsWrappedForInput {
    #[serde(default)]
    segments: Vec<SourceSegmentForTerminologyCommand>,
}

#[allow(dead_code)]
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

#[allow(dead_code)]
pub fn default_task_id_from_path(path: &str) -> String {
    std::path::Path::new(path)
        .file_stem()
        .and_then(|name| name.to_str())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or("task")
        .to_string()
}
