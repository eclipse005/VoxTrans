use super::super::responses::{compact_for_split_match, validate_source_split_response};
use super::super::source_split_boundaries::map_source_parts_to_boundaries;
use super::super::source_split_readability::merge_tiny_ranges_for_readability;
use super::super::source_text::build_source_from_tokens;
use super::super::{
    BuildStep5SourceSplitRequest, Step5DraftSegment, Step5Token,
    build_step_5_1_source_split_with_progress,
};

#[test]
fn step5_source_rebuild_preserves_space_after_inline_punctuation() {
    let tokens = vec![
        Step5Token {
            text: "So".to_string(),
            start: 0.0,
            end: 0.1,
        },
        Step5Token {
            text: "in".to_string(),
            start: 0.1,
            end: 0.2,
        },
        Step5Token {
            text: "this".to_string(),
            start: 0.2,
            end: 0.3,
        },
        Step5Token {
            text: "video,".to_string(),
            start: 0.3,
            end: 0.4,
        },
        Step5Token {
            text: "I'm".to_string(),
            start: 0.4,
            end: 0.5,
        },
        Step5Token {
            text: "here.".to_string(),
            start: 0.5,
            end: 0.6,
        },
    ];

    assert_eq!(
        build_source_from_tokens(&tokens),
        "So in this video, I'm here."
    );
}

#[test]
fn step5_source_rebuild_keeps_attached_decimal_punctuation() {
    let tokens = vec![
        Step5Token {
            text: "It's".to_string(),
            start: 0.0,
            end: 0.1,
        },
        Step5Token {
            text: "only".to_string(),
            start: 0.1,
            end: 0.2,
        },
        Step5Token {
            text: "like".to_string(),
            start: 0.2,
            end: 0.3,
        },
        Step5Token {
            text: "1".to_string(),
            start: 0.3,
            end: 0.4,
        },
        Step5Token {
            text: ".3.".to_string(),
            start: 0.4,
            end: 0.5,
        },
    ];

    assert_eq!(build_source_from_tokens(&tokens), "It's only like 1.3.");
}

#[test]
fn step5_source_split_splits_on_hard_pause() {
    let response = tauri::async_runtime::block_on(build_step_5_1_source_split_with_progress(
        BuildStep5SourceSplitRequest {
            task_id: "t1".to_string(),
            media_path: "sample.mp4".to_string(),
            source_lang: "en".to_string(),
            target_lang: "zh-CN".to_string(),
            subtitle_length_preset: "short".to_string(),
            translate_api_key: "test".to_string(),
            translate_base_url: "https://api.openai.com/v1".to_string(),
            translate_model: "gpt-4.1-mini".to_string(),
            llm_concurrency: 1,
            segments: vec![Step5DraftSegment {
                segment_id: 1,
                start: 0.0,
                end: 8.0,
                source: "hello world how are you".to_string(),
                draft_translation: "你好 世界 你好吗".to_string(),
                tokens: vec![
                    Step5Token {
                        text: "hello".to_string(),
                        start: 0.0,
                        end: 0.5,
                    },
                    Step5Token {
                        text: "world".to_string(),
                        start: 0.5,
                        end: 1.0,
                    },
                    Step5Token {
                        text: "how".to_string(),
                        start: 3.4,
                        end: 3.8,
                    },
                    Step5Token {
                        text: "are".to_string(),
                        start: 3.8,
                        end: 4.2,
                    },
                    Step5Token {
                        text: "you".to_string(),
                        start: 4.2,
                        end: 4.8,
                    },
                ],
            }],
        },
        None,
    ))
    .expect("step5 source split");

    assert_eq!(response.parent_total, 1);
    assert_eq!(response.part_total, 2);
    assert_eq!(response.parents[0].parts.len(), 2);
    assert_eq!(response.parents[0].parts[0].source, "hello world");
    assert_eq!(response.parents[0].parts[1].source, "how are you");
}

#[test]
fn step5_source_split_keeps_short_segment_without_llm_settings() {
    let response = tauri::async_runtime::block_on(build_step_5_1_source_split_with_progress(
        BuildStep5SourceSplitRequest {
            task_id: "t1".to_string(),
            media_path: "sample.mp4".to_string(),
            source_lang: "zh-CN".to_string(),
            target_lang: "en".to_string(),
            subtitle_length_preset: "standard".to_string(),
            translate_api_key: String::new(),
            translate_base_url: String::new(),
            translate_model: String::new(),
            llm_concurrency: 1,
            segments: vec![Step5DraftSegment {
                segment_id: 1,
                start: 0.0,
                end: 2.0,
                source: "这是一个完整句子。".to_string(),
                draft_translation: "This is a complete sentence.".to_string(),
                tokens: vec![Step5Token {
                    text: "这是一个完整句子。".to_string(),
                    start: 0.0,
                    end: 2.0,
                }],
            }],
        },
        None,
    ))
    .expect("step5 source split");

    assert_eq!(response.part_total, 1);
    assert_eq!(response.parents[0].parts[0].source, "这是一个完整句子。");
}

#[test]
fn step5_source_split_validator_accepts_binary_exact_parts() {
    let value = serde_json::json!({
        "sourceParts": [
            "阿里巴巴就能在别的地方",
            "把钱赚回来。"
        ]
    });
    let parts = validate_source_split_response(value, "阿里巴巴就能在别的地方把钱赚回来。", false)
        .expect("valid split response");
    assert_eq!(parts.len(), 2);
    assert_eq!(
        compact_for_split_match(&parts.join("")),
        compact_for_split_match("阿里巴巴就能在别的地方把钱赚回来。")
    );
}

#[test]
fn step5_source_split_validator_rejects_extra_or_mutated_parts() {
    let too_many = serde_json::json!({
        "sourceParts": ["这是一", "个完整", "句子。"]
    });
    assert!(validate_source_split_response(too_many, "这是一个完整句子。", false).is_err());

    let mutated = serde_json::json!({
        "sourceParts": ["这是一个完整", "句子！"]
    });
    assert!(validate_source_split_response(mutated, "这是一个完整句子。", false).is_err());
}

#[test]
fn step5_source_split_validator_requires_two_parts_when_hard_overlong() {
    let single = serde_json::json!({
        "sourceParts": ["这是一个非常长但是仍然没有被拆开的句子。"]
    });
    assert!(
        validate_source_split_response(single, "这是一个非常长但是仍然没有被拆开的句子。", true)
            .is_err()
    );
}

#[test]
fn step5_source_split_maps_exact_cjk_parts_to_token_boundary() {
    let tokens = vec![
        Step5Token {
            text: "阿里巴巴".to_string(),
            start: 0.0,
            end: 0.4,
        },
        Step5Token {
            text: "就能".to_string(),
            start: 0.4,
            end: 0.8,
        },
        Step5Token {
            text: "在别的地方".to_string(),
            start: 0.8,
            end: 1.4,
        },
        Step5Token {
            text: "把钱赚回来。".to_string(),
            start: 1.4,
            end: 2.0,
        },
    ];
    let boundaries = map_source_parts_to_boundaries(
        &[
            "阿里巴巴就能在别的地方".to_string(),
            "把钱赚回来。".to_string(),
        ],
        &tokens,
        "zh-CN",
    );

    assert_eq!(boundaries, vec![3]);
}

#[test]
fn step5_source_split_merges_tiny_leading_piece() {
    let tokens = vec![
        Step5Token {
            text: "So,".to_string(),
            start: 0.0,
            end: 0.2,
        },
        Step5Token {
            text: "we".to_string(),
            start: 0.2,
            end: 0.4,
        },
        Step5Token {
            text: "are".to_string(),
            start: 0.4,
            end: 0.6,
        },
        Step5Token {
            text: "looking".to_string(),
            start: 0.6,
            end: 1.0,
        },
        Step5Token {
            text: "for".to_string(),
            start: 1.0,
            end: 1.2,
        },
        Step5Token {
            text: "entries".to_string(),
            start: 1.2,
            end: 1.6,
        },
        Step5Token {
            text: "right".to_string(),
            start: 1.6,
            end: 1.9,
        },
        Step5Token {
            text: "now".to_string(),
            start: 1.9,
            end: 2.2,
        },
        Step5Token {
            text: "today".to_string(),
            start: 2.2,
            end: 2.8,
        },
    ];
    let ranges = vec![(0usize, 0usize), (1usize, 8usize)];
    let merged = merge_tiny_ranges_for_readability(ranges, &tokens, "en", 20.0, &[]);
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0], (0, 8));
}

#[test]
fn step5_source_split_merges_sub_half_second_piece() {
    let tokens = vec![
        Step5Token {
            text: "intro".to_string(),
            start: 0.0,
            end: 0.2,
        },
        Step5Token {
            text: "this".to_string(),
            start: 0.2,
            end: 0.8,
        },
        Step5Token {
            text: "is".to_string(),
            start: 0.8,
            end: 1.2,
        },
        Step5Token {
            text: "longer".to_string(),
            start: 1.2,
            end: 2.0,
        },
    ];
    let ranges = vec![(0usize, 0usize), (1usize, 3usize)];
    let merged = merge_tiny_ranges_for_readability(ranges, &tokens, "en", 20.0, &[]);
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0], (0, 3));
}
