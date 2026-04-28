use std::collections::HashMap;

use serde_json::Value;

use crate::services::llm::client::LlmSemanticValidationError;

use super::normalize_inline_text;

pub(super) fn validate_source_split_response(
    value: Value,
    min_parts: usize,
) -> Result<Vec<String>, LlmSemanticValidationError> {
    let Some(items) = value
        .get("sourceParts")
        .or_else(|| value.get("source_parts"))
        .or_else(|| value.get("parts"))
        .and_then(|v| v.as_array())
    else {
        return Err(LlmSemanticValidationError::retryable(
            "sourceParts array is required",
        ));
    };
    let mut out = Vec::<String>::new();
    for item in items {
        let Some(text) = item.as_str() else {
            continue;
        };
        let text = normalize_inline_text(text);
        if !text.is_empty() {
            out.push(text);
        }
    }
    if out.len() < min_parts.max(2) {
        return Err(LlmSemanticValidationError::retryable(
            "sourceParts has too few items",
        ));
    }
    if out.len() > min_parts.max(2) {
        out.truncate(min_parts.max(2));
    }
    Ok(out)
}

pub(super) fn validate_align_response(
    value: Value,
    expected_ids: &[usize],
) -> Result<HashMap<usize, String>, LlmSemanticValidationError> {
    let mut out = HashMap::<usize, String>::new();
    let Some(items) = value.get("translations").and_then(|v| v.as_array()) else {
        return Err(LlmSemanticValidationError::retryable(
            "translations array is required",
        ));
    };
    for item in items {
        let Some(obj) = item.as_object() else {
            continue;
        };
        let Some(id) = obj.get("id").and_then(|v| v.as_u64()).map(|v| v as usize) else {
            continue;
        };
        if !expected_ids.contains(&id) {
            continue;
        }
        let text = obj
            .get("text")
            .or_else(|| obj.get("translation"))
            .and_then(|v| v.as_str())
            .map(normalize_inline_text)
            .unwrap_or_default();
        out.insert(id, text);
    }
    for expected_id in expected_ids {
        out.entry(*expected_id).or_insert_with(String::new);
    }
    Ok(out)
}

pub(super) fn validate_polish_response(value: Value) -> Result<String, LlmSemanticValidationError> {
    let text = value
        .get("text")
        .or_else(|| value.get("translation"))
        .and_then(|v| v.as_str())
        .map(normalize_inline_text)
        .unwrap_or_default();
    Ok(text)
}
