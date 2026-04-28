use std::collections::HashSet;

use super::language_units::text_length_units;
use super::numbers::extract_numbers;
use super::source_residue::looks_like_source_residue;
use super::text_utils::normalize_inline_text;
use super::translation_candidate::{has_tail_ellipsis, is_punctuation_only};
use super::types::{
    BuildStep6FinalCheckRequest, BuildStep6FinalCheckResponse, Step5QualityIssue,
    Step6FinalCheckMetrics,
};
use super::watchability::is_watchability_fragment_issue;

pub fn build_step_6_final_check(
    request: BuildStep6FinalCheckRequest,
) -> Result<BuildStep6FinalCheckResponse, String> {
    if request.segments.is_empty() {
        return Err("segments is required".to_string());
    }

    let mut issues = Vec::<Step5QualityIssue>::new();
    let mut issue_keys = HashSet::<String>::new();
    let mut empty_count = 0usize;
    let mut ellipsis_tail_count = 0usize;
    let mut numeric_drift_count = 0usize;
    let mut cross_line_leak_count = 0usize;
    let mut gt25_count = 0usize;
    let mut gt32_count = 0usize;

    for (index, segment) in request.segments.iter().enumerate() {
        let source = normalize_inline_text(&segment.source);
        let translation = normalize_inline_text(&segment.translation);
        let target_units = text_length_units(&translation, &request.target_lang);
        if target_units > 25.0 {
            gt25_count += 1;
        }
        if target_units > 32.0 {
            gt32_count += 1;
        }
        if translation.is_empty() {
            empty_count += 1;
            push_quality_issue(
                &mut issues,
                &mut issue_keys,
                Step5QualityIssue {
                    rule_id: "empty_translation".to_string(),
                    severity: "hard".to_string(),
                    segment_id: segment.segment_id,
                    part_id: index + 1,
                    message: "字幕译文为空".to_string(),
                },
            );
            continue;
        }
        if is_punctuation_only(&translation) {
            push_quality_issue(
                &mut issues,
                &mut issue_keys,
                Step5QualityIssue {
                    rule_id: "non_lexical_translation".to_string(),
                    severity: "hard".to_string(),
                    segment_id: segment.segment_id,
                    part_id: index + 1,
                    message: "字幕仅包含标点或无有效文本".to_string(),
                },
            );
            continue;
        }
        if has_tail_ellipsis(&translation) {
            ellipsis_tail_count += 1;
            push_quality_issue(
                &mut issues,
                &mut issue_keys,
                Step5QualityIssue {
                    rule_id: "tail_ellipsis".to_string(),
                    severity: "hard".to_string(),
                    segment_id: segment.segment_id,
                    part_id: index + 1,
                    message: "字幕以省略号结尾，疑似截断".to_string(),
                },
            );
        }
        if looks_like_source_residue(&source, &translation, &request.target_lang) {
            push_quality_issue(
                &mut issues,
                &mut issue_keys,
                Step5QualityIssue {
                    rule_id: "source_residue".to_string(),
                    severity: "hard".to_string(),
                    segment_id: segment.segment_id,
                    part_id: index + 1,
                    message: "译文含大段源语言残留，疑似未翻译".to_string(),
                },
            );
        }

        let source_numbers = extract_numbers(&source);
        if !source_numbers.is_empty() {
            let translation_numbers = extract_numbers(&translation);
            let missing = source_numbers
                .iter()
                .any(|value| !translation_numbers.contains(value));
            if missing {
                numeric_drift_count += 1;
                push_quality_issue(
                    &mut issues,
                    &mut issue_keys,
                    Step5QualityIssue {
                        rule_id: "numeric_drift".to_string(),
                        severity: "hard".to_string(),
                        segment_id: segment.segment_id,
                        part_id: index + 1,
                        message: "数字锚点未保持一致".to_string(),
                    },
                );
            }
        }
        if is_watchability_fragment_issue(&source, &translation, &request.target_lang) {
            push_quality_issue(
                &mut issues,
                &mut issue_keys,
                Step5QualityIssue {
                    rule_id: "watchability_fragment".to_string(),
                    severity: "soft".to_string(),
                    segment_id: segment.segment_id,
                    part_id: index + 1,
                    message: "译文疑似碎片化，影响观看流畅度".to_string(),
                },
            );
        }
    }

    for window in request.segments.windows(2) {
        let current = &window[0];
        let next = &window[1];
        let current_source_numbers = extract_numbers(&current.source);
        let current_translation_numbers = extract_numbers(&current.translation);
        let next_source_numbers = extract_numbers(&next.source);
        if next_source_numbers.is_empty() || current_translation_numbers.is_empty() {
            continue;
        }
        let mut next_only = HashSet::<String>::new();
        for value in next_source_numbers {
            if !current_source_numbers.contains(&value) {
                next_only.insert(value);
            }
        }
        if next_only.is_empty() {
            continue;
        }
        let leaked = next_only
            .iter()
            .any(|value| current_translation_numbers.contains(value));
        if leaked {
            cross_line_leak_count += 1;
            push_quality_issue(
                &mut issues,
                &mut issue_keys,
                Step5QualityIssue {
                    rule_id: "cross_line_leak".to_string(),
                    severity: "hard".to_string(),
                    segment_id: current.segment_id,
                    part_id: 0,
                    message: "当前句疑似提前翻译下一句信息".to_string(),
                },
            );
        }
    }

    let hard_fail_count = issues
        .iter()
        .filter(|issue| issue.severity == "hard")
        .count();
    let segment_total = request.segments.len();
    let long_line_penalty = gt25_count as f64 * 1.2 + gt32_count as f64 * 2.5;
    let hard_penalty = hard_fail_count as f64 * 20.0;
    let soft_penalty = issues
        .iter()
        .filter(|issue| issue.severity == "soft")
        .count() as f64
        * 1.5;
    let mut soft_score = 100.0 - hard_penalty - long_line_penalty - soft_penalty;
    if soft_score < 0.0 {
        soft_score = 0.0;
    }

    Ok(BuildStep6FinalCheckResponse {
        passed: hard_fail_count == 0,
        hard_fail_count,
        soft_score: (soft_score * 10.0).round() / 10.0,
        issue_count: issues.len(),
        issues,
        metrics: Step6FinalCheckMetrics {
            segment_total,
            empty_count,
            ellipsis_tail_count,
            numeric_drift_count,
            cross_line_leak_count,
            gt25_count,
            gt32_count,
        },
    })
}

fn push_quality_issue(
    issues: &mut Vec<Step5QualityIssue>,
    issue_keys: &mut HashSet<String>,
    issue: Step5QualityIssue,
) {
    let key = format!(
        "{}|{}|{}|{}|{}",
        issue.rule_id, issue.severity, issue.segment_id, issue.part_id, issue.message
    );
    if issue_keys.insert(key) {
        issues.push(issue);
    }
}
