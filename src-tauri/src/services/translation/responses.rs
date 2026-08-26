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
    /// Source texts in the same order as `expected_ids`; used to exempt
    /// genuinely repeated source lines from the adjacent-duplicate check.
    /// Empty means "no source information" — duplicates are always suspect.
    pub source_texts: &'a [String],
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
            source_texts: &[],
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

    // Merge/shift signature: a model that fuses two source lines into one
    // translation and pads the id list by repeating a neighbouring translation
    // passes the id-count check while the content is shifted by one line.
    // Adjacent identical translations over *different* source lines are the
    // observable trace. Genuinely repeated source lines ("Okay?" / "Okay?")
    // are exempt when sources are provided. This failure is retryable: the
    // client retries, and persistent failure feeds the split ladder
    // (20→10→5→1), which shrinks the window until merging is impossible.
    let dup_ids = adjacent_duplicate_ids(&ordered, ctx.source_texts);
    if !dup_ids.is_empty() {
        return Err(LlmSemanticValidationError::retryable(format!(
            "adjacent duplicate translations on ids {}; each currentLines id needs its own translation — do not merge lines or repeat a neighbouring translation",
            format_id_list(&dup_ids)
        )));
    }

    Ok(out)
}

/// Min length (chars) for an adjacent duplicate pair to be suspect. Short
/// interjections ("对吧？") repeat legitimately far too often to flag.
const MIN_DUPLICATE_CHARS: usize = 10;

fn adjacent_duplicate_ids(
    ordered: &[(usize, &str)],
    source_texts: &[String],
) -> Vec<usize> {
    let mut ids = Vec::new();
    for (index, pair) in ordered.windows(2).enumerate() {
        let (id_a, text_a) = pair[0];
        let (id_b, text_b) = pair[1];
        if text_a != text_b || text_a.chars().count() < MIN_DUPLICATE_CHARS {
            continue;
        }
        let same_source = source_texts
            .get(index)
            .zip(source_texts.get(index + 1))
            .is_some_and(|(a, b)| a == b);
        if same_source {
            continue;
        }
        ids.push(id_a);
        ids.push(id_b);
    }
    ids.sort_unstable();
    ids.dedup();
    ids
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
