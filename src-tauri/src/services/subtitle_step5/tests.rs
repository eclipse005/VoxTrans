use std::collections::HashSet;

use super::alignment_repair::repair_aligned_lines;
use super::alignment_score::choose_better_alignment;
use super::numbers::extract_numbers;
use super::polish_repair::repair_polished_translation;
use super::quality::split_line_quality_score;
use super::source_residue::looks_like_source_residue;
use super::source_split::split_token_ranges;
use super::source_split_readability::{
    merge_tiny_ranges_for_readability, rebalance_dangling_tail_tokens,
};
use super::source_text::build_source_from_tokens;
use super::terminology_filter::source_contains_terminology_term;
use super::translation_candidate::{
    has_tail_ellipsis, is_unusable_translation, trim_before_leaked_number_anchor,
};
use super::translation_split::heuristic_split_translation;
use super::watchability::{
    is_watchability_fragment_issue, repair_single_watchability_line, repair_watchability_fragments,
};
use super::watchability_split::split_watchability_overlong_segments;
use super::{
    BuildStep5SourceSplitRequest, BuildStep6FinalCheckRequest, Step5DraftSegment,
    Step5FinalSegment, Step5SplitParent, Step5SplitPart, Step5Token,
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
            subtitle_max_words_per_segment: 16,
            subtitle_length_reference: 16,
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
fn step5_split_token_ranges_force_split_on_over_limit_without_pause() {
    let tokens = (0..24usize)
        .map(|idx| Step5Token {
            text: format!("w{idx}"),
            start: idx as f64 * 0.2,
            end: idx as f64 * 0.2 + 0.19,
        })
        .collect::<Vec<_>>();
    let ranges = split_token_ranges(&tokens, "en", 8.0, 80.0, 24.0, 24.0);
    assert!(ranges.len() >= 2);

    for (start, end) in ranges {
        let word_count = end.saturating_sub(start) + 1;
        assert!(word_count <= 10);
    }
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

#[test]
fn step5_source_split_rebalances_dangling_tail_tokens() {
    let tokens = vec![
        Step5Token {
            text: "it's".to_string(),
            start: 0.0,
            end: 0.2,
        },
        Step5Token {
            text: "very".to_string(),
            start: 0.2,
            end: 0.4,
        },
        Step5Token {
            text: "common".to_string(),
            start: 0.4,
            end: 0.6,
        },
        Step5Token {
            text: "when".to_string(),
            start: 0.6,
            end: 0.8,
        },
        Step5Token {
            text: "you're".to_string(),
            start: 0.8,
            end: 1.0,
        },
        Step5Token {
            text: "starting".to_string(),
            start: 1.0,
            end: 1.2,
        },
        Step5Token {
            text: "out".to_string(),
            start: 1.2,
            end: 1.4,
        },
        Step5Token {
            text: "to".to_string(),
            start: 1.4,
            end: 1.6,
        },
        Step5Token {
            text: "take".to_string(),
            start: 1.6,
            end: 1.8,
        },
        Step5Token {
            text: "a".to_string(),
            start: 1.8,
            end: 2.0,
        },
        Step5Token {
            text: "report".to_string(),
            start: 2.0,
            end: 2.4,
        },
    ];
    let ranges = vec![(0usize, 8usize), (9usize, 10usize)];
    let rebalanced = rebalance_dangling_tail_tokens(ranges, &tokens, "en", 20.0, &[]);
    assert_eq!(rebalanced, vec![(0usize, 6usize), (7usize, 10usize)]);
}

#[test]
fn step5_split_quality_penalizes_overlapping_lines() {
    let overlapping = vec![
        "10 因为我的大脑自然会想：为什么你不都在20点出场呢？".to_string(),
        "为什么你不都在20点出场呢？".to_string(),
    ];
    let clean = vec![
        "比如在10点出场一部分，20点再出场一部分。".to_string(),
        "因为我的大脑自然会想：为什么你不都在20点出场呢？".to_string(),
    ];
    assert!(split_line_quality_score(&clean) > split_line_quality_score(&overlapping));
}

#[test]
fn step5_trim_before_leaked_number_anchor_trims_head() {
    let mut leaked = HashSet::<String>::new();
    leaked.insert("10".to_string());
    let trimmed = trim_before_leaked_number_anchor(
        "你知道， 对于复盘练习， 当你刚开始时， 做一个10分钟的记录，",
        &leaked,
    );
    assert_eq!(
        trimmed,
        Some("你知道， 对于复盘练习， 当你刚开始时， 做一个".to_string())
    );
}

#[test]
fn step5_source_terminology_matcher_uses_boundaries_for_short_ascii_terms() {
    assert!(!source_contains_terminology_term(
        "this example executes normally",
        "x"
    ));
    assert!(source_contains_terminology_term(
        "this result is 2x higher",
        "x"
    ));
    assert!(source_contains_terminology_term(
        "this result is x higher",
        "x"
    ));
    assert!(!source_contains_terminology_term(
        "the prefixvalue should not match",
        "fix"
    ));
}

#[test]
fn step5_align_repair_fills_empty_lines() {
    let parent = Step5SplitParent {
        parent_segment_id: 1,
        draft_translation: "先做计划，再执行".to_string(),
        parts: vec![
            Step5SplitPart {
                part_id: 1,
                start: 0.0,
                end: 1.0,
                source: "first part".to_string(),
                tokens: vec![],
            },
            Step5SplitPart {
                part_id: 2,
                start: 1.0,
                end: 2.0,
                source: "second part".to_string(),
                tokens: vec![],
            },
        ],
    };
    let aligned = vec!["".to_string(), "执行".to_string()];
    let fallback = vec!["先做计划".to_string(), "再执行".to_string()];
    let repaired = repair_aligned_lines(&parent, &aligned, &fallback, "zh-CN");
    assert_eq!(repaired.len(), 2);
    assert_eq!(repaired[0], "先做计划");
    assert_eq!(repaired[1], "执行");
}

#[test]
fn step6_final_check_detects_hard_issues() {
    let report = super::build_step_6_final_check(BuildStep6FinalCheckRequest {
        target_lang: "zh-CN".to_string(),
        segments: vec![
            Step5FinalSegment {
                segment_id: 1,
                start: 0.0,
                end: 1.0,
                source: "the invoice was 1037 units".to_string(),
                translation: "".to_string(),
                tokens: vec![],
            },
            Step5FinalSegment {
                segment_id: 2,
                start: 1.0,
                end: 2.0,
                source: "be patient and disciplined".to_string(),
                translation: "要保持耐心，而且...".to_string(),
                tokens: vec![],
            },
        ],
    })
    .expect("final check");
    assert!(!report.passed);
    assert!(report.hard_fail_count >= 2);
}

#[test]
fn step6_final_check_flags_watchability_fragment_issue() {
    let report = super::build_step_6_final_check(BuildStep6FinalCheckRequest {
        target_lang: "zh-CN".to_string(),
        segments: vec![Step5FinalSegment {
            segment_id: 1,
            start: 0.0,
            end: 1.0,
            source: "and I sit down, I start loading up my writing workspace".to_string(),
            translation: "然后花大约".to_string(),
            tokens: vec![],
        }],
    })
    .expect("final check");
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.rule_id == "watchability_fragment")
    );
}

#[test]
fn step6_final_check_does_not_report_terminology_drift() {
    let report = super::build_step_6_final_check(BuildStep6FinalCheckRequest {
        target_lang: "zh-CN".to_string(),
        segments: vec![Step5FinalSegment {
            segment_id: 1,
            start: 0.0,
            end: 1.0,
            source: "This mentions a single-letter term.".to_string(),
            translation: "这里提到了一个单字母术语。".to_string(),
            tokens: vec![],
        }],
    })
    .expect("final check");
    assert!(report.passed);
    assert!(
        !report
            .issues
            .iter()
            .any(|issue| issue.rule_id == "terminology_drift")
    );
}

#[test]
fn step5_watchability_repair_does_not_invent_percent_sentence() {
    let mut segments = vec![Step5FinalSegment {
            segment_id: 1,
            start: 0.0,
            end: 1.0,
            source: "happy with if you're keeping 50%or 60%of the total budget without leaving a lot unused.".to_string(),
            translation: "60 50 你更有可能持续地以满意的金额完成那项计划。".to_string(),
            tokens: vec![],
        }];
    repair_watchability_fragments(&mut segments, "zh-CN");
    assert_eq!(
        segments[0].translation,
        "60 50 你更有可能持续地以满意的金额完成那项计划。"
    );
    assert!(!segments[0].translation.contains("如果你拿走"));
}

#[test]
fn step5_watchability_repair_does_not_invent_domain_sentence() {
    let mut segments = vec![Step5FinalSegment {
        segment_id: 1,
        start: 0.0,
        end: 1.0,
        source: "to take a 10-minute note and watch it grow to 50 lines".to_string(),
        translation: "10 看着它增加到50行。".to_string(),
        tokens: vec![],
    }];
    repair_watchability_fragments(&mut segments, "zh-CN");
    assert_eq!(segments[0].translation, "10 看着它增加到50行。");
}

#[test]
fn step5_watchability_repair_does_not_invent_domain_phrasing() {
    let repaired = repair_single_watchability_line(
        "after the 10 minute review we wait for confirmation",
        "10后等待确认",
        "zh-CN",
    );

    assert_eq!(repaired, "10后等待确认");
}

#[test]
fn step5_watchability_repair_does_not_invent_day_phrase() {
    let mut segments = vec![Step5FinalSegment {
        segment_id: 1,
        start: 0.0,
        end: 1.0,
        source: "a day in 30 minutes or whatever it is, right?".to_string(),
        translation: "30分钟，对吧？".to_string(),
        tokens: vec![],
    }];
    repair_watchability_fragments(&mut segments, "zh-CN");
    assert_eq!(segments[0].translation, "30分钟，对吧？");
}

#[test]
fn step5_watchability_repair_does_not_invent_seconds_phrase() {
    let mut segments = vec![Step5FinalSegment {
        segment_id: 1,
        start: 0.0,
        end: 1.0,
        source: "and I just take like 30 seconds to read these".to_string(),
        translation: "30 来阅读这些内容。".to_string(),
        tokens: vec![],
    }];
    repair_watchability_fragments(&mut segments, "zh-CN");
    assert_eq!(segments[0].translation, "30 来阅读这些内容。");
}

#[test]
fn step5_watchability_repair_trims_trailing_connector_fragment() {
    let mut segments = vec![Step5FinalSegment {
        segment_id: 1,
        start: 0.0,
        end: 1.0,
        source:
            "I have these notes open and when they get outdated I rewrite them and put them back up"
                .to_string(),
        translation: "我有这些笔记，而且".to_string(),
        tokens: vec![],
    }];
    repair_watchability_fragments(&mut segments, "zh-CN");
    assert_eq!(segments[0].translation, "我有这些笔记");
}

#[test]
fn step5_polish_adds_terminal_punctuation_for_finished_source_sentence() {
    let mut segment = Step5FinalSegment {
        segment_id: 190,
        start: 0.0,
        end: 1.0,
        source: "Alright, I believe this is the last one for the week.".to_string(),
        translation: "好了，我认为这是本周最后一个".to_string(),
        tokens: vec![],
    };

    repair_polished_translation(&mut segment);

    assert_eq!(segment.translation, "好了，我认为这是本周最后一个。");
    assert!(!is_watchability_fragment_issue(
        &segment.source,
        &segment.translation,
        "zh-CN"
    ));
}

#[test]
fn step5_watchability_split_long_segments_with_source_tokens() {
    let mut segments = vec![Step5FinalSegment {
            segment_id: 1,
            start: 0.0,
            end: 12.0,
            source: "I want to share a deeper note for this part and make it easier for review.".to_string(),
            translation: "I want to share a deeper note for this part and make it easier for review while keeping enough context".to_string(),
            tokens: (0..16)
                .map(|index| Step5Token {
                    text: format!("w{index}"),
                    start: index as f64 * 0.2,
                    end: (index as f64 + 1.0) * 0.2,
                })
                .collect::<Vec<_>>(),
        }];
    split_watchability_overlong_segments(&mut segments, 15.0, "en-US");
    assert!(segments.len() >= 2);
    for segment in &segments {
        assert!(!segment.translation.trim().is_empty());
        assert!(!segment.source.trim().is_empty());
        assert!(segment.end >= segment.start);
    }
}

#[test]
fn step5_watchability_split_long_segments_without_tokens() {
    let mut segments = vec![Step5FinalSegment {
        segment_id: 1,
        start: 0.0,
        end: 10.0,
        source: "这个中文句子会被切分以便提升观看体验".to_string(),
        translation: "这个中文句子会被切分以便提升观看体验效果从而更容易理解".to_string(),
        tokens: vec![],
    }];
    split_watchability_overlong_segments(&mut segments, 7.0, "zh-CN");
    assert!(segments.len() >= 2);
    assert_eq!(segments[0].segment_id, 1);
    assert_eq!(segments[segments.len() - 1].segment_id, segments.len());
    assert!(segments[0].end <= segments[1].end);
}

#[test]
fn step5_watchability_split_rejects_source_residue_fallback_for_cjk() {
    let source = "Then we switch to the one minute review window, wait for the paragraph to match the highlighted note, which was";
    let words = source.split_whitespace().collect::<Vec<_>>();
    let tokens = words
        .iter()
        .enumerate()
        .map(|(index, word)| Step5Token {
            text: (*word).to_string(),
            start: index as f64 * 0.4,
            end: (index as f64 + 1.0) * 0.4,
        })
        .collect::<Vec<_>>();
    let mut segments = vec![Step5FinalSegment {
        segment_id: 1,
        start: 82.56,
        end: 87.52,
        source: source.to_string(),
        translation: "然后我们切换到一分钟复盘窗口，等待段落匹配高亮笔记（即，高亮笔记 / 关注区"
            .to_string(),
        tokens,
    }];

    split_watchability_overlong_segments(&mut segments, 8.0, "zh-CN");

    assert!(segments.iter().all(|segment| {
        !looks_like_source_residue(&segment.source, &segment.translation, "zh-CN")
            && !is_unusable_translation(&segment.translation)
    }));
}

#[test]
fn step5_helpers_detect_tail_ellipsis_and_numbers() {
    assert!(has_tail_ellipsis("我们正试图..."));
    assert!(has_tail_ellipsis("我们正试图. ."));
    let numbers = extract_numbers("start 1,037.5 then 20 grand and 5万");
    assert!(numbers.contains("1037.5"));
    assert!(numbers.contains("20000"));
    assert!(numbers.contains("50000"));

    let bucks = extract_numbers("make 2,000 bucks");
    assert!(bucks.contains("2000"));

    let mixed = extract_numbers("a thousand and 37 bucks");
    assert!(mixed.contains("1037"));

    let punct_followed = extract_numbers("50,my tank might decrease");
    assert!(punct_followed.contains("50"));
    assert!(!punct_followed.contains("50000000"));
}

#[test]
fn step5_repair_treats_punctuation_only_as_invalid() {
    let parent = Step5SplitParent {
        parent_segment_id: 1,
        draft_translation: "保持耐心，稳扎稳打".to_string(),
        parts: vec![Step5SplitPart {
            part_id: 1,
            start: 0.0,
            end: 1.0,
            source: "be patient and stay consistent".to_string(),
            tokens: vec![],
        }],
    };
    let aligned = vec![".".to_string()];
    let fallback = vec!["保持耐心，稳扎稳打".to_string()];
    let repaired = repair_aligned_lines(&parent, &aligned, &fallback, "zh-CN");
    assert_eq!(repaired[0], "保持耐心，稳扎稳打");
}

#[test]
fn step5_repair_prefers_numeric_consistent_fallback() {
    let parent = Step5SplitParent {
        parent_segment_id: 1,
        draft_translation: "应该赚到2万美元".to_string(),
        parts: vec![Step5SplitPart {
            part_id: 1,
            start: 0.0,
            end: 1.0,
            source: "should be making $20,000".to_string(),
            tokens: vec![],
        }],
    };
    let aligned = vec!["应该在30分钟内赚到2万美元".to_string()];
    let fallback = vec!["应该赚到2万美元".to_string()];
    let repaired = repair_aligned_lines(&parent, &aligned, &fallback, "zh-CN");
    assert_eq!(repaired[0], "应该赚到2万美元");
}

#[test]
fn step5_align_repair_replaces_duplicated_parent_copy() {
    let parent = Step5SplitParent {
        parent_segment_id: 1,
        draft_translation: "如果你每次上垒都能稳定执行，长期就能积累优势".to_string(),
        parts: vec![
            Step5SplitPart {
                part_id: 1,
                start: 0.0,
                end: 1.0,
                source: "if you consistently get on base".to_string(),
                tokens: vec![],
            },
            Step5SplitPart {
                part_id: 2,
                start: 1.0,
                end: 2.0,
                source: "you will stack an edge over time".to_string(),
                tokens: vec![],
            },
        ],
    };
    let aligned = vec![
        "如果你每次上垒都能稳定执行，长期就能积累优势".to_string(),
        "如果你每次上垒都能稳定执行，长期就能积累优势".to_string(),
    ];
    let fallback = vec![
        "每次上垒都稳定执行".to_string(),
        "长期就能积累优势".to_string(),
    ];
    let repaired = repair_aligned_lines(&parent, &aligned, &fallback, "zh-CN");
    assert_eq!(repaired[0], "每次上垒都稳定执行");
    assert_eq!(repaired[1], "长期就能积累优势");
}

#[test]
fn step5_align_repair_replaces_source_residue_for_cjk_target() {
    let parent = Step5SplitParent {
        parent_segment_id: 1,
        draft_translation: "要收手，等下一次机会".to_string(),
        parts: vec![Step5SplitPart {
            part_id: 1,
            start: 0.0,
            end: 1.0,
            source: "and out, keep my short note and review another day".to_string(),
            tokens: vec![],
        }],
    };
    let aligned = vec!["and out keep my short note and review another day".to_string()];
    let fallback = vec!["收手，保留简短记录，留到下次再复盘".to_string()];
    let repaired = repair_aligned_lines(&parent, &aligned, &fallback, "zh-CN");
    assert_eq!(repaired[0], "收手，保留简短记录，留到下次再复盘");
}

#[test]
fn step5_fallback_split_avoids_empty_lines_for_weak_model() {
    let lines = heuristic_split_translation(
        "如果你一次只做一个高把握动作就能稳定推进并且减少回撤",
        3,
        None,
    );
    assert_eq!(lines.len(), 3);
    assert!(lines.iter().all(|line| !line.trim().is_empty()));
}

#[test]
fn step5_align_repair_rebalances_leaked_numbers_between_neighbors() {
    let parent = Step5SplitParent {
        parent_segment_id: 1,
        draft_translation: "刚开始时很常见，你写一个10分钟记录，看它扩展到50行。".to_string(),
        parts: vec![
            Step5SplitPart {
                part_id: 1,
                start: 0.0,
                end: 1.0,
                source: "it's very common and challenging when you're first starting out"
                    .to_string(),
                tokens: vec![],
            },
            Step5SplitPart {
                part_id: 2,
                start: 1.0,
                end: 2.0,
                source: "to write a 10-minute note and watch it grow to 50 lines".to_string(),
                tokens: vec![],
            },
        ],
    };
    let aligned = vec![
        "刚开始时，写一个10分钟记录很常见".to_string(),
        "看它扩展到50行很难".to_string(),
    ];
    let fallback = vec![
        "刚开始时这很常见".to_string(),
        "看它扩展到50行很难".to_string(),
    ];
    let repaired = repair_aligned_lines(&parent, &aligned, &fallback, "zh-CN");
    assert_eq!(repaired[0], "刚开始时这很常见");
    assert!(repaired[1].contains("10"));
    assert!(repaired[1].contains("50"));
}

#[test]
fn step5_align_repair_removes_next_line_number_from_numberless_line() {
    let parent = Step5SplitParent {
            parent_segment_id: 67,
            draft_translation: "你知道，对于复盘练习，当你刚开始时，写一个10分钟的记录，看着它扩展到50行或100行，这实际上非常常见，也非常具有挑战性。".to_string(),
            parts: vec![
                Step5SplitPart {
                    part_id: 1,
                    start: 0.0,
                    end: 1.0,
                    source:
                        "with review practice it's actually very common and very challenging when you're first starting out"
                            .to_string(),
                    tokens: vec![],
                },
                Step5SplitPart {
                    part_id: 2,
                    start: 1.0,
                    end: 2.0,
                    source:
                        "to write a 10-minute note and watch it grow to 50 lines or watch it grow to 100 lines"
                            .to_string(),
                    tokens: vec![],
                },
            ],
        };
    let aligned = vec![
        "你知道， 对于复盘练习， 当你刚开始时， 写一个10分钟的记录，".to_string(),
        "10 看着它扩展到50行或100行， 这实际上非常常见， 也非常具有挑战性。".to_string(),
    ];
    let fallback = aligned.clone();
    let repaired = repair_aligned_lines(&parent, &aligned, &fallback, "zh-CN");
    assert!(
        !repaired[0].contains("10"),
        "left line still leaks number: {}",
        repaired[0]
    );
    assert!(repaired[0].contains("当你刚开始时"));
    assert!(repaired[1].contains("10"));
    assert!(repaired[1].contains("50"));
    assert!(repaired[1].contains("100"));
}

#[test]
fn step5_align_repair_does_not_invent_percent_line() {
    let parent = Step5SplitParent {
            parent_segment_id: 191,
            draft_translation: "如果你保留总预算的50%或60%，而不留下太多未使用额度，你更有可能持续地以满意的金额完成那项计划。".to_string(),
            parts: vec![
                Step5SplitPart {
                    part_id: 1,
                    start: 0.0,
                    end: 1.0,
                    source: "You're more consistently likely to be able to get out".to_string(),
                    tokens: vec![],
                },
                Step5SplitPart {
                    part_id: 2,
                    start: 1.0,
                    end: 2.0,
                    source: "of that plan at a total value that you're".to_string(),
                    tokens: vec![],
                },
                Step5SplitPart {
                    part_id: 3,
                    start: 2.0,
                    end: 3.0,
                    source: "happy with if you're keeping 50%or 60%of the total budget without leaving a lot unused.".to_string(),
                    tokens: vec![],
                },
            ],
        };
    let aligned = vec![
        "如果你保留总预算的50%或60%，".to_string(),
        "而不留下太多未使用额度，".to_string(),
        "60 50 你更有可能持续地以满意的金额完成那项计划。".to_string(),
    ];
    let fallback = aligned.clone();
    let repaired = repair_aligned_lines(&parent, &aligned, &fallback, "zh-CN");
    assert_eq!(
        repaired[2],
        "60 50 你更有可能持续地以满意的金额完成那项计划。"
    );
    assert!(!repaired[2].contains("如果你保留"));
}

#[test]
fn step5_align_repair_fixes_hua_dayue_fragment_pair() {
    let parent = Step5SplitParent {
            parent_segment_id: 221,
            draft_translation: "但当我坐下来时，我会先打开写作工作区，然后花大约30秒的时间，一个小小的30秒休息，来阅读这些内容并大声念出来。".to_string(),
            parts: vec![
                Step5SplitPart {
                    part_id: 1,
                    start: 0.0,
                    end: 1.0,
                    source: "But what I do when I sit down,I come".to_string(),
                    tokens: vec![],
                },
                Step5SplitPart {
                    part_id: 2,
                    start: 1.0,
                    end: 2.0,
                    source: "and I sit down,I start opening up my writing workspace,".to_string(),
                    tokens: vec![],
                },
                Step5SplitPart {
                    part_id: 3,
                    start: 2.0,
                    end: 3.0,
                    source: "and I just take like 30 seconds,a little 30-second break to read these and actually say them out loud.".to_string(),
                    tokens: vec![],
                },
            ],
        };
    let aligned = vec![
        "但我坐下时，我会先打开写作工作区。".to_string(),
        "然后花大约".to_string(),
        "30 来阅读这些内容并大声念出来。".to_string(),
    ];
    let fallback = aligned.clone();
    let repaired = repair_aligned_lines(&parent, &aligned, &fallback, "zh-CN");
    assert_ne!(repaired[1], "然后花大约");
    assert!(repaired[2].contains("30"));
}

#[test]
fn step5_align_repair_does_not_invent_day_in_minutes_fragment() {
    let parent = Step5SplitParent {
        parent_segment_id: 202,
        draft_translation: "因为我在网上看到，我应该在30分钟内赚到2万美元，或者类似的说法，对吧？"
            .to_string(),
        parts: vec![
            Step5SplitPart {
                part_id: 1,
                start: 0.0,
                end: 1.0,
                source: "Because I see online that I should be making$20,000".to_string(),
                tokens: vec![],
            },
            Step5SplitPart {
                part_id: 2,
                start: 1.0,
                end: 2.0,
                source: "a day in 30 minutes or whatever it is,right?".to_string(),
                tokens: vec![],
            },
        ],
    };
    let aligned = vec![
        "因为我在网上看到，我应该在30分钟内赚到2万美元".to_string(),
        "30 或者类似的说法，对吧？".to_string(),
    ];
    let fallback = aligned.clone();
    let repaired = repair_aligned_lines(&parent, &aligned, &fallback, "zh-CN");
    assert_eq!(repaired[1], "30 或者类似的说法，对吧？");
    assert!(!repaired[1].contains("一天"));
}

#[test]
fn step5_align_repair_does_not_invent_seconds_fragment() {
    let parent = Step5SplitParent {
            parent_segment_id: 221,
            draft_translation: "但当我坐下来时，我会先打开写作工作区，然后花大约30秒的时间，一个小小的30秒休息，来阅读这些内容并大声念出来。".to_string(),
            parts: vec![
                Step5SplitPart {
                    part_id: 1,
                    start: 0.0,
                    end: 1.0,
                    source: "But what I do when I sit down,I come".to_string(),
                    tokens: vec![],
                },
                Step5SplitPart {
                    part_id: 2,
                    start: 1.0,
                    end: 2.0,
                    source: "and I sit down,I start opening up my writing workspace,".to_string(),
                    tokens: vec![],
                },
                Step5SplitPart {
                    part_id: 3,
                    start: 2.0,
                    end: 3.0,
                    source: "and I just take like 30 seconds,a little 30-second break to read these and actually say them out loud.".to_string(),
                    tokens: vec![],
                },
            ],
        };
    let aligned = vec![
        "但我坐下时，我会先打开写作工作区。".to_string(),
        "然后".to_string(),
        "30 来阅读这些内容并大声念出来。".to_string(),
    ];
    let fallback = aligned.clone();
    let repaired = repair_aligned_lines(&parent, &aligned, &fallback, "zh-CN");
    assert!(repaired.iter().any(|line| line.contains("写作工作区")));
    assert_eq!(repaired[2], "30 来阅读这些内容并大声念出来。");
    assert!(!repaired[2].starts_with("花大约"));
}

#[test]
fn step5_align_repair_removes_neighbor_number_leak_when_targets_already_covered() {
    let parent = Step5SplitParent {
        parent_segment_id: 1,
        draft_translation: "如果文档先增加100行再删掉50行，我的状态可能比稳定增加10行更快下降。"
            .to_string(),
        parts: vec![
            Step5SplitPart {
                part_id: 1,
                start: 0.0,
                end: 1.0,
                source: "if my document grows by 100 lines and starts shrinking".to_string(),
                tokens: vec![],
            },
            Step5SplitPart {
                part_id: 2,
                start: 1.0,
                end: 2.0,
                source:
                    "50,my focus might decrease even faster than just adding consistent 10 lines"
                        .to_string(),
                tokens: vec![],
            },
        ],
    };
    let aligned = vec![
        "如果文档先增加100行再删掉50行".to_string(),
        "50 我的状态可能比稳定增加10行更快下降".to_string(),
    ];
    let fallback = vec![
        "如果文档先增加100行再删掉50行".to_string(),
        "50 我的状态可能比稳定增加10行更快下降".to_string(),
    ];
    let repaired = repair_aligned_lines(&parent, &aligned, &fallback, "zh-CN");
    assert!(repaired[0].contains("100"));
    assert!(!repaired[0].contains("50"));
    assert!(repaired[1].contains("50"));
    assert!(repaired[1].contains("10"));
}

#[test]
fn step5_align_repair_handles_real_world_parent150_number_leak() {
    let parent = Step5SplitParent {
            parent_segment_id: 150,
            draft_translation: "如果我的文档增加了100行然后删掉50行，我的精力可能比只是稳定增加10行消耗得更快。".to_string(),
            parts: vec![
                Step5SplitPart {
                    part_id: 1,
                    start: 574.645,
                    end: 578.005,
                    source: "If my document grows by 100 lines and starts shrinking"
                        .to_string(),
                    tokens: vec![],
                },
                Step5SplitPart {
                    part_id: 2,
                    start: 578.085,
                    end: 582.725,
                    source:
                        "50,my energy might decrease even faster as opposed to just having consistent 10 lines."
                            .to_string(),
                    tokens: vec![],
                },
            ],
        };
    let aligned = vec![
        "如果我的文档增加了100行然后删掉50行，".to_string(),
        "50 我的精力可能比只是稳定增加10行消耗得更快。".to_string(),
    ];
    let fallback = aligned.clone();
    let repaired = repair_aligned_lines(&parent, &aligned, &fallback, "zh-CN");
    assert!(repaired[0].contains("100"));
    assert!(!repaired[0].contains("50"));
    assert!(repaired[1].contains("50"));
    assert!(repaired[1].contains("10"));
}

#[test]
fn step5_heuristic_split_avoids_cjk_mid_phrase_fragments() {
    let parts = vec![
            Step5SplitPart {
                part_id: 1,
                start: 0.0,
                end: 1.0,
                source: "But what I do when I sit down,I come and".to_string(),
                tokens: vec![],
            },
            Step5SplitPart {
                part_id: 2,
                start: 1.0,
                end: 2.0,
                source: "I sit down,I start opening up my writing workspace,".to_string(),
                tokens: vec![],
            },
            Step5SplitPart {
                part_id: 3,
                start: 2.0,
                end: 3.0,
                source: "and I just take like 30 seconds,a little 30-second break to read these and actually say them out loud.".to_string(),
                tokens: vec![],
            },
        ];
    let lines = heuristic_split_translation(
        "但当我坐下来时，我会先打开写作工作区，然后花大约30秒的时间，一个小小的30秒休息，来阅读这些内容并大声念出来。",
        3,
        Some(&parts),
    );
    assert_eq!(lines.len(), 3);
    assert!(lines.iter().all(|line| !line.trim().is_empty()));
    assert!(!lines.iter().any(|line| line.trim() == "然后花大约"));
    assert!(lines.iter().any(|line| line.contains("写作工作区")));
    assert!(lines.iter().any(|line| line.contains("30秒")));
}

#[test]
fn step5_align_prefers_fallback_when_llm_lines_are_fragmented() {
    let parent = Step5SplitParent {
            parent_segment_id: 221,
            draft_translation: "但当我坐下来时，我会先打开写作工作区，然后花大约30秒的时间，一个小小的30秒休息，来阅读这些内容并大声念出来。".to_string(),
            parts: vec![
                Step5SplitPart {
                    part_id: 1,
                    start: 0.0,
                    end: 1.0,
                    source: "But what I do when I sit down,I come and".to_string(),
                    tokens: vec![],
                },
                Step5SplitPart {
                    part_id: 2,
                    start: 1.0,
                    end: 2.0,
                    source: "I sit down,I start opening up my writing workspace,".to_string(),
                    tokens: vec![],
                },
                Step5SplitPart {
                    part_id: 3,
                    start: 2.0,
                    end: 3.0,
                    source: "and I just take like 30 seconds,a little 30-second break to read these and actually say them out loud.".to_string(),
                    tokens: vec![],
                },
            ],
        };
    let fragmented = vec![
        "但我坐下来时，我会先打开写作工作区。".to_string(),
        "然后花大约".to_string(),
        "时间，一个小小的30秒休息，来阅读这些内容并大声念出来。".to_string(),
    ];
    let fallback = vec![
        "但当我坐下来时".to_string(),
        "我会先打开写作工作区，然后花一点时间".to_string(),
        "进行一个30秒的小休息，来阅读这些内容并大声念出来。".to_string(),
    ];
    let selected = choose_better_alignment(&parent, &fragmented, &fallback, "zh-CN");
    assert_eq!(selected, fallback);
}

#[test]
fn step5_align_choice_rejects_fallback_with_next_line_numeric_leak() {
    let parent = Step5SplitParent {
            parent_segment_id: 67,
            draft_translation: "对于复盘练习，当你刚开始时，写一个10分钟的记录，看着它扩展到50行或100行，这很常见也很有挑战。".to_string(),
            parts: vec![
                Step5SplitPart {
                    part_id: 1,
                    start: 0.0,
                    end: 1.0,
                    source: "with review practice it's very common and challenging when you're first starting out".to_string(),
                    tokens: vec![],
                },
                Step5SplitPart {
                    part_id: 2,
                    start: 1.0,
                    end: 2.0,
                    source: "to write a 10-minute note and watch it grow to 50 lines or 100 lines".to_string(),
                    tokens: vec![],
                },
            ],
        };
    let aligned_without_leak = vec![
        "对于复盘练习，当你刚开始时，这很常见也很有挑战。".to_string(),
        "写一个10分钟的记录，看着它扩展到50行或100行。".to_string(),
    ];
    let fallback_with_leak = vec![
        "对于复盘练习，当你刚开始时，写一个10分钟的记录。".to_string(),
        "10 看着它扩展到50行或100行，这很常见也很有挑战。".to_string(),
    ];
    let selected =
        choose_better_alignment(&parent, &aligned_without_leak, &fallback_with_leak, "zh-CN");
    assert_eq!(selected, aligned_without_leak);
}

#[test]
fn step5_source_residue_detection_flags_untranslated_english() {
    assert!(looks_like_source_residue(
        "and out, keep my short note and review another day",
        "and out keep my short note and review another day",
        "zh-CN"
    ));
    assert!(!looks_like_source_residue(
        "review strategy",
        "这是我的 review 策略",
        "zh-CN"
    ));
}
