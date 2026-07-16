use std::collections::{HashMap, HashSet};

use serde_json::Value;

use crate::services::llm::client::LlmSemanticValidationError;

use super::text::normalize_inline_text;

pub(super) fn validate_batch_translation_response(
    value: Value,
    expected_ids: &[usize],
) -> Result<HashMap<usize, String>, LlmSemanticValidationError> {
    let expected_set: HashSet<usize> = expected_ids.iter().copied().collect();
    let mut out = HashMap::<usize, String>::new();
    let mut seen_expected: HashSet<usize> = HashSet::new();
    let mut empty_ids: Vec<usize> = Vec::new();
    let mut duplicate_ids: Vec<usize> = Vec::new();
    let mut unexpected_ids: Vec<usize> = Vec::new();
    let mut structural_issues: Vec<String> = Vec::new();

    if let Some(items) = value.get("translations").and_then(|v| v.as_array()) {
        for (index, item) in items.iter().enumerate() {
            let Some(obj) = item.as_object() else {
                structural_issues.push(format!("translations[{index}] must be object"));
                continue;
            };
            let Some(id) = obj
                .get("id")
                .and_then(|v| v.as_u64())
                .map(|v| v as usize)
            else {
                structural_issues.push(format!("translations[{index}] missing numeric id"));
                continue;
            };
            record_item(
                id,
                extract_text(obj.get("text"))
                    .or_else(|| extract_text(obj.get("translation")))
                    .or_else(|| extract_text(obj.get("translatedText")))
                    .unwrap_or_default(),
                &expected_set,
                &mut out,
                &mut seen_expected,
                &mut empty_ids,
                &mut duplicate_ids,
                &mut unexpected_ids,
            );
        }
    } else if let Some(obj) = value.as_object() {
        for (key, item) in obj {
            let Ok(id) = key.parse::<usize>() else {
                // Ignore non-numeric top-level keys (commentary fields, etc.).
                continue;
            };
            let text = if let Some(s) = item.as_str() {
                normalize_inline_text(s)
            } else if let Some(map) = item.as_object() {
                extract_text(map.get("text"))
                    .or_else(|| extract_text(map.get("translation")))
                    .or_else(|| extract_text(map.get("translatedText")))
                    .unwrap_or_default()
            } else {
                structural_issues.push(format!("id {id} value must be string or object"));
                continue;
            };
            record_item(
                id,
                text,
                &expected_set,
                &mut out,
                &mut seen_expected,
                &mut empty_ids,
                &mut duplicate_ids,
                &mut unexpected_ids,
            );
        }
    } else {
        return Err(LlmSemanticValidationError::retryable(
            "translation response root must be object",
        ));
    }

    let mut missing_ids: Vec<usize> = expected_ids
        .iter()
        .copied()
        .filter(|id| !seen_expected.contains(id))
        .collect();
    // Empty entries were seen but are not valid output; keep them out of `out`.
    // They are reported as empty, not missing.
    missing_ids.sort_unstable();
    empty_ids.sort_unstable();
    empty_ids.dedup();
    duplicate_ids.sort_unstable();
    duplicate_ids.dedup();
    unexpected_ids.sort_unstable();
    unexpected_ids.dedup();

    let has_semantic_failure = !missing_ids.is_empty()
        || !empty_ids.is_empty()
        || !duplicate_ids.is_empty()
        || !structural_issues.is_empty();

    if has_semantic_failure {
        let mut got_ids: Vec<usize> = out.keys().copied().collect();
        got_ids.sort_unstable();

        let mut parts: Vec<String> = Vec::new();
        if !missing_ids.is_empty() {
            parts.push(format!("missing ids {}", format_id_list(&missing_ids)));
        }
        if !empty_ids.is_empty() {
            parts.push(format!("empty ids {}", format_id_list(&empty_ids)));
        }
        if !duplicate_ids.is_empty() {
            parts.push(format!("duplicate ids {}", format_id_list(&duplicate_ids)));
        }
        // Unexpected ids are advisory only (do not fail alone).
        if !unexpected_ids.is_empty() {
            parts.push(format!(
                "unexpected ids {}",
                format_id_list(&unexpected_ids)
            ));
        }
        if !structural_issues.is_empty() {
            parts.push(format!("structural: {}", structural_issues.join("; ")));
        }
        parts.push(format!("got ids {}", format_id_list(&got_ids)));
        parts.push(format!("expected {} items", expected_ids.len()));

        return Err(LlmSemanticValidationError::retryable(parts.join("; ")));
    }

    Ok(out)
}

fn extract_text(value: Option<&Value>) -> Option<String> {
    value.and_then(|v| v.as_str()).map(normalize_inline_text)
}

fn record_item(
    id: usize,
    text: String,
    expected_set: &HashSet<usize>,
    out: &mut HashMap<usize, String>,
    seen_expected: &mut HashSet<usize>,
    empty_ids: &mut Vec<usize>,
    duplicate_ids: &mut Vec<usize>,
    unexpected_ids: &mut Vec<usize>,
) {
    if !expected_set.contains(&id) {
        unexpected_ids.push(id);
        return;
    }

    if !seen_expected.insert(id) {
        // Already saw this id. Prefer a later non-empty value over an earlier empty.
        if text.is_empty() {
            // Empty after a prior entry: only hard-flag duplicate when we already
            // have a usable translation (two conflicting claims).
            if out.contains_key(&id) {
                duplicate_ids.push(id);
            }
            return;
        }
        if out.contains_key(&id) {
            // Keep first non-empty; report the conflict.
            duplicate_ids.push(id);
            return;
        }
        // Previous was empty only — recover with this non-empty text.
        empty_ids.retain(|&x| x != id);
        out.insert(id, text);
        return;
    }

    if text.is_empty() {
        empty_ids.push(id);
        return;
    }

    out.insert(id, text);
}

fn format_id_list(ids: &[usize]) -> String {
    format!(
        "[{}]",
        ids.iter()
            .map(|id| id.to_string())
            .collect::<Vec<_>>()
            .join(",")
    )
}
