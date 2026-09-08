//! Response validation for the sentence-unit + display-slot protocol.
//!
//! The model must answer `{"units":[{"id":1,"translation":"...","slots":
//! [{"id":101,"text":"..."}]}]}`. Per unit: every display-slot id present and
//! non-empty, and the normalized concatenation of the slot texts must
//! reproduce the unit translation. Failures are retryable — the client
//! retries, and persistent failure feeds the split ladder (8→4→2→1 units).
//! At the ladder floor (a single-unit window) lenient mode salvages what it
//! can instead of failing the whole task: complete-but-mismatched slots are
//! kept (content is still the model's sentence translation), and a unit with
//! only a translation is backfilled deterministically by the caller.

use std::collections::HashMap;

use serde_json::Value;

use crate::services::llm::client::LlmSemanticValidationError;

use super::slots::norm_core;
use super::text::normalize_inline_text;

/// Expected shape of one unit (from the batch window): display slots in order
/// (global segment id + cue source) and the unit's source sentence.
pub(super) struct SlotUnitExpectation<'a> {
    pub slots: &'a [(usize, String)],
    pub source: &'a str,
}

pub(super) struct SlotValidationContext<'a> {
    pub units: &'a [SlotUnitExpectation<'a>],
    pub source_lang: &'a str,
    pub target_lang: &'a str,
    /// Terminology targets enforced verbatim on this batch; their source-script
    /// characters are exempt from the leak check.
    pub enforced_targets: &'a [String],
    /// Lenient mode (single-unit ladder floor): salvage soft failures instead
    /// of erroring.
    pub lenient: bool,
}

#[derive(Debug, Clone)]
pub(super) enum UnitFill {
    /// Slot texts in the unit's expected slot order.
    Slots(Vec<String>),
    /// Only the unit translation could be salvaged; the caller distributes it
    /// proportionally onto the unit's display slots.
    TranslationOnly(String),
}

#[derive(Debug, Clone)]
pub(super) struct ValidatedUnit {
    pub translation: String,
    pub fill: UnitFill,
}

pub(super) fn validate_sentence_slot_response_with_context(
    value: Value,
    ctx: &SlotValidationContext<'_>,
) -> Result<Vec<ValidatedUnit>, LlmSemanticValidationError> {
    // Collect unit candidates from anywhere in the JSON tree (envelope-tolerant).
    let mut candidates: HashMap<usize, UnitCandidate> = HashMap::new();
    let mut duplicate_unit_ids: Vec<usize> = Vec::new();
    collect_unit_candidates(&value, &mut candidates, &mut duplicate_unit_ids);

    let mut out = Vec::<ValidatedUnit>::with_capacity(ctx.units.len());
    let mut missing_ids: Vec<usize> = Vec::new();
    let mut empty_ids: Vec<usize> = Vec::new();
    let mut issues: Vec<String> = Vec::new();
    let mut checked: Vec<(usize, String)> = Vec::new();

    for (index, expected) in ctx.units.iter().enumerate() {
        let unit_id = index + 1;
        let Some(candidate) = candidates.get(&unit_id) else {
            missing_ids.push(unit_id);
            continue;
        };
        let translation = candidate.translation.trim().to_string();
        if translation.is_empty() {
            empty_ids.push(unit_id);
            continue;
        }

        // Slot protocol: every expected slot id present, non-empty.
        let model_slots: HashMap<usize, String> = candidate
            .slots
            .iter()
            .filter(|(id, _)| expected.slots.iter().any(|(eid, _)| eid == id))
            .map(|(id, text)| (*id, text.trim().to_string()))
            .collect();
        let missing_slot_ids: Vec<usize> = expected
            .slots
            .iter()
            .filter(|(id, _)| model_slots.get(id).map_or(true, |text| text.is_empty()))
            .map(|(id, _)| *id)
            .collect();

        if !missing_slot_ids.is_empty() {
            if ctx.lenient {
                out.push(ValidatedUnit {
                    translation: translation.clone(),
                    fill: UnitFill::TranslationOnly(translation.clone()),
                });
                checked.push((unit_id, translation));
                continue;
            }
            issues.push(format!(
                "unit {unit_id}: displaySlots incomplete — missing or empty slot ids [{}]; fill every slot of the unit",
                format_id_list(&missing_slot_ids)
            ));
            continue;
        }

        // Accept gate: slot concatenation must reproduce the translation.
        let slot_texts: Vec<String> = expected
            .slots
            .iter()
            .map(|(id, _)| model_slots[id].clone())
            .collect();
        let concat_core = norm_core(&slot_texts.concat());
        let translation_core = norm_core(&translation);
        if concat_core != translation_core {
            if ctx.lenient {
                // Soft accept: content is the model's sentence translation and
                // the slots are complete; keep them (the boundary repair pass
                // still cleans up deterministic defects).
                out.push(ValidatedUnit {
                    translation: translation.clone(),
                    fill: UnitFill::Slots(slot_texts),
                });
                checked.push((unit_id, translation));
                continue;
            }
            issues.push(format!(
                "unit {unit_id}: concatenating displaySlot texts must reproduce the unit translation — got \"{}\", translation \"{}\"; re-split the same translation across the slots without changing its words",
                truncate(&slot_texts.concat(), 60),
                truncate(&translation, 60)
            ));
            continue;
        }

        out.push(ValidatedUnit {
            translation: translation.clone(),
            fill: UnitFill::Slots(slot_texts),
        });
        checked.push((unit_id, translation));
    }

    let mut parts: Vec<String> = Vec::new();
    if !missing_ids.is_empty() {
        parts.push(format!("missing unit ids {}", format_id_list(&missing_ids)));
    }
    if !empty_ids.is_empty() {
        parts.push(format!("empty unit ids {}", format_id_list(&empty_ids)));
    }
    if !duplicate_unit_ids.is_empty() {
        parts.push(format!(
            "duplicate unit ids {}",
            format_id_list(&duplicate_unit_ids)
        ));
    }
    parts.extend(issues);
    if !parts.is_empty() {
        parts.push(format!("expected {} units", ctx.units.len()));
        return Err(LlmSemanticValidationError::retryable(parts.join("; ")));
    }

    // Source-script leak on the unit translations.
    let leak_ids = super::guard::language_leak_ids(
        &checked.iter().map(|(id, t)| (*id, t.as_str())).collect::<Vec<_>>(),
        ctx.source_lang,
        ctx.target_lang,
        ctx.enforced_targets,
    );
    if !leak_ids.is_empty() {
        return Err(LlmSemanticValidationError::retryable(format!(
            "source-language leak on unit ids {}",
            format_id_list(&leak_ids)
        )));
    }

    // Merge/shift signature over units: a model that fused two source
    // sentences and padded ids repeats a neighbouring translation.
    let sources: Vec<String> = ctx.units.iter().map(|u| u.source.to_string()).collect();
    let dup_ids = adjacent_duplicate_ids(&checked, &sources);
    if !dup_ids.is_empty() {
        return Err(LlmSemanticValidationError::retryable(format!(
            "adjacent duplicate translations on unit ids {}; each currentUnits id needs its own translation — do not merge units or repeat a neighbouring translation",
            format_id_list(&dup_ids)
        )));
    }

    Ok(out)
}

#[derive(Debug, Clone, Default)]
struct UnitCandidate {
    translation: String,
    slots: Vec<(usize, String)>,
}

fn collect_unit_candidates(
    value: &Value,
    out: &mut HashMap<usize, UnitCandidate>,
    duplicate_unit_ids: &mut Vec<usize>,
) {
    match value {
        Value::Array(items) => {
            for item in items {
                collect_unit_candidates(item, out, duplicate_unit_ids);
            }
        }
        Value::Object(map) => {
            if let Some(candidate) = unit_item(map) {
                if let Some(id) = json_id(map.get("id")) {
                    if out.contains_key(&id) {
                        duplicate_unit_ids.push(id);
                        // Keep the first candidate that carries slots; a later
                        // bare translation cannot beat a slotted one.
                        if out[&id].slots.is_empty() && !candidate.slots.is_empty() {
                            out.insert(id, candidate);
                        }
                    } else {
                        out.insert(id, candidate);
                    }
                    return;
                }
            }
            for child in map.values() {
                collect_unit_candidates(child, out, duplicate_unit_ids);
            }
        }
        _ => {}
    }
}

/// A unit object: has an id, a slots array, and a translation-ish text.
fn unit_item(map: &serde_json::Map<String, Value>) -> Option<UnitCandidate> {
    if !map.contains_key("slots") {
        return None;
    }
    let slots_value = map.get("slots")?;
    let slots_array = slots_value.as_array()?;
    let mut slots = Vec::new();
    for item in slots_array {
        let Some(obj) = item.as_object() else { continue };
        let Some(id) = json_id(obj.get("id")) else {
            continue;
        };
        let text = extract_text(obj.get("text"))
            .or_else(|| extract_text(obj.get("translation")))
            .unwrap_or_default();
        slots.push((id, text));
    }
    let translation = extract_text(map.get("translation"))
        .or_else(|| extract_text(map.get("text")))
        .or_else(|| extract_text(map.get("translatedText")))
        .unwrap_or_default();
    Some(UnitCandidate { translation, slots })
}

/// Min length (chars) for an adjacent duplicate pair to be suspect. Short
/// interjections ("对吧？") repeat legitimately far too often to flag.
const MIN_DUPLICATE_CHARS: usize = 10;

fn adjacent_duplicate_ids(
    ordered: &[(usize, String)],
    source_texts: &[String],
) -> Vec<usize> {
    let mut ids = Vec::new();
    for (index, pair) in ordered.windows(2).enumerate() {
        let (id_a, text_a) = (&pair[0].0, pair[0].1.as_str());
        let (id_b, text_b) = (&pair[1].0, pair[1].1.as_str());
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
        ids.push(*id_a);
        ids.push(*id_b);
    }
    ids.sort_unstable();
    ids.dedup();
    ids
}

fn json_id(value: Option<&Value>) -> Option<usize> {
    value?.as_u64().map(|n| n as usize).or_else(|| value?.as_str()?.parse().ok())
}

fn extract_text(value: Option<&Value>) -> Option<String> {
    value.and_then(|v| v.as_str()).map(normalize_inline_text)
}

fn truncate(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max_chars).collect();
    out.push('…');
    out
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

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx<'a>(
        units: &'a [SlotUnitExpectation<'a>],
        lenient: bool,
    ) -> SlotValidationContext<'a> {
        SlotValidationContext {
            units,
            source_lang: "en",
            target_lang: "zh-CN",
            enforced_targets: &[],
            lenient,
        }
    }

    fn unit_expectation<'a>(
        slots: &'a [(usize, String)],
        source: &'a str,
    ) -> SlotUnitExpectation<'a> {
        SlotUnitExpectation { slots, source }
    }

    #[test]
    fn accepts_complete_slot_protocol() {
        let slots = vec![
            (101, "Hence".to_string()),
            (102, "this video.".to_string()),
        ];
        let units = [unit_expectation(&slots, "Hence this video.")];
        let value = serde_json::json!({
            "units": [
                { "id": 1, "translation": "因此有了这期视频。",
                  "slots": [ { "id": 101, "text": "因此" },
                             { "id": 102, "text": "有了这期视频。" } ] }
            ]
        });
        let out = validate_sentence_slot_response_with_context(value, &ctx(&units, false))
            .expect("complete slot protocol must pass");
        assert_eq!(out.len(), 1);
        match &out[0].fill {
            UnitFill::Slots(texts) => assert_eq!(
                texts,
                &vec!["因此".to_string(), "有了这期视频。".to_string()]
            ),
            other => panic!("expected slots, got {other:?}"),
        }
    }

    #[test]
    fn rejects_concat_mismatch_with_retryable_feedback() {
        let slots = vec![(101, "a".to_string()), (102, "b".to_string())];
        let units = [unit_expectation(&slots, "a b")];
        let value = serde_json::json!({
            "units": [
                { "id": 1, "translation": "甲乙丙",
                  "slots": [ { "id": 101, "text": "甲" },
                             { "id": 102, "text": "乙" } ] }
            ]
        });
        let err = validate_sentence_slot_response_with_context(value, &ctx(&units, false))
            .expect_err("concat mismatch must fail strict validation");
        let msg = format!("{err:?}");
        assert!(msg.contains("unit 1"), "msg={msg}");
        assert!(msg.contains("reproduce the unit translation"), "msg={msg}");
    }

    #[test]
    fn lenient_mode_accepts_concat_mismatch() {
        let slots = vec![(101, "a".to_string()), (102, "b".to_string())];
        let units = [unit_expectation(&slots, "a b")];
        let value = serde_json::json!({
            "units": [
                { "id": 1, "translation": "甲乙丙",
                  "slots": [ { "id": 101, "text": "甲" },
                             { "id": 102, "text": "乙" } ] }
            ]
        });
        let out = validate_sentence_slot_response_with_context(value, &ctx(&units, true))
            .expect("lenient floor keeps complete slots");
        match &out[0].fill {
            UnitFill::Slots(texts) => assert_eq!(texts.len(), 2),
            other => panic!("expected slots, got {other:?}"),
        }
    }

    #[test]
    fn lenient_mode_falls_back_to_translation_only_when_slots_missing() {
        let slots = vec![(101, "a".to_string()), (102, "b".to_string())];
        let units = [unit_expectation(&slots, "a b")];
        let value = serde_json::json!({
            "units": [
                { "id": 1, "translation": "甲乙丙", "slots": [] }
            ]
        });
        let out = validate_sentence_slot_response_with_context(value, &ctx(&units, true))
            .expect("lenient floor salvages the bare translation");
        match &out[0].fill {
            UnitFill::TranslationOnly(text) => assert_eq!(text, "甲乙丙"),
            other => panic!("expected translation-only, got {other:?}"),
        }
    }

    #[test]
    fn strict_mode_rejects_missing_unit_and_missing_slot() {
        let slots = vec![(101, "a".to_string()), (102, "b".to_string())];
        let units = [
            unit_expectation(&slots, "a b"),
            unit_expectation(&slots, "c d"),
        ];
        let value = serde_json::json!({
            "units": [
                { "id": 1, "translation": "甲乙",
                  "slots": [ { "id": 101, "text": "甲" } ] }
            ]
        });
        let err = validate_sentence_slot_response_with_context(value, &ctx(&units, false))
            .expect_err("missing unit and missing slot must fail");
        let msg = format!("{err:?}");
        assert!(msg.contains("missing unit ids [2]"), "msg={msg}");
        assert!(msg.contains("incomplete"), "msg={msg}");
    }
}
