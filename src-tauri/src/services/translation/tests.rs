use super::batches::build_batch_windows;
use super::responses::validate_batch_translation_response;
use super::segments::{merge_dangling_source_segments, normalize_segments};
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

fn timed_seg(index: usize, text: &str) -> TranslationSegmentInput {
    TranslationSegmentInput {
        segment: text.to_string(),
        start: index as f64,
        end: index as f64 + 1.0,
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
fn validate_batch_translation_response_rejects_missing_expected_id() {
    let value = json!({
        "translations": [
            { "id": 1, "text": "first" }
        ]
    });

    let err =
        validate_batch_translation_response(value, &[1, 2]).expect_err("should reject missing id");
    assert!(format!("{err:?}").contains("missing translation id 2"));
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
    assert!(format!("{err:?}").contains("translation id 1 must be non-empty"));
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
fn merge_dangling_source_segments_keeps_translation_units_semantic() {
    let normalized = normalize_segments(&[
        timed_seg(
            0,
            "It's something I've been trying to do every week just to get a good idea of how I'm performing",
        ),
        timed_seg(1, "against the reference list of literally reviewing a"),
        timed_seg(2, "high quality example that I see"),
        timed_seg(3, "because sometimes your execution slips, you"),
        timed_seg(
            4,
            "might skip every high quality example due to hesitation or maybe you choose weaker examples because you're not thinking straight.",
        ),
        timed_seg(
            5,
            "And it's also just a good exercise to rebuild belief in the system.",
        ),
        timed_seg(
            6,
            "If maybe you had a bad week and you're like, oh, this doesn't work anymore,",
        ),
        timed_seg(
            7,
            "but you can just actually look back at all the proper examples and see how it actually would have performed.",
        ),
    ]);

    let merged = merge_dangling_source_segments(normalized);
    let sources = merged
        .iter()
        .map(|segment| segment.source.as_str())
        .collect::<Vec<_>>();

    assert!(sources.contains(
        &"against the reference list of literally reviewing a high quality example that I see"
    ));
    assert!(sources.contains(&"because sometimes your execution slips, you might skip every high quality example due to hesitation or maybe you choose weaker examples because you're not thinking straight."));
    assert!(sources.contains(&"If maybe you had a bad week and you're like, oh, this doesn't work anymore, but you can just actually look back at all the proper examples and see how it actually would have performed."));
    assert_eq!(merged.len(), 5);
}
