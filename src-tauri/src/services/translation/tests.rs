use super::batches::build_batch_windows;
use super::responses::validate_batch_translation_response;
use super::segments::normalize_segments;
use super::{TranslationSegmentInput, TranslationTerminologyEntry};
use serde_json::json;

fn seg(text: &str) -> TranslationSegmentInput {
    TranslationSegmentInput {
        segment: text.to_string(),
        start: 0.0,
        end: 1.0,
        tokens: Vec::new(),
    }
}

#[test]
fn split_batches_respects_requested_size() {
    let normalized = normalize_segments(&[seg("a"), seg("b"), seg("c"), seg("d"), seg("e")]);
    let windows = build_batch_windows(
        &normalized,
        2,
        "en",
        "zh-CN",
        "theme",
        &Vec::<TranslationTerminologyEntry>::new(),
    );
    assert_eq!(windows.len(), 3);
    assert_eq!(windows[0].local_ids, vec![1, 2]);
    assert_eq!(windows[1].local_ids, vec![1, 2]);
    assert_eq!(windows[2].local_ids, vec![1]);
    assert_eq!(windows[0].local_to_global, vec![1, 2]);
    assert_eq!(windows[1].local_to_global, vec![3, 4]);
    assert_eq!(windows[2].local_to_global, vec![5]);
}

#[test]
fn context_windows_are_prev_3_next_2() {
    let inputs: Vec<_> = (0..10).map(|i| seg(&format!("L{i}"))).collect();
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
    assert_eq!(prev, vec!["L1", "L2", "L3"]);
    assert_eq!(next, vec!["L6", "L7"]);
}


#[test]
fn bilingual_context_renders_known_translations_in_prompt() {
    use std::collections::HashMap;
    let normalized = normalize_segments(&[seg("a"), seg("b"), seg("c"), seg("d"), seg("e")]);
    let windows = build_batch_windows(
        &normalized,
        2,
        "en",
        "zh-CN",
        "",
        &Vec::<TranslationTerminologyEntry>::new(),
    );
    // Window 1 covers c,d (global ids 3,4); prev = a,b (ids 1,2); next = e (id 5).
    let mut known = HashMap::<usize, String>::new();
    known.insert(1, "甲".to_string());
    known.insert(2, "乙".to_string());
    let prompt = windows[1].build_prompt(&known, &[], &[]);
    // Known translations render bilingually...
    assert!(prompt.contains("a → 甲"), "prompt={prompt}");
    assert!(prompt.contains("b → 乙"), "prompt={prompt}");
    // ...unknown lines stay source-only, and current lines are plain text.
    assert!(prompt.contains("\"e\""), "prompt={prompt}");
    assert!(prompt.contains("\"c\""), "prompt={prompt}");
    assert!(!prompt.contains("d → "), "prompt={prompt}");
    // nextLines stay source-only even when a translation is already known
    // (resume). They are context, not an answer key.
    known.insert(5, "戊".to_string());
    let resume_prompt = windows[1].build_prompt(&known, &[], &[]);
    assert!(
        !resume_prompt.contains("e → 戊"),
        "nextLines must not be bilingual: {resume_prompt}"
    );
    assert!(resume_prompt.contains("\"e\""), "prompt={resume_prompt}");
    // Without any known translation the context is raw source.
    let empty_prompt = windows[2].build_prompt(&HashMap::new(), &[], &[]);
    assert!(empty_prompt.contains("\"e\""), "empty prompt must keep raw context");
}
#[test]
fn validate_batch_translation_response_rejects_missing_expected_id() {
    let value = json!({
        "translations": [
            { "id": 1, "text": "first" }
        ]
    });

    let err =
        validate_batch_translation_response(value, &[1, 2]).expect_err("should reject missing id");
    let msg = format!("{err:?}");
    assert!(msg.contains("missing ids [2]"), "msg={msg}");
    assert!(msg.contains("got ids [1]"), "msg={msg}");
    assert!(msg.contains("expected 2 items"), "msg={msg}");
}

#[test]
fn validate_batch_translation_response_rejects_empty_translation_text() {
    let value = json!({
        "translations": [
            { "id": 1, "text": "" }
        ]
    });

    let err = validate_batch_translation_response(value, &[1])
        .expect_err("should reject empty translation");
    let msg = format!("{err:?}");
    assert!(msg.contains("empty ids [1]"), "msg={msg}");
    assert!(msg.contains("got ids []"), "msg={msg}");
}

#[test]
fn validate_batch_translation_response_aggregates_multiple_issues() {
    let value = json!({
        "translations": [
            { "id": 1, "text": "ok" },
            { "id": 2, "text": "" },
            // Conflicting second non-empty after a good first would be duplicate;
            // here id 2 recovers from empty→non-empty (see dedicated test).
            { "id": 2, "text": "two" },
            { "id": 4, "text": "four" },
            { "id": 4, "text": "four-again" },
            { "id": 99, "text": "noise" }
        ]
    });

    let err = validate_batch_translation_response(value, &[1, 2, 3, 4, 5])
        .expect_err("should aggregate issues");
    let msg = format!("{err:?}");
    // One message reports every problem so a single retry can fix the batch.
    assert!(msg.contains("missing ids [3,5]"), "msg={msg}");
    assert!(!msg.contains("empty ids"), "id 2 should recover; msg={msg}");
    assert!(msg.contains("duplicate ids [4]"), "msg={msg}");
    assert!(msg.contains("unexpected ids [99]"), "msg={msg}");
    assert!(msg.contains("got ids [1,2,4]"), "msg={msg}");
    assert!(msg.contains("expected 5 items"), "msg={msg}");
}

#[test]
fn validate_batch_translation_response_prefers_non_empty_over_earlier_empty() {
    let value = json!({
        "translations": [
            { "id": 1, "text": "" },
            { "id": 1, "text": "recovered" },
            { "id": 2, "text": "second" }
        ]
    });

    let out = validate_batch_translation_response(value, &[1, 2])
        .expect("empty then non-empty should recover");
    assert_eq!(out.get(&1).map(String::as_str), Some("recovered"));
    assert_eq!(out.get(&2).map(String::as_str), Some("second"));
}

#[test]
fn validate_batch_translation_response_keeps_first_non_empty_on_conflict() {
    let value = json!({
        "translations": [
            { "id": 1, "text": "first" },
            { "id": 1, "text": "second" }
        ]
    });

    let err = validate_batch_translation_response(value, &[1])
        .expect_err("two non-empty values are a conflict");
    let msg = format!("{err:?}");
    assert!(msg.contains("duplicate ids [1]"), "msg={msg}");
    assert!(msg.contains("got ids [1]"), "msg={msg}");
}

#[test]
fn validate_batch_translation_response_ignores_unexpected_when_complete() {
    let value = json!({
        "translations": [
            { "id": 1, "text": "first" },
            { "id": 2, "text": "second" },
            { "id": 99, "text": "extra" }
        ]
    });

    let out = validate_batch_translation_response(value, &[1, 2]).expect("extras are non-fatal");
    assert_eq!(out.get(&1).map(String::as_str), Some("first"));
    assert_eq!(out.get(&2).map(String::as_str), Some("second"));
    assert!(!out.contains_key(&99));
}

#[test]
fn validate_batch_translation_response_accepts_complete_non_empty_batch() {
    let value = json!({
        "translations": [
            { "id": 1, "text": "first" },
            { "id": 2, "text": "second" }
        ]
    });

    let out = validate_batch_translation_response(value, &[1, 2]).expect("should parse full batch");
    assert_eq!(out.get(&1).map(String::as_str), Some("first"));
    assert_eq!(out.get(&2).map(String::as_str), Some("second"));
}

#[test]
fn validate_batch_translation_response_finds_ids_inside_any_envelope() {
    let value = json!({
        "data": {
            "items": [
                { "id": "1", "text": "第一句" },
                { "id": 2, "translatedText": "第二句" }
            ]
        }
    });
    let out = validate_batch_translation_response(value, &[1, 2])
        .expect("id/text pairs must be found regardless of wrapper keys");
    assert_eq!(out.get(&1).map(String::as_str), Some("第一句"));
    assert_eq!(out.get(&2).map(String::as_str), Some("第二句"));
}

#[test]
fn validate_batch_translation_response_accepts_root_array() {
    let value = json!([
        { "id": 1, "text": "第一句" },
        { "id": 2, "text": "第二句" }
    ]);
    let out = validate_batch_translation_response(value, &[1, 2]).expect("root array");
    assert_eq!(out.get(&1).map(String::as_str), Some("第一句"));
    assert_eq!(out.get(&2).map(String::as_str), Some("第二句"));
}

#[test]
fn validate_batch_translation_response_accepts_map_string_values() {
    let value = json!({
        "1": "first",
        "2": "second"
    });
    let out = validate_batch_translation_response(value, &[1, 2]).expect("map strings ok");
    assert_eq!(out.get(&1).map(String::as_str), Some("first"));
    assert_eq!(out.get(&2).map(String::as_str), Some("second"));
}

#[test]
fn validate_rejects_source_language_paraphrase() {
    use super::responses::{
        TranslationValidationContext, validate_batch_translation_response_with_context,
    };
    let value = json!({
        "translations": [
            { "id": 1, "text": "過去にはいろいろな悩みを抱えてきました" }
        ]
    });
    let err = validate_batch_translation_response_with_context(
        value,
        &TranslationValidationContext {
            expected_ids: &[1],
            source_lang: "ja",
            target_lang: "zh-CN",
            enforced_targets: &[],
            source_texts: &[],
        },
    )
    .expect_err("source-language paraphrase must fail");
    let msg = format!("{err:?}");
    assert!(msg.contains("source-language leak"), "msg={msg}");
}

#[test]
fn validate_accepts_enforced_target_with_source_script() {
    use super::responses::{
        TranslationValidationContext, validate_batch_translation_response_with_context,
    };
    // The term target is enforced verbatim; its kana must not count as a leak.
    let value = json!({
        "translations": [
            { "id": 1, "text": "只能去Last Call（ラストコール）了啊，去Last Call（ラストコール）吧" }
        ]
    });
    let out = validate_batch_translation_response_with_context(
        value,
        &TranslationValidationContext {
            expected_ids: &[1],
            source_lang: "ja",
            target_lang: "zh-CN",
            enforced_targets: &["Last Call（ラストコール）".to_string()],
            source_texts: &[],
        },
    )
    .expect("enforced kana-bearing target must not trip the leak guard");
    assert_eq!(out.len(), 1);
}

#[test]
fn validate_rejects_adjacent_duplicate_translations_over_different_sources() {
    use super::responses::{
        TranslationValidationContext, validate_batch_translation_response_with_context,
    };
    // Merge/shift signature: id 3's translation is a copy of id 2's while the
    // sources differ — the model merged a line and padded the id list.
    let value = json!({
        "translations": [
            { "id": 1, "text": "第一句的译文。" },
            { "id": 2, "text": "我们在预期季度转换时会关注未平仓合约量" },
            { "id": 3, "text": "我们在预期季度转换时会关注未平仓合约量" }
        ]
    });
    let sources = vec![
        "first source line.".to_string(),
        "We look at open interest when anticipating a quarterly shift.".to_string(),
        "This is what we're doing in price action, right?".to_string(),
    ];
    let err = validate_batch_translation_response_with_context(
        value,
        &TranslationValidationContext {
            expected_ids: &[1, 2, 3],
            source_lang: "en",
            target_lang: "zh-CN",
            enforced_targets: &[],
            source_texts: &sources,
        },
    )
    .expect_err("adjacent duplicate over different sources must fail");
    let msg = format!("{err:?}");
    assert!(msg.contains("adjacent duplicate translations"), "msg={msg}");
}

#[test]
fn validate_allows_adjacent_duplicates_when_source_repeats_or_text_is_short() {
    use super::responses::{
        TranslationValidationContext, validate_batch_translation_response_with_context,
    };
    // Same source twice ("Okay?" / "Okay?") -> same translation is legitimate.
    let value = json!({
        "translations": [
            { "id": 1, "text": "市场在这一段里反复震荡整理中" },
            { "id": 2, "text": "市场在这一段里反复震荡整理中" },
            { "id": 3, "text": "对吧？" },
            { "id": 4, "text": "对吧？" }
        ]
    });
    let sources = vec![
        "The market keeps ranging here, right?".to_string(),
        "The market keeps ranging here, right?".to_string(),
        "Okay?".to_string(),
        "Right?".to_string(),
    ];
    let out = validate_batch_translation_response_with_context(
        value,
        &TranslationValidationContext {
            expected_ids: &[1, 2, 3, 4],
            source_lang: "en",
            target_lang: "zh-CN",
            enforced_targets: &[],
            source_texts: &sources,
        },
    )
    .expect("repeated source lines and short interjections must pass");
    assert_eq!(out.len(), 4);
}

#[test]
fn merge_local_to_global_ignores_zero_and_out_of_range_ids() {
    use super::merge_local_to_global;
    let local_to_global = vec![10, 11, 12];
    let mut skipped = Vec::new();
    let merged = merge_local_to_global(
        &local_to_global,
        vec![
            (0, "zero".to_string()),
            (1, "first".to_string()),
            (4, "past".to_string()),
        ],
        &mut |id| skipped.push(id),
    );
    // id 0 previously aliased the first line (saturating_sub); id 4 the last.
    assert_eq!(merged.len(), 1);
    assert_eq!(merged.get(&10).map(String::as_str), Some("first"));
    assert_eq!(skipped, vec![0, 4]);
}
