use super::batches::build_batch_windows;
use super::responses::{
    SlotUnitExpectation, SlotValidationContext, UnitFill,
    validate_sentence_slot_response_with_context,
};
use super::segments::normalize_segments;
use super::slots::{norm_core, proportional_split, repair_slot_boundaries, slot_texts_in_order};
use super::{TranslationSegmentInput, TranslationTerminologyEntry};
use serde_json::json;

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
