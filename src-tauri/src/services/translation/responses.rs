use std::collections::HashMap;

use serde_json::Value;

use crate::services::llm::client::LlmSemanticValidationError;

use super::text::normalize_inline_text;

pub(super) fn validate_batch_translation_response(
    value: Value,
    expected_ids: &[usize],
) -> Result<HashMap<usize, String>, LlmSemanticValidationError> {
    let mut out = HashMap::<usize, String>::new();

    if let Some(items) = value.get("translations").and_then(|v| v.as_array()) {
        for item in items {
            let Some(obj) = item.as_object() else {
                return Err(LlmSemanticValidationError::retryable(
                    "translations item must be object",
                ));
            };
            let id = obj
                .get("id")
                .and_then(|v| v.as_u64())
                .map(|v| v as usize)
                .ok_or_else(|| {
                    LlmSemanticValidationError::retryable("translation id is required")
                })?;
            let text = obj
                .get("text")
                .or_else(|| obj.get("translation"))
                .or_else(|| obj.get("translatedText"))
                .and_then(|v| v.as_str())
                .map(normalize_inline_text)
                .unwrap_or_default();
            if !expected_ids.contains(&id) {
                continue;
            }
            if text.is_empty() {
                return Err(LlmSemanticValidationError::retryable(format!(
                    "translation id {id} must be non-empty"
                )));
            }
            if out.insert(id, text).is_some() {
                return Err(LlmSemanticValidationError::retryable(format!(
                    "duplicate translation id {id}"
                )));
            }
        }
    } else if let Some(obj) = value.as_object() {
        for (key, item) in obj {
            let id = key.parse::<usize>().map_err(|_| {
                LlmSemanticValidationError::retryable("translation map key must be numeric id")
            })?;
            let text = item
                .get("text")
                .or_else(|| item.get("translation"))
                .or_else(|| item.get("translatedText"))
                .and_then(|v| v.as_str())
                .map(normalize_inline_text)
                .unwrap_or_default();
            if !expected_ids.contains(&id) {
                continue;
            }
            if text.is_empty() {
                return Err(LlmSemanticValidationError::retryable(format!(
                    "translation id {id} must be non-empty"
                )));
            }
            if out.insert(id, text).is_some() {
                return Err(LlmSemanticValidationError::retryable(format!(
                    "duplicate translation id {id}"
                )));
            }
        }
    } else {
        return Err(LlmSemanticValidationError::retryable(
            "translation response root must be object",
        ));
    }

    for expected_id in expected_ids {
        if !out.contains_key(expected_id) {
            return Err(LlmSemanticValidationError::retryable(format!(
                "missing translation id {expected_id}"
            )));
        }
    }

    Ok(out)
}
