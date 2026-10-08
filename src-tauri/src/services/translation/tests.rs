use super::batches::{build_batch_windows, split_chunk_size, split_window};
use super::responses::{
    SlotUnitExpectation, SlotValidationContext, UnitFill,
    validate_sentence_slot_response_with_context,
};
use super::segments::normalize_segments;
use super::slots::{norm_core, proportional_split, repair_slot_boundaries, slot_texts_in_order};
use super::{TranslationSegmentInput, TranslationTerminologyEntry};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use serde_json::{Value, json};

fn seg(text: &str) -> TranslationSegmentInput {
    TranslationSegmentInput {
        segment: text.to_string(),
        sentence_id: 0,
        start: 0.0,
        end: 1.0,
        tokens: Vec::new(),
    }
}

fn seg_in(text: &str, sentence_id: usize) -> TranslationSegmentInput {
    TranslationSegmentInput {
        segment: text.to_string(),
        sentence_id,
        start: 0.0,
        end: 1.0,
        tokens: Vec::new(),
    }
}

#[test]
fn split_batches_respects_requested_size() {
    let normalized = normalize_segments(&[
        seg("One."),
        seg("Two."),
        seg("Three."),
        seg("Four."),
        seg("Five."),
    ]);
    let windows = build_batch_windows(
        &normalized,
        2,
        "en",
        "zh-CN",
        "theme",
        &Vec::<TranslationTerminologyEntry>::new(),
    );
    // No sentence grouping: every segment is its own unit.
    assert_eq!(windows.len(), 3);
    assert_eq!(windows[0].local_to_global, vec![1, 2]);
    assert_eq!(windows[1].local_to_global, vec![3, 4]);
    assert_eq!(windows[2].local_to_global, vec![5]);
    // Every unit carries exactly its own display slot.
    assert_eq!(windows[0].unit_slots.as_ref(), &[vec![(1, "One.".to_string())], vec![(2, "Two.".to_string())]]);
}

#[test]
fn same_sentence_segments_group_into_one_unit_with_slots() {
    let normalized = normalize_segments(&[
        seg_in("Hence", 7),
        seg_in("this video.", 7),
        seg_in("Next one.", 8),
        seg_in("Another cue.", 8),
    ]);
    let windows = build_batch_windows(
        &normalized,
        8,
        "en",
        "zh-CN",
        "",
        &Vec::<TranslationTerminologyEntry>::new(),
    );
    // Two units: sentence 7 (slots 1,2) and sentence 8 (slots 3,4).
    assert_eq!(windows.len(), 1);
    assert_eq!(windows[0].local_to_global, vec![1, 2]);
    assert_eq!(
        windows[0].unit_slots.as_ref(),
        &[
            vec![(1, "Hence".to_string()), (2, "this video.".to_string())],
            vec![(3, "Next one.".to_string()), (4, "Another cue.".to_string())],
        ]
    );
    // The unit source is the slot texts joined with language-aware spacing.
    assert_eq!(windows[0].current_lines[0].text, "Hence this video.");
    assert_eq!(windows[0].current_lines[1].text, "Next one. Another cue.");
}

#[test]
fn context_windows_are_prev_3_next_2() {
    let inputs: Vec<_> = (0..10).map(|i| seg(&format!("L{i}."))).collect();
    let normalized = normalize_segments(&inputs);
    let windows = build_batch_windows(
        &normalized,
        2,
        "en",
        "zh-CN",
        "",
        &Vec::<TranslationTerminologyEntry>::new(),
    );
    let window = &windows[2];
    assert_eq!(window.local_to_global, vec![5, 6]);
    let prev: Vec<&str> = window.prev_lines.iter().map(|(_, t)| t.as_str()).collect();
    let next: Vec<&str> = window.next_lines.iter().map(|(_, t)| t.as_str()).collect();
    assert_eq!(prev, vec!["L1.", "L2.", "L3."]);
    assert_eq!(next, vec!["L6.", "L7."]);
}

#[test]
fn bilingual_context_renders_known_unit_translations_in_prompt() {
    use std::collections::HashMap;
    let normalized = normalize_segments(&[
        seg("One."),
        seg("Two."),
        seg("Three."),
        seg("Four."),
        seg("Five."),
    ]);
    let windows = build_batch_windows(
        &normalized,
        2,
        "en",
        "zh-CN",
        "",
        &Vec::<TranslationTerminologyEntry>::new(),
    );
    // Window 1 covers units Three.,Four. (global unit ids 3,4); prev = units 1,2; next = unit 5.
    let mut known = HashMap::<usize, String>::new();
    known.insert(1, "甲".to_string());
    known.insert(2, "乙".to_string());
    let prompt = windows[1].build_prompt(&known, &[], &[]);
    // Known unit translations render bilingually...
    assert!(prompt.contains("One. → 甲"), "prompt={prompt}");
    assert!(prompt.contains("Two. → 乙"), "prompt={prompt}");
    // ...unknown units stay source-only, and current units carry slots.
    assert!(prompt.contains("\"Five.\""), "prompt={prompt}");
    assert!(prompt.contains("\"Three.\""), "prompt={prompt}");
    assert!(!prompt.contains("Four. → "), "prompt={prompt}");
    // nextLines stay source-only even when a translation is already known
    // (resume). They are context, not an answer key.
    known.insert(5, "戊".to_string());
    let resume_prompt = windows[1].build_prompt(&known, &[], &[]);
    assert!(
        !resume_prompt.contains("Five. → 戊"),
        "nextLines must not be bilingual: {resume_prompt}"
    );
    // The currentUnits payload carries display slots with global segment ids.
    assert!(prompt.contains("\"slots\""), "prompt={prompt}");
    assert!(prompt.contains("\"sourceText\"") || prompt.contains("\"text\""), "prompt={prompt}");
}

#[test]
fn slot_validator_accepts_complete_protocol_and_orders_slots() {
    let slots = vec![
        (1usize, "Hence".to_string()),
        (2usize, "this video.".to_string()),
    ];
    let units = [SlotUnitExpectation {
        slots: &slots,
        source: "Hence this video.",
    }];
    let value = json!({
        "units": [
            { "id": 1, "translation": "因此有了这期视频。",
              "slots": [
                  { "id": 2, "text": "有了这期视频。" },
                  { "id": 1, "text": "因此" }
              ] }
        ]
    });
    let out = validate_sentence_slot_response_with_context(
        value,
        &SlotValidationContext {
            units: &units,
            source_lang: "en",
            target_lang: "zh-CN",
            enforced_targets: &[],
            lenient: false,
        },
    )
    .expect("slot protocol complete (order is normalized, not trusted)");
    match &out[0].fill {
        UnitFill::Slots(texts) => {
            // Expected slot order, not the model's emission order.
            assert_eq!(texts[0], "因此");
            assert_eq!(texts[1], "有了这期视频。");
        }
        other => panic!("expected slots, got {other:?}"),
    }
}

#[test]
fn slot_validator_rejects_missing_unit_and_reports_expected_count() {
    let slots = vec![(1usize, "a".to_string())];
    let units = [
        SlotUnitExpectation { slots: &slots, source: "a" },
        SlotUnitExpectation { slots: &slots, source: "b" },
    ];
    let value = json!({
        "units": [
            { "id": 1, "translation": "甲", "slots": [ { "id": 1, "text": "甲" } ] }
        ]
    });
    let err = validate_sentence_slot_response_with_context(
        value,
        &SlotValidationContext {
            units: &units,
            source_lang: "en",
            target_lang: "zh-CN",
            enforced_targets: &[],
            lenient: false,
        },
    )
    .expect_err("missing unit must fail");
    let msg = format!("{err:?}");
    assert!(msg.contains("missing unit ids [2]"), "msg={msg}");
    assert!(msg.contains("expected 2 units"), "msg={msg}");
}

#[test]
fn slot_validator_rejects_slot_concat_mismatch_strictly() {
    let slots = vec![(1usize, "a".to_string()), (2usize, "b".to_string())];
    let units = [SlotUnitExpectation { slots: &slots, source: "a b" }];
    let value = json!({
        "units": [
            { "id": 1, "translation": "甲乙丙",
              "slots": [ { "id": 1, "text": "甲" }, { "id": 2, "text": "乙" } ] }
        ]
    });
    let err = validate_sentence_slot_response_with_context(
        value,
        &SlotValidationContext {
            units: &units,
            source_lang: "en",
            target_lang: "zh-CN",
            enforced_targets: &[],
            lenient: false,
        },
    )
    .expect_err("concat mismatch must fail strict validation");
    let msg = format!("{err:?}");
    assert!(msg.contains("reproduce the unit translation"), "msg={msg}");
}

#[test]
fn slot_validator_lenient_mode_salvages_at_ladder_floor() {
    let slots = vec![(1usize, "a".to_string()), (2usize, "b".to_string())];
    let units = [SlotUnitExpectation { slots: &slots, source: "a b" }];
    // Slots incomplete but translation present → TranslationOnly.
    let value = json!({
        "units": [
            { "id": 1, "translation": "甲乙丙", "slots": [ { "id": 1, "text": "甲" } ] }
        ]
    });
    let out = validate_sentence_slot_response_with_context(
        value,
        &SlotValidationContext {
            units: &units,
            source_lang: "en",
            target_lang: "zh-CN",
            enforced_targets: &[],
            lenient: true,
        },
    )
    .expect("lenient floor salvages the bare translation");
    match &out[0].fill {
        UnitFill::TranslationOnly(text) => assert_eq!(text, "甲乙丙"),
        other => panic!("expected translation-only, got {other:?}"),
    }
    // Complete slots with a concat mismatch → soft-kept in lenient mode.
    let value = json!({
        "units": [
            { "id": 1, "translation": "甲乙丙",
              "slots": [ { "id": 1, "text": "甲" }, { "id": 2, "text": "乙" } ] }
        ]
    });
    let out = validate_sentence_slot_response_with_context(
        value,
        &SlotValidationContext {
            units: &units,
            source_lang: "en",
            target_lang: "zh-CN",
            enforced_targets: &[],
            lenient: true,
        },
    )
    .expect("lenient floor keeps complete slots");
    match &out[0].fill {
        UnitFill::Slots(texts) => assert_eq!(texts.len(), 2),
        other => panic!("expected slots, got {other:?}"),
    }
}

#[test]
fn slot_validator_still_rejects_source_language_paraphrase() {
    let slots = vec![(1usize, "a".to_string())];
    let units = [SlotUnitExpectation { slots: &slots, source: "a" }];
    let value = json!({
        "units": [
            { "id": 1, "translation": "過去にはいろいろな悩みを抱えてきました",
              "slots": [ { "id": 1, "text": "過去にはいろいろな悩みを抱えてきました" } ] }
        ]
    });
    let err = validate_sentence_slot_response_with_context(
        value,
        &SlotValidationContext {
            units: &units,
            source_lang: "ja",
            target_lang: "zh-CN",
            enforced_targets: &[],
            lenient: false,
        },
    )
    .expect_err("source-language paraphrase must fail");
    let msg = format!("{err:?}");
    assert!(msg.contains("source-language leak"), "msg={msg}");
}

#[test]
fn proportional_split_covers_all_slots() {
    let out = proportional_split("因此有了这期视频。", &[5, 5]);
    assert_eq!(out.join(""), "因此有了这期视频。");
    assert!(out.iter().all(|t| !t.is_empty()));
}

#[test]
fn slot_texts_in_order_normalizes_model_slot_order() {
    let expected = vec![(1, "a".to_string()), (2, "b".to_string())];
    let got = vec![(2, "乙".to_string()), (1, "甲".to_string())];
    let ordered = slot_texts_in_order(&expected, &got).expect("all slots present");
    assert_eq!(ordered, vec!["甲".to_string(), "乙".to_string()]);
}

#[test]
fn boundary_repair_snaps_leading_punctuation_and_keeps_concat() {
    let mut texts = vec!["用2倍速一口气刷完".to_string(), "、却不真正体验".to_string()];
    let before = norm_core(&texts.concat());
    repair_slot_boundaries(&mut texts, "zh-CN");
    assert_eq!(texts, vec!["用2倍速一口气刷完、", "却不真正体验"]);
    assert_eq!(norm_core(&texts.concat()), before);
}

#[test]
fn norm_core_is_punctuation_and_case_insensitive() {
    assert_eq!(norm_core("因此，有了这期视频。"), norm_core("因此有了这期视频"));
    assert_eq!(norm_core("Ｈｅｌｌｏ，　Ｗｏｒｌｄ！"), norm_core("hello world"));
}

// ---------------------------------------------------------------------------
// End-to-end sample from a real task (ep6 slot pilot, 49-minute episode).
// The unit grouping, the slot texts and the accept gate are pinned against
// data a real run produced, not against hand-written examples.
// ---------------------------------------------------------------------------

const EP6_SAMPLE: &str = include_str!("testdata/ep6_slot_units.json");

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct SampleFixture {
    units: Vec<SampleUnit>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct SampleUnit {
    sentence_id: usize,
    unit_translation: String,
    slots: Vec<SampleSlot>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct SampleSlot {
    segment: String,
    translation: String,
}

struct Sample {
    segments: Vec<TranslationSegmentInput>,
    /// Per unit: (global slot id, cue source) in display order.
    slot_lists: Vec<Vec<(usize, String)>>,
    /// Per unit: the recorded slot translations, aligned with `slot_lists`.
    slot_texts: Vec<Vec<String>>,
    /// Per unit: the recorded whole-sentence translation.
    translations: Vec<String>,
    /// Per unit: the concatenated source cues (what the model saw as one unit).
    sources: Vec<String>,
}

impl Sample {
    fn load() -> Self {
        let fixture: SampleFixture = serde_json::from_str(EP6_SAMPLE).expect("fixture parses");
        let mut segments = Vec::new();
        let mut slot_lists = Vec::new();
        let mut slot_texts = Vec::new();
        let mut translations = Vec::new();
        let mut sources = Vec::new();
        let mut next_slot_id = 1usize;
        for unit in &fixture.units {
            let mut slots = Vec::new();
            let mut texts = Vec::new();
            let mut source = String::new();
            for slot in &unit.slots {
                segments.push(TranslationSegmentInput {
                    segment: slot.segment.clone(),
                    sentence_id: unit.sentence_id,
                    start: 0.0,
                    end: 1.0,
                    tokens: Vec::new(),
                });
                slots.push((next_slot_id, slot.segment.clone()));
                texts.push(slot.translation.clone());
                source.push_str(&slot.segment);
                next_slot_id += 1;
            }
            slot_lists.push(slots);
            slot_texts.push(texts);
            translations.push(unit.unit_translation.clone());
            sources.push(source);
        }
        Self {
            segments,
            slot_lists,
            slot_texts,
            translations,
            sources,
        }
    }

    fn expectations(&self) -> Vec<SlotUnitExpectation<'_>> {
        self.slot_lists
            .iter()
            .zip(self.sources.iter())
            .map(|(slots, source)| SlotUnitExpectation {
                slots,
                source: source.as_str(),
            })
            .collect()
    }

    /// The recorded run's answer, as the model would have returned it for a
    /// single window holding every unit (unit ids are local, 1-based).
    fn response(&self) -> serde_json::Value {
        let units: Vec<serde_json::Value> = self
            .slot_lists
            .iter()
            .zip(self.slot_texts.iter())
            .zip(self.translations.iter())
            .enumerate()
            .map(|(index, ((slots, texts), translation))| {
                let slots: Vec<serde_json::Value> = slots
                    .iter()
                    .map(|(id, _)| *id)
                    .zip(texts.iter())
                    .map(|(id, text)| json!({ "id": id, "text": text }))
                    .collect();
                json!({ "id": index + 1, "translation": translation, "slots": slots })
            })
            .collect();
        json!({ "units": units })
    }
}

fn validate_sample(
    sample: &Sample,
    value: serde_json::Value,
    lenient: bool,
) -> Result<Vec<super::responses::ValidatedUnit>, crate::services::llm::client::LlmSemanticValidationError> {
    let expectations = sample.expectations();
    validate_sentence_slot_response_with_context(
        value,
        &SlotValidationContext {
            units: &expectations,
            source_lang: "en",
            target_lang: "zh-CN",
            enforced_targets: &[],
            lenient,
        },
    )
}

#[test]
fn ep6_sample_groups_cues_into_sentence_units_with_global_slot_ids() {
    let sample = Sample::load();
    let normalized = normalize_segments(&sample.segments);
    let windows = build_batch_windows(
        &normalized,
        8,
        "en",
        "zh-CN",
        "",
        &Vec::<TranslationTerminologyEntry>::new(),
    );
    // 13 cues, 5 sentence units: one batch, and the cue count per unit is the
    // pilot's real distribution (3/2/5/2/1).
    assert_eq!(windows.len(), 1);
    let window = &windows[0];
    assert_eq!(window.local_to_global, vec![1, 2, 3, 4, 5]);
    let slot_counts: Vec<usize> = window.unit_slots.iter().map(|slots| slots.len()).collect();
    assert_eq!(slot_counts, vec![3, 2, 5, 2, 1]);
    // Slot ids are the global cue ids, in display order, and each unit owns
    // exactly its own cues (no duplication, no gaps).
    let ids: Vec<usize> = window
        .unit_slots
        .iter()
        .flatten()
        .map(|(id, _)| *id)
        .collect();
    assert_eq!(ids, (1..=13).collect::<Vec<_>>());
}

#[test]
fn ep6_sample_recorded_response_passes_the_strict_gate() {
    let sample = Sample::load();
    let units = validate_sample(&sample, sample.response(), false).expect("recorded answer accepted");
    assert_eq!(units.len(), sample.translations.len());
    for (index, unit) in units.iter().enumerate() {
        match &unit.fill {
            UnitFill::Slots(texts) => assert_eq!(
                norm_core(&texts.concat()),
                norm_core(&sample.translations[index]),
                "unit {} must concat back to its sentence translation",
                index + 1
            ),
            other => panic!("unit {} expected slots, got {other:?}", index + 1),
        }
    }
}

#[test]
fn strict_gate_rejects_a_dropped_slot_and_the_floor_salvages_it() {
    let sample = Sample::load();
    let mut value = sample.response();
    let _ = value["units"][0]["slots"].as_array_mut().unwrap().pop();

    let err = validate_sample(&sample, value.clone(), false).expect_err("strict must bounce it");
    let msg = format!("{err:?}");
    assert!(msg.contains("displaySlots incomplete"), "msg={msg}");

    let salvaged = validate_sample(&sample, value, true).expect("floor salvages the unit");
    match &salvaged[0].fill {
        UnitFill::TranslationOnly(text) => {
            assert_eq!(norm_core(text), norm_core(&sample.translations[0]));
        }
        other => panic!("expected translation-only at the floor, got {other:?}"),
    }
}

#[test]
fn strict_gate_rejects_bare_punctuation_on_a_content_bearing_slot() {
    let sample = Sample::load();
    let mut value = sample.response();
    // The real defect shape: the model front-loads a slot's semantics into the
    // next slot and parks punctuation on the slot it emptied. The concat still
    // reproduces the translation, so only the strict content invariant sees it.
    let merged = format!("{}{}", sample.slot_texts[0][1], sample.slot_texts[0][2]);
    value["units"][0]["slots"][2]["text"] = json!(merged);
    value["units"][0]["slots"][1]["text"] = json!("，");

    let err = validate_sample(&sample, value.clone(), false).expect_err("strict must bounce it");
    let msg = format!("{err:?}");
    assert!(msg.contains("bare punctuation"), "msg={msg}");

    let lenient = validate_sample(&sample, value, true).expect("lenient keeps the slots");
    match &lenient[0].fill {
        UnitFill::Slots(texts) => assert_eq!(texts[1], "，"),
        other => panic!("expected slots in lenient mode, got {other:?}"),
    }
}

#[test]
fn strict_gate_rejects_concat_mismatch_but_lenient_keeps_the_slots() {
    let sample = Sample::load();
    let mut value = sample.response();
    value["units"][1]["translation"] = json!(format!("{}哦", sample.translations[1]));

    let err = validate_sample(&sample, value.clone(), false).expect_err("strict must bounce it");
    let msg = format!("{err:?}");
    assert!(msg.contains("reproduce the unit translation"), "msg={msg}");

    let lenient = validate_sample(&sample, value, true).expect("lenient keeps complete slots");
    match &lenient[1].fill {
        UnitFill::Slots(texts) => assert_eq!(texts.concat(), sample.slot_texts[1].concat()),
        other => panic!("expected slots in lenient mode, got {other:?}"),
    }
}

#[test]
fn ladder_splits_eight_units_to_one_without_losing_global_ids() {
    let segments: Vec<TranslationSegmentInput> = (1..=8)
        .map(|i| seg_in(&format!("Sentence {i}."), i))
        .collect();
    let normalized = normalize_segments(&segments);
    let windows = build_batch_windows(
        &normalized,
        8,
        "en",
        "zh-CN",
        "",
        &Vec::<TranslationTerminologyEntry>::new(),
    );
    assert_eq!(windows.len(), 1);
    let full = windows.into_iter().next().unwrap();
    assert_eq!(full.local_to_global, (1..=8).collect::<Vec<_>>());

    // The ladder is 8 → 4 → 2 → 1, then the lenient floor (no further split).
    assert_eq!(split_chunk_size(8), Some(4));
    assert_eq!(split_chunk_size(4), Some(2));
    assert_eq!(split_chunk_size(2), Some(1));
    assert_eq!(split_chunk_size(1), None);

    let halves = split_window(&full, 4);
    assert_eq!(halves.len(), 2);
    assert_eq!(halves[0].local_to_global, vec![1, 2, 3, 4]);
    assert_eq!(halves[1].local_to_global, vec![5, 6, 7, 8]);
    // Context is recomputed per part: the tail keeps the three units before it.
    assert!(halves[0].prev_lines.is_empty());
    let first_next: Vec<usize> = halves[0].next_lines.iter().map(|(id, _)| *id).collect();
    assert_eq!(first_next, vec![5, 6]);
    let second_prev: Vec<usize> = halves[1].prev_lines.iter().map(|(id, _)| *id).collect();
    assert_eq!(second_prev, vec![2, 3, 4]);
    assert!(halves[1].next_lines.is_empty());
    // Local ids renumber for the model; the global slot id follows its unit.
    assert_eq!(halves[1].current_lines[0].id, 1);
    assert_eq!(
        halves[1].unit_slots[0],
        vec![(5usize, "Sentence 5.".to_string())]
    );

    // Walking the whole ladder must cover every unit exactly once, in order.
    let mut queue: std::collections::VecDeque<_> = halves.into();
    let mut seen: Vec<usize> = Vec::new();
    while let Some(part) = queue.pop_front() {
        match split_chunk_size(part.local_to_global.len()) {
            Some(chunk) => queue.extend(split_window(&part, chunk)),
            None => seen.push(part.local_to_global[0]),
        }
    }
    assert_eq!(seen, (1..=8).collect::<Vec<_>>());
}

// ── injectable LLM seam: end-to-end window / ladder / slot tests ────────────

/// Scripted fake for the `TranslationLlm` seam. It replays a queue of assistant
/// JSON answers by feeding each through the caller's *real* `semantic_validate`
/// closure, so the accept gates, slot backfill and the 8→4→2→1 ladder all run
/// for real — only the network/LLM is faked. An exhausted script returns a
/// retryable error, so split attempts become observable instead of the fake
/// inventing a success the pipeline never earned.
#[derive(Clone)]
struct FakeLlm {
    scripted: Arc<Mutex<VecDeque<Option<Value>>>>,
}

impl FakeLlm {
    fn new(values: Vec<Value>) -> Self {
        Self {
            scripted: Arc::new(Mutex::new(values.into_iter().map(Some).collect())),
        }
    }
}

impl crate::services::llm::client::TranslationLlm for FakeLlm {
    async fn call_json_validated_streaming<T, F>(
        &self,
        _context: &crate::services::llm::port::LlmCallContext,
        _request_id: &str,
        _user_prompt: &str,
        _response_validator: Option<&crate::services::llm::json_guard::JsonResponseValidator>,
        _on_partial: Arc<dyn Fn(String) + Send + Sync>,
        semantic_validate: F,
    ) -> Result<
        crate::services::llm::client::LlmValidatedJsonResult<T>,
        crate::services::llm::error::LlmError,
    >
    where
        F: Fn(
            Value,
        ) -> Result<T, crate::services::llm::client::LlmSemanticValidationError>,
    {
        let next = self
            .scripted
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .pop_front();
        match next {
            Some(Some(value)) => semantic_validate(value)
                .map(|value| crate::services::llm::client::LlmValidatedJsonResult { value })
                .map_err(|crate::services::llm::client::LlmSemanticValidationError::Retryable(msg)| {
                    crate::services::llm::error::LlmError::new(
                        crate::services::llm::error::LlmErrorKind::InvalidSemantic,
                        msg,
                    )
                }),
            _ => Err(crate::services::llm::error::LlmError::new(
                crate::services::llm::error::LlmErrorKind::InvalidSemantic,
                "fake script exhausted",
            )),
        }
    }
}

fn call_context() -> crate::services::llm::port::LlmCallContext {
    crate::services::llm::port::LlmCallContext {
        task_id: "t".into(),
        media_path: None,
        phase: "translate".into(),
        store: None,
    }
}

fn fake_call(fake: FakeLlm, segments: Vec<super::types::NormalizedSegment>) -> super::TranslateCall<FakeLlm> {
    super::TranslateCall {
        llm_client: fake,
        context: call_context(),
        segments: Arc::new(segments),
        known_translations: Arc::new(Mutex::new(HashMap::new())),
        partial_map: Arc::new(Mutex::new(HashMap::new())),
        progress_callback: None,
        completed_batches: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        partial_snapshot_cache: Arc::new(Mutex::new(Vec::new())),
        batch_total: 1,
    }
}

/// The seam really admits a fake, and the deterministic backfill maps the
/// model's per-slot text onto the global segment ids.
#[tokio::test]
async fn translation_pipeline_runs_on_injected_fake_llm() {
    let normalized = normalize_segments(&[seg_in("Hello there", 5), seg_in("my friend", 5)]);
    let windows = build_batch_windows(
        &normalized,
        8,
        "en",
        "zh-CN",
        "",
        &Vec::<TranslationTerminologyEntry>::new(),
    );
    assert_eq!(windows.len(), 1);
    let window = windows.into_iter().next().unwrap();

    let call = fake_call(
        FakeLlm::new(vec![json!({
            "units": [{
                "id": 1,
                "translation": "你好啊，我的朋友",
                "slots": [
                    {"id": 1, "text": "你好啊"},
                    {"id": 2, "text": "我的朋友"}
                ]
            }]
        })]),
        normalized,
    );

    let outcome = super::translate_window_with_split(&call, window, "r1".to_string())
        .await
        .expect("window should translate on the fake");
    assert_eq!(
        outcome.segment_translations.get(&1).map(String::as_str),
        Some("你好啊")
    );
    assert_eq!(
        outcome.segment_translations.get(&2).map(String::as_str),
        Some("我的朋友")
    );
}

/// With an empty script every attempt fails, so the whole window fails rather
/// than fabricating output — proving errors propagate through the seam.
#[tokio::test]
async fn injected_fake_drives_window_failure_path() {
    let normalized = normalize_segments(&[seg("One.")]);
    let windows = build_batch_windows(
        &normalized,
        8,
        "en",
        "zh-CN",
        "",
        &Vec::<TranslationTerminologyEntry>::new(),
    );
    let window = windows.into_iter().next().unwrap();

    let call = fake_call(FakeLlm::new(vec![]), normalized);
    let result = super::translate_window_with_split(&call, window, "r1".to_string()).await;
    assert!(result.is_err(), "an exhausted fake script must fail, not invent text");
}

