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
        None,
        &[],
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
fn validate_batch_translation_response_accepts_map_string_values() {
    let value = json!({
        "1": "first",
        "2": "second"
    });
    let out = validate_batch_translation_response(value, &[1, 2]).expect("map strings ok");
    assert_eq!(out.get(&1).map(String::as_str), Some("first"));
    assert_eq!(out.get(&2).map(String::as_str), Some("second"));
}
