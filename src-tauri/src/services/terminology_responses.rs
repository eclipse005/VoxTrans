use serde::{Deserialize, Serialize};

use crate::services::llm::client::LlmSemanticValidationError;

use super::terminology::TerminologyEntry;

/// Parsed per-window briefing: the glossary terms extracted from this window
/// and (for the first window) the style guide that drives the translator.
pub(super) struct BriefingResponse {
    pub glossary: Vec<TerminologyEntry>,
    pub style_guide: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct BriefingExtraction {
    #[serde(default)]
    glossary: Vec<TerminologyEntryExtraction>,
    #[serde(default)]
    #[serde(alias = "style_guide")]
    style_guide: String,
    #[serde(default)]
    output: Option<BriefingOutputExtraction>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct BriefingOutputExtraction {
    #[serde(default)]
    glossary: Vec<TerminologyEntryExtraction>,
    #[serde(default)]
    #[serde(alias = "style_guide")]
    style_guide: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct TerminologyEntryExtraction {
    #[serde(default)]
    source: String,
    #[serde(default)]
    target: String,
    #[serde(default)]
    note: String,
}

pub(super) fn parse_briefing_response(
    value: serde_json::Value,
) -> Result<BriefingResponse, LlmSemanticValidationError> {
    let result = serde_json::from_value::<BriefingExtraction>(value).map_err(|err| {
        LlmSemanticValidationError::retryable(format!("briefing parse failed: {err}"))
    })?;

    let glossary = if result.glossary.is_empty() {
        result
            .output
            .as_ref()
            .map(|item| item.glossary.clone())
            .unwrap_or_default()
    } else {
        result.glossary
    };
    let style_guide = if result.style_guide.trim().is_empty() {
        result
            .output
            .as_ref()
            .map(|item| item.style_guide.clone())
            .unwrap_or_default()
    } else {
        result.style_guide
    };

    let glossary = glossary
        .into_iter()
        .map(|entry| TerminologyEntry {
            source: entry.source,
            target: entry.target,
            note: entry.note,
        })
        .collect();

    Ok(BriefingResponse {
        glossary,
        style_guide: style_guide.trim().to_string(),
    })
}
