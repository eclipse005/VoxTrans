use serde::{Deserialize, Serialize};

use crate::services::llm::client::LlmSemanticValidationError;

use super::terminology::TerminologyEntry;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ThemeExtraction {
    #[serde(default)]
    theme: String,
    #[serde(default)]
    output: Option<ThemeOutputExtraction>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct UserTermFilterExtraction {
    #[serde(default)]
    #[serde(alias = "keep_indexes")]
    keep_indexes: Vec<usize>,
    #[serde(default)]
    output: Option<UserTermFilterOutputExtraction>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ExtractedTermsExtraction {
    #[serde(default)]
    terms: Vec<TerminologyEntryExtraction>,
    #[serde(default)]
    output: Option<ExtractedTermsOutputExtraction>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ThemeOutputExtraction {
    #[serde(default)]
    theme: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct UserTermFilterOutputExtraction {
    #[serde(default)]
    #[serde(alias = "keep_indexes")]
    keep_indexes: Vec<usize>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ExtractedTermsOutputExtraction {
    #[serde(default)]
    terms: Vec<TerminologyEntryExtraction>,
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

pub(super) fn parse_theme_response(
    value: serde_json::Value,
) -> Result<String, LlmSemanticValidationError> {
    let result = serde_json::from_value::<ThemeExtraction>(value).map_err(|err| {
        LlmSemanticValidationError::retryable(format!("theme parse failed: {err}"))
    })?;
    if !result.theme.trim().is_empty() {
        return Ok(result.theme);
    }
    Ok(result
        .output
        .as_ref()
        .map(|item| item.theme.clone())
        .unwrap_or_default())
}

pub(super) fn parse_user_term_filter_response(
    value: serde_json::Value,
) -> Result<Vec<usize>, LlmSemanticValidationError> {
    let result = serde_json::from_value::<UserTermFilterExtraction>(value).map_err(|err| {
        LlmSemanticValidationError::retryable(format!("user term filter parse failed: {err}"))
    })?;
    if !result.keep_indexes.is_empty() {
        return Ok(result.keep_indexes);
    }
    Ok(result
        .output
        .as_ref()
        .map(|item| item.keep_indexes.clone())
        .unwrap_or_default())
}

pub(super) fn parse_extracted_terms_response(
    value: serde_json::Value,
) -> Result<Vec<TerminologyEntry>, LlmSemanticValidationError> {
    let result = serde_json::from_value::<ExtractedTermsExtraction>(value).map_err(|err| {
        LlmSemanticValidationError::retryable(format!("term extraction parse failed: {err}"))
    })?;
    let extracted = if result.terms.is_empty() {
        result
            .output
            .as_ref()
            .map(|item| item.terms.clone())
            .unwrap_or_default()
    } else {
        result.terms
    };
    Ok(extracted
        .into_iter()
        .map(|entry| TerminologyEntry {
            source: entry.source,
            target: entry.target,
            note: entry.note,
        })
        .collect())
}
