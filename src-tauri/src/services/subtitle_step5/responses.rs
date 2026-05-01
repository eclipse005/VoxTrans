use std::collections::HashMap;

use serde_json::Value;

use crate::services::llm::client::LlmSemanticValidationError;

use super::text_utils::normalize_inline_text;

pub(super) fn validate_source_split_response(
    value: Value,
    source_text: &str,
    require_split: bool,
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
    if out.is_empty() {
        return Err(LlmSemanticValidationError::retryable(
            "sourceParts has too few items",
        ));
    }
    if out.len() > 2 {
        return Err(LlmSemanticValidationError::retryable(
            "sourceParts must contain at most two items",
        ));
    }
    if require_split && out.len() < 2 {
        return Err(LlmSemanticValidationError::retryable(
            "sourceParts must contain two items for this overlong sourceText",
        ));
    }
    let expected = compact_for_split_match(source_text);
    let actual = compact_for_split_match(&out.join(""));
    if !expected.is_empty() && expected != actual {
        return Err(LlmSemanticValidationError::retryable(
            "sourceParts must concatenate back to sourceText",
        ));
    }
    Ok(out)
}

pub(super) fn compact_for_split_match(text: &str) -> String {
    normalize_inline_text(text)
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect()
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
