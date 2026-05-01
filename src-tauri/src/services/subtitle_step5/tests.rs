use std::collections::HashSet;

use super::alignment_repair::repair_aligned_lines;
use super::alignment_score::choose_better_alignment;
use super::language_units::text_length_units;
use super::numbers::extract_numbers;
use super::quality::split_line_quality_score;
use super::source_residue::looks_like_source_residue;
use super::terminology_filter::source_contains_terminology_term;
use super::translation_candidate::{has_tail_ellipsis, trim_before_leaked_number_anchor};
use super::translation_split::heuristic_split_translation;
use super::watchability::repair_single_watchability_line;
use super::{Step5SplitParent, Step5SplitPart};
use crate::services::subtitle_length::{
    SubtitleLengthPreset, effective_subtitle_limits, normalize_subtitle_length_preset,
};

mod source_split_tests;

#[test]
fn subtitle_length_presets_cover_supported_source_and_target_languages() {
    let source_langs = ["en", "zh", "yue", "ja", "ko", "fr", "de", "it", "es", "pt"];
    let target_langs = [
        "zh-CN", "zh-TW", "en", "ja", "ko", "fr", "de", "es", "it", "pt", "ru", "ar", "vi", "th",
        "id", "tr", "nl", "pl",
    ];

    for source_lang in source_langs {
        for target_lang in target_langs {
            let short =
                effective_subtitle_limits(source_lang, target_lang, SubtitleLengthPreset::Short);
            let standard =
                effective_subtitle_limits(source_lang, target_lang, SubtitleLengthPreset::Standard);
            let loose =
                effective_subtitle_limits(source_lang, target_lang, SubtitleLengthPreset::Loose);

            assert!(
                short.source_limit < standard.source_limit
                    && standard.source_limit < loose.source_limit,
                "source limits should increase for {source_lang}"
            );
            assert!(
                short.target_limit < standard.target_limit
                    && standard.target_limit < loose.target_limit,
                "target limits should increase for {target_lang}"
            );
        }
    }
}

#[test]
fn subtitle_length_preset_normalization_accepts_three_ui_modes() {
    assert_eq!(normalize_subtitle_length_preset("short"), "short");
    assert_eq!(normalize_subtitle_length_preset("standard"), "standard");
    assert_eq!(normalize_subtitle_length_preset("loose"), "loose");
    assert_eq!(normalize_subtitle_length_preset("unexpected"), "standard");
}

#[test]
fn subtitle_length_limits_are_language_aware_for_source_and_target() {
    let english_to_chinese =
        effective_subtitle_limits("en", "zh-CN", SubtitleLengthPreset::Standard);
    assert_eq!(english_to_chinese.source_limit, 20);
    assert_eq!(english_to_chinese.target_limit, 28);

    let chinese_to_english = effective_subtitle_limits("zh", "en", SubtitleLengthPreset::Standard);
    assert_eq!(chinese_to_english.source_limit, 28);
    assert_eq!(chinese_to_english.target_limit, 16);

    let cantonese_to_english =
        effective_subtitle_limits("yue", "en", SubtitleLengthPreset::Standard);
    assert_eq!(cantonese_to_english.source_limit, 28);
    assert_eq!(cantonese_to_english.target_limit, 16);

    let english_to_thai = effective_subtitle_limits("en", "th", SubtitleLengthPreset::Standard);
    assert!(english_to_thai.target_limit > english_to_chinese.target_limit);
}

#[test]
fn subtitle_length_units_count_thai_as_char_units() {
    assert!(text_length_units("นี่คือข้อความภาษาไทย", "th") > 5.0);
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
fn step5_watchability_repair_does_not_invent_percent_sentence() {
    let repaired = repair_single_watchability_line(
        "happy with if you're keeping 50%or 60%of the total budget without leaving a lot unused.",
        "60 50 你更有可能持续地以满意的金额完成那项计划。",
        "zh-CN",
    );
    assert_eq!(repaired, "60 50 你更有可能持续地以满意的金额完成那项计划。");
    assert!(!repaired.contains("如果你拿走"));
}

#[test]
fn step5_watchability_repair_does_not_invent_domain_sentence() {
    let repaired = repair_single_watchability_line(
        "to take a 10-minute note and watch it grow to 50 lines",
        "10 看着它增加到50行。",
        "zh-CN",
    );
    assert_eq!(repaired, "10 看着它增加到50行。");
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
    let repaired = repair_single_watchability_line(
        "a day in 30 minutes or whatever it is, right?",
        "30分钟，对吧？",
        "zh-CN",
    );
    assert_eq!(repaired, "30分钟，对吧？");
}

#[test]
fn step5_watchability_repair_does_not_invent_seconds_phrase() {
    let repaired = repair_single_watchability_line(
        "and I just take like 30 seconds to read these",
        "30 来阅读这些内容。",
        "zh-CN",
    );
    assert_eq!(repaired, "30 来阅读这些内容。");
}

#[test]
fn step5_watchability_repair_trims_trailing_connector_fragment() {
    let repaired = repair_single_watchability_line(
        "I have these notes open and when they get outdated I rewrite them and put them back up",
        "我有这些笔记，而且",
        "zh-CN",
    );
    assert_eq!(repaired, "我有这些笔记");
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
