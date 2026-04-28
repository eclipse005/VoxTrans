use crate::services::transcribe::WordTokenDto;

use super::text::{
    ends_with_soft_punctuation, ends_with_terminal_punctuation, is_bad_segment_start_word,
    is_dangling_tail_word, is_pronoun_or_auxiliary_start, is_semantic_clause_start,
    normalize_ascii_word,
};
use super::timing::{gap_ms, span_duration_ms};
use super::{MAX_UNPUNCTUATED_DURATION_MS, MIN_SEMANTIC_SEGMENT_WORDS, SOFT_SPLIT_GAP_MS};

pub(super) fn score_absolute_splits(
    splits: &[usize],
    start: usize,
    end: usize,
    desired_parts: usize,
    soft_limit: usize,
) -> f64 {
    if start > end {
        return 1_000_000.0;
    }
    let mut score = 0.0f64;
    let mut cursor = start;
    let total_words = end.saturating_sub(start) + 1;
    let target_len = (total_words as f64 / desired_parts.max(1) as f64).max(1.0);
    for split_after in splits.iter().copied().chain(std::iter::once(end)) {
        if split_after < cursor || split_after > end {
            score += 1_000.0;
            continue;
        }
        let len = split_after.saturating_sub(cursor) + 1;
        score += (len as f64 - target_len).abs();
        if len < MIN_SEMANTIC_SEGMENT_WORDS {
            score += (MIN_SEMANTIC_SEGMENT_WORDS - len) as f64 * 8.0 + 12.0;
        }
        if len > soft_limit {
            score += (len - soft_limit) as f64 * 6.0 + 10.0;
        }
        cursor = split_after + 1;
    }
    score
}

pub(super) fn translation_unit_word_limit_from_span(
    start: usize,
    end: usize,
    desired_parts: usize,
) -> usize {
    let total = end.saturating_sub(start) + 1;
    ((total as f64 / desired_parts.max(1) as f64).ceil() as usize).max(MIN_SEMANTIC_SEGMENT_WORDS)
}

pub(super) fn semantic_boundary_score(
    words: &[WordTokenDto],
    range_start: usize,
    range_end: usize,
    split_after: usize,
    target: usize,
    word_limit: usize,
) -> f64 {
    let left_len = split_after.saturating_sub(range_start) + 1;
    let right_len = range_end.saturating_sub(split_after);
    let mut score = split_after.abs_diff(target) as f64;
    score += semantic_boundary_penalty(words, split_after);
    if left_len < MIN_SEMANTIC_SEGMENT_WORDS {
        score += (MIN_SEMANTIC_SEGMENT_WORDS - left_len) as f64 * 8.0;
    }
    if right_len < MIN_SEMANTIC_SEGMENT_WORDS {
        score += (MIN_SEMANTIC_SEGMENT_WORDS - right_len) as f64 * 8.0;
    }
    if left_len > word_limit {
        score += (left_len - word_limit) as f64 * 2.5;
    }
    if right_len > word_limit {
        score += (right_len - word_limit) as f64 * 1.5;
    }
    score
}

fn semantic_boundary_penalty(words: &[WordTokenDto], split_after: usize) -> f64 {
    let mut penalty = 0.0f64;
    let Some(left) = words.get(split_after) else {
        return 100.0;
    };
    let Some(right) = words.get(split_after + 1) else {
        return 100.0;
    };
    let left_word = normalize_ascii_word(&left.word);
    let right_word = normalize_ascii_word(&right.word);
    let gap = gap_ms(left.end, right.start);

    if gap >= SOFT_SPLIT_GAP_MS {
        penalty -= 3.0;
    }
    if ends_with_terminal_punctuation(&left.word) {
        penalty -= 5.0;
    } else if ends_with_soft_punctuation(&left.word) {
        penalty -= 8.0;
        if is_pronoun_or_auxiliary_start(&right_word) {
            penalty += 8.0;
        }
    }
    if is_semantic_clause_start(&right_word) {
        penalty -= 7.0;
    }
    if is_dangling_tail_word(&left_word) {
        penalty += 8.0;
    }
    if is_bad_segment_start_word(&right_word) {
        penalty += 7.0;
    }
    penalty
}

pub(super) fn semantic_boundary_reason(words: &[WordTokenDto], split_after: usize) -> String {
    let Some(left) = words.get(split_after) else {
        return "candidate".to_string();
    };
    let Some(right) = words.get(split_after + 1) else {
        return "candidate".to_string();
    };
    let right_word = normalize_ascii_word(&right.word);
    let gap = gap_ms(left.end, right.start);
    if gap >= SOFT_SPLIT_GAP_MS {
        "pause".to_string()
    } else if ends_with_terminal_punctuation(&left.word) {
        "terminal_punctuation".to_string()
    } else if ends_with_soft_punctuation(&left.word) {
        "soft_punctuation".to_string()
    } else if is_semantic_clause_start(&right_word) {
        format!("before_{right_word}")
    } else {
        "balanced_length".to_string()
    }
}

pub(super) fn is_structural_boundary(words: &[WordTokenDto], split_after: usize) -> bool {
    let Some(left) = words.get(split_after) else {
        return false;
    };
    let Some(right) = words.get(split_after + 1) else {
        return false;
    };
    let right_word = normalize_ascii_word(&right.word);
    gap_ms(left.end, right.start) >= SOFT_SPLIT_GAP_MS
        || ends_with_terminal_punctuation(&left.word)
        || ends_with_soft_punctuation(&left.word)
        || is_semantic_clause_start(&right_word)
}

pub(super) fn should_refine_semantic_span(
    words: &[WordTokenDto],
    start: usize,
    end: usize,
    word_limit: usize,
) -> bool {
    if !should_split_semantic_span(words, start, end, word_limit) {
        return false;
    }
    let required_boundaries = desired_semantic_part_count(words, start, end, word_limit)
        .saturating_sub(1)
        .max(1);
    semantic_boundary_signal_count(words, start, end) < required_boundaries
}

pub(super) fn should_split_semantic_span(
    words: &[WordTokenDto],
    start: usize,
    end: usize,
    word_limit: usize,
) -> bool {
    if words.is_empty() || start >= words.len() || end >= words.len() || start >= end {
        return false;
    }
    let word_count = end.saturating_sub(start) + 1;
    word_count > word_limit || span_duration_ms(words, start, end) > MAX_UNPUNCTUATED_DURATION_MS
}

fn semantic_boundary_signal_count(words: &[WordTokenDto], start: usize, end: usize) -> usize {
    if words.is_empty() || start >= words.len() || end >= words.len() || start >= end {
        return 0;
    }

    let mut count = 0usize;
    for split_after in start..end {
        let Some(left) = words.get(split_after) else {
            continue;
        };
        let Some(right) = words.get(split_after + 1) else {
            continue;
        };
        let right_word = normalize_ascii_word(&right.word);
        if ends_with_soft_punctuation(&left.word)
            || gap_ms(left.end, right.start) >= SOFT_SPLIT_GAP_MS
            || is_semantic_clause_start(&right_word)
        {
            count += 1;
        }
    }
    count
}

pub(super) fn desired_semantic_part_count(
    words: &[WordTokenDto],
    start: usize,
    end: usize,
    word_limit: usize,
) -> usize {
    if words.is_empty() || start >= words.len() || end >= words.len() || start > end {
        return 1;
    }
    let word_count = end.saturating_sub(start) + 1;
    let word_parts = if word_limit == 0 {
        1
    } else {
        word_count.div_ceil(word_limit).max(1)
    };
    let duration = span_duration_ms(words, start, end);
    let duration_parts = if duration <= MAX_UNPUNCTUATED_DURATION_MS {
        1
    } else {
        duration.div_ceil(MAX_UNPUNCTUATED_DURATION_MS).max(1) as usize
    };
    word_parts.max(duration_parts).max(1)
}

pub(super) fn translation_unit_word_limit(subtitle_max_words_per_segment: u32) -> usize {
    (subtitle_max_words_per_segment.clamp(8, 40) as usize).max(MIN_SEMANTIC_SEGMENT_WORDS)
}
