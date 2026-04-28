use std::collections::HashSet;

use super::constants::{
    HARD_MIN_SEGMENT_DURATION_SECONDS, MIN_READABLE_DURATION_SECONDS, MIN_READABLE_UNITS,
};
use super::language_units::{text_length_units, use_char_units};
use super::source_split::enforce_source_limit_ranges;
use super::text_utils::ends_with_sentence_punctuation;
use super::types::Step5Token;

pub(super) fn finalize_readable_source_ranges(
    mut ranges: Vec<(usize, usize)>,
    tokens: &[Step5Token],
    source_lang: &str,
    source_limit: f64,
    mandatory_boundaries: &[usize],
) -> Vec<(usize, usize)> {
    ranges = merge_tiny_ranges_for_readability(
        ranges,
        tokens,
        source_lang,
        source_limit,
        mandatory_boundaries,
    );
    ranges = rebalance_dangling_tail_tokens(
        ranges,
        tokens,
        source_lang,
        source_limit,
        mandatory_boundaries,
    );
    ranges = enforce_source_limit_ranges(ranges, tokens, source_lang, source_limit);
    ranges = merge_tiny_ranges_for_readability(
        ranges,
        tokens,
        source_lang,
        source_limit,
        mandatory_boundaries,
    );
    rebalance_dangling_tail_tokens(
        ranges,
        tokens,
        source_lang,
        source_limit,
        mandatory_boundaries,
    )
}

pub(super) fn merge_tiny_ranges_for_readability(
    mut ranges: Vec<(usize, usize)>,
    tokens: &[Step5Token],
    source_lang: &str,
    source_limit: f64,
    mandatory_boundaries: &[usize],
) -> Vec<(usize, usize)> {
    if ranges.len() <= 1 || tokens.is_empty() {
        return ranges;
    }

    let mandatory_set = mandatory_boundaries.iter().copied().collect::<HashSet<_>>();

    let mut changed = true;
    while changed && ranges.len() > 1 {
        changed = false;
        let mut idx = 0usize;
        while idx < ranges.len() {
            let (start, end) = ranges[idx];
            if start >= tokens.len() || end >= tokens.len() || end < start {
                idx += 1;
                continue;
            }

            let units = tokens[start..=end]
                .iter()
                .map(|token| text_length_units(&token.text, source_lang))
                .sum::<f64>();
            let duration = segment_duration_seconds(tokens, start, end);
            let too_short = duration < HARD_MIN_SEGMENT_DURATION_SECONDS
                || (units < MIN_READABLE_UNITS && duration < MIN_READABLE_DURATION_SECONDS);
            if !too_short {
                idx += 1;
                continue;
            }

            let left_boundary = if idx > 0 { Some(start) } else { None };
            let right_boundary = if idx + 1 < ranges.len() {
                Some(end + 1)
            } else {
                None
            };
            let can_merge_left = left_boundary
                .map(|b| !mandatory_set.contains(&b))
                .unwrap_or(false);
            let can_merge_right = right_boundary
                .map(|b| !mandatory_set.contains(&b))
                .unwrap_or(false);

            let mut merged = false;
            if can_merge_left && can_merge_right {
                let left_score = merge_penalty_with_neighbor(
                    ranges[idx - 1],
                    ranges[idx],
                    tokens,
                    source_lang,
                    source_limit,
                );
                let right_score = merge_penalty_with_neighbor(
                    ranges[idx],
                    ranges[idx + 1],
                    tokens,
                    source_lang,
                    source_limit,
                );
                if left_score <= right_score {
                    ranges[idx - 1] = (ranges[idx - 1].0, ranges[idx].1);
                    ranges.remove(idx);
                } else {
                    ranges[idx + 1] = (ranges[idx].0, ranges[idx + 1].1);
                    ranges.remove(idx);
                }
                merged = true;
            } else if can_merge_left {
                ranges[idx - 1] = (ranges[idx - 1].0, ranges[idx].1);
                ranges.remove(idx);
                merged = true;
            } else if can_merge_right {
                ranges[idx + 1] = (ranges[idx].0, ranges[idx + 1].1);
                ranges.remove(idx);
                merged = true;
            }

            if merged {
                changed = true;
                break;
            }
            idx += 1;
        }
    }

    ranges
}

pub(super) fn rebalance_dangling_tail_tokens(
    mut ranges: Vec<(usize, usize)>,
    tokens: &[Step5Token],
    source_lang: &str,
    source_limit: f64,
    mandatory_boundaries: &[usize],
) -> Vec<(usize, usize)> {
    if ranges.len() <= 1 || tokens.is_empty() {
        return ranges;
    }

    let probe_text = tokens
        .iter()
        .take(24)
        .map(|token| token.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    if use_char_units(source_lang, &probe_text) {
        return ranges;
    }

    let mandatory_set = mandatory_boundaries.iter().copied().collect::<HashSet<_>>();
    let mut changed = true;
    while changed {
        changed = false;
        for index in 0..ranges.len().saturating_sub(1) {
            let (left_start, left_end) = ranges[index];
            let (right_start, right_end) = ranges[index + 1];
            if left_end + 1 != right_start {
                continue;
            }
            if mandatory_set.contains(&right_start) {
                continue;
            }
            let move_count = trailing_dangling_token_count(tokens, left_start, left_end);
            if move_count == 0 || left_end < left_start + move_count {
                continue;
            }
            let new_left_end = left_end - move_count;
            let new_right_start = right_start - move_count;
            if new_right_start > right_end || new_left_end < left_start {
                continue;
            }

            let left_units = tokens[left_start..=new_left_end]
                .iter()
                .map(|token| text_length_units(&token.text, source_lang))
                .sum::<f64>();
            if left_units < 2.0 {
                continue;
            }
            let right_units = tokens[new_right_start..=right_end]
                .iter()
                .map(|token| text_length_units(&token.text, source_lang))
                .sum::<f64>();
            if right_units > source_limit * 1.45 {
                continue;
            }

            ranges[index] = (left_start, new_left_end);
            ranges[index + 1] = (new_right_start, right_end);
            changed = true;
        }
    }

    ranges
}

fn trailing_dangling_token_count(tokens: &[Step5Token], start: usize, end: usize) -> usize {
    if end <= start {
        return 0;
    }
    let Some(last_token) = tokens.get(end) else {
        return 0;
    };
    if ends_with_sentence_punctuation(&last_token.text) {
        return 0;
    }
    let last_word = normalize_ascii_token_word(&last_token.text);
    if last_word.is_empty() {
        return 0;
    }

    let prev_word = end
        .checked_sub(1)
        .and_then(|idx| tokens.get(idx))
        .map(|token| normalize_ascii_token_word(&token.text))
        .unwrap_or_default();

    if is_dangling_tail_word(&last_word) {
        if !prev_word.is_empty() && is_dangling_tail_word(&prev_word) {
            return 2;
        }
        return 1;
    }
    if prev_word == "to" && looks_like_content_word(&last_word) {
        return 2;
    }
    0
}

fn normalize_ascii_token_word(raw: &str) -> String {
    raw.chars()
        .filter(|ch| ch.is_ascii_alphabetic())
        .map(|ch| ch.to_ascii_lowercase())
        .collect::<String>()
}

fn looks_like_content_word(word: &str) -> bool {
    word.len() >= 2 && !is_dangling_tail_word(word)
}

fn is_dangling_tail_word(word: &str) -> bool {
    matches!(
        word,
        "a" | "an"
            | "and"
            | "as"
            | "at"
            | "because"
            | "before"
            | "but"
            | "by"
            | "for"
            | "from"
            | "he"
            | "her"
            | "his"
            | "i"
            | "if"
            | "in"
            | "into"
            | "it"
            | "its"
            | "my"
            | "of"
            | "on"
            | "or"
            | "our"
            | "she"
            | "so"
            | "that"
            | "the"
            | "their"
            | "them"
            | "then"
            | "these"
            | "they"
            | "this"
            | "those"
            | "to"
            | "we"
            | "when"
            | "where"
            | "which"
            | "while"
            | "who"
            | "with"
            | "you"
            | "your"
    )
}

fn segment_duration_seconds(tokens: &[Step5Token], start: usize, end: usize) -> f64 {
    let start_time = tokens.get(start).map(|t| t.start).unwrap_or(0.0);
    let end_time = tokens.get(end).map(|t| t.end).unwrap_or(start_time);
    (end_time - start_time).max(0.0)
}

fn merge_penalty_with_neighbor(
    left: (usize, usize),
    right: (usize, usize),
    tokens: &[Step5Token],
    source_lang: &str,
    source_limit: f64,
) -> f64 {
    let start = left.0.min(right.0);
    let end = left.1.max(right.1);
    if start >= tokens.len() || end >= tokens.len() || end < start {
        return 1_000_000.0;
    }
    let units = tokens[start..=end]
        .iter()
        .map(|token| text_length_units(&token.text, source_lang))
        .sum::<f64>();
    if units <= source_limit {
        return units;
    }
    units + (units - source_limit) * 20.0
}
