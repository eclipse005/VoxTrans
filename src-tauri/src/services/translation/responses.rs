use std::collections::{HashMap, HashSet};

use serde_json::Value;

use crate::services::llm::client::LlmSemanticValidationError;

use super::text::normalize_inline_text;

pub(super) struct TranslationValidationContext<'a> {
    pub expected_ids: &'a [usize],
    pub source_lang: &'a str,
    pub target_lang: &'a str,
    /// Terminology targets enforced verbatim on this batch; their source-script
    /// characters are exempt from the leak check.
    pub enforced_targets: &'a [String],
}

#[cfg(test)]
pub(super) fn validate_batch_translation_response(
    value: Value,
    expected_ids: &[usize],
) -> Result<HashMap<usize, String>, LlmSemanticValidationError> {
    validate_batch_translation_response_with_context(
        value,
        &TranslationValidationContext {
            expected_ids,
            source_lang: "",
            target_lang: "",
            enforced_targets: &[],
        },
    )
}

pub(super) fn validate_batch_translation_response_with_context(
    value: Value,
    ctx: &TranslationValidationContext<'_>,
) -> Result<HashMap<usize, String>, LlmSemanticValidationError> {
    let expected_ids = ctx.expected_ids;
    let expected_set: HashSet<usize> = expected_ids.iter().copied().collect();
    let mut out = HashMap::<usize, String>::new();
    let mut seen_expected: HashSet<usize> = HashSet::new();
    let mut empty_ids: Vec<usize> = Vec::new();
    let mut duplicate_ids: Vec<usize> = Vec::new();
    let mut unexpected_ids: Vec<usize> = Vec::new();

    let mut collected = Vec::new();
    collect_translation_pairs(&value, &mut collected);
    for (id, text) in collected {
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
        || !duplicate_ids.is_empty();

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
        parts.push(format!("got ids {}", format_id_list(&got_ids)));
        parts.push(format!("expected {} items", expected_ids.len()));

        return Err(LlmSemanticValidationError::retryable(parts.join("; ")));
    }

    let ordered: Vec<(usize, &str)> = expected_ids
        .iter()
        .filter_map(|id| out.get(id).map(|text| (*id, text.as_str())))
        .collect();
    let leak_ids = super::guard::language_leak_ids(
        &ordered,
        ctx.source_lang,
        ctx.target_lang,
        ctx.enforced_targets,
    );
    if !leak_ids.is_empty() {
        return Err(LlmSemanticValidationError::retryable(format!(
            "source-language leak on ids {}",
            format_id_list(&leak_ids)
        )));
    }

    Ok(out)
}

fn collect_translation_pairs(value: &Value, out: &mut Vec<(usize, String)>) {
    if let Some(pair) = translation_item(value) {
        out.push(pair);
        return;
    }
    match value {
        Value::Array(items) => {
            for item in items {
                collect_translation_pairs(item, out);
            }
        }
        Value::Object(map) => {
            for (key, child) in map {
                if let Ok(id) = key.parse::<usize>() {
                    if let Some(text) = map_entry_text(child) {
                        out.push((id, text));
                    }
                    continue;
                }
                collect_translation_pairs(child, out);
            }
        }
        _ => {}
    }
}

fn translation_item(value: &Value) -> Option<(usize, String)> {
    let obj = value.as_object()?;
    let id = json_id(obj.get("id")?)?;
    if !obj.contains_key("text")
        && !obj.contains_key("translation")
        && !obj.contains_key("translatedText")
    {
        return None;
    }
    let text = extract_text(obj.get("text"))
        .or_else(|| extract_text(obj.get("translation")))
        .or_else(|| extract_text(obj.get("translatedText")))
        .unwrap_or_default();
    Some((id, text))
}

fn json_id(value: &Value) -> Option<usize> {
    value
        .as_u64()
        .map(|n| n as usize)
        .or_else(|| value.as_str()?.parse().ok())
}

fn map_entry_text(value: &Value) -> Option<String> {
    if let Some(text) = extract_text(Some(value)) {
        return Some(text);
    }
    let obj = value.as_object()?;
    extract_text(obj.get("text"))
        .or_else(|| extract_text(obj.get("translation")))
        .or_else(|| extract_text(obj.get("translatedText")))
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
