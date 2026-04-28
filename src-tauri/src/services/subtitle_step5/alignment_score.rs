use std::collections::HashSet;

use super::language_units::{count_word_units, text_length_units};
use super::numbers::extract_numbers;
use super::quality::split_line_quality_score;
use super::source_residue::looks_like_source_residue;
use super::text_utils::normalize_inline_text;
use super::translation_candidate::{has_tail_ellipsis, leading_number_anchor};
use super::types::Step5SplitParent;

pub(super) fn choose_better_alignment(
    parent: &Step5SplitParent,
    aligned_lines: &[String],
    fallback_lines: &[String],
    target_lang: &str,
) -> Vec<String> {
    let aligned_score = alignment_candidate_score(parent, aligned_lines, target_lang);
    let fallback_score = alignment_candidate_score(parent, fallback_lines, target_lang);
    if fallback_score > aligned_score + 2 {
        return fallback_lines.to_vec();
    }
    aligned_lines.to_vec()
}

fn alignment_candidate_score(
    parent: &Step5SplitParent,
    lines: &[String],
    target_lang: &str,
) -> i64 {
    let mut score = split_line_quality_score(lines);
    for (index, part) in parent.parts.iter().enumerate() {
        let line = lines
            .get(index)
            .map(|value| normalize_inline_text(value))
            .unwrap_or_default();
        if line.is_empty() {
            score -= 40;
            continue;
        }
        if looks_like_source_residue(&part.source, &line, target_lang) {
            score -= 24;
        }
        if has_tail_ellipsis(&line) {
            score -= 16;
        }
        let source_numbers = extract_numbers(&part.source);
        if !source_numbers.is_empty() {
            let line_numbers = extract_numbers(&line);
            let missing = source_numbers
                .iter()
                .filter(|value| !line_numbers.contains(*value))
                .count();
            score -= (missing as i64) * 14;
        }
        if let Some(line_leading_number) = leading_number_anchor(&line) {
            let source_leading_number = leading_number_anchor(&part.source);
            let source_matches_leading = source_leading_number
                .as_ref()
                .map(|value| value == &line_leading_number)
                .unwrap_or(false);
            if !source_matches_leading {
                score -= 12;
            }
        }
        let source_units = count_word_units(&part.source) as f64;
        let line_units = text_length_units(&line, target_lang);
        if source_units >= 8.0 && line_units <= 5.0 {
            score -= 14;
        } else if source_units >= 6.0 && line_units <= 4.0 {
            score -= 8;
        }
    }
    if parent.parts.len() >= 2 {
        for index in 0..(parent.parts.len() - 1) {
            let current_source_numbers = extract_numbers(&parent.parts[index].source);
            let next_source_numbers = extract_numbers(&parent.parts[index + 1].source);
            if next_source_numbers.is_empty() {
                continue;
            }
            let current_translation_numbers = lines
                .get(index)
                .map(|line| extract_numbers(line))
                .unwrap_or_default();
            if current_translation_numbers.is_empty() {
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
                score -= 40;
            }
        }
    }
    score
}
