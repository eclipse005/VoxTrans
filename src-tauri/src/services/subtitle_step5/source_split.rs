use std::collections::HashSet;

use super::constants::{
    FORCE_SPLIT_MARGIN, HARD_MIN_SEGMENT_DURATION_SECONDS, HARD_SPLIT_GAP_SECONDS,
    MIN_READABLE_DURATION_SECONDS, MIN_READABLE_UNITS, MIN_TOKENS_FOR_SOFT_SPLIT,
    SOFT_SPLIT_GAP_SECONDS,
};
use super::language_units::{text_length_units, use_char_units};
use super::source_text::build_source_from_tokens;
use super::split_parts::{boundary_ids_to_ranges, ranges_to_boundary_ids};
use super::text_utils::ends_with_sentence_punctuation;
use super::types::Step5Token;

pub(super) fn hard_pause_boundaries(tokens: &[Step5Token]) -> Vec<usize> {
    if tokens.len() < 2 {
        return Vec::new();
    }
    let mut out = Vec::<usize>::new();
    for index in 0..tokens.len() - 1 {
        let current = &tokens[index];
        let next = &tokens[index + 1];
        let gap = (next.start - current.end).max(0.0);
        if gap >= HARD_SPLIT_GAP_SECONDS {
            out.push(index + 1);
        }
    }
    out
}

pub(super) fn choose_preferred_split_ranges(
    llm_ranges: Vec<(usize, usize)>,
    fallback_ranges: Vec<(usize, usize)>,
    tokens: &[Step5Token],
    source_lang: &str,
    source_limit: f64,
) -> Vec<(usize, usize)> {
    if llm_ranges.is_empty() {
        return fallback_ranges;
    }
    if fallback_ranges.is_empty() {
        return llm_ranges;
    }
    let llm_score = score_split_ranges(&llm_ranges, tokens, source_lang, source_limit);
    let fallback_score = score_split_ranges(&fallback_ranges, tokens, source_lang, source_limit);
    if llm_score <= fallback_score * 1.05 {
        llm_ranges
    } else {
        fallback_ranges
    }
}

fn score_split_ranges(
    ranges: &[(usize, usize)],
    tokens: &[Step5Token],
    source_lang: &str,
    source_limit: f64,
) -> f64 {
    if ranges.is_empty() {
        return 1_000_000.0;
    }
    let mut score = 0.0f64;
    let mut lengths = Vec::<f64>::new();
    for (start, end) in ranges {
        if *start >= tokens.len() || *end >= tokens.len() || end < start {
            score += 1000.0;
            continue;
        }
        let text = build_source_from_tokens(&tokens[*start..=*end]);
        let units = text_length_units(&text, source_lang);
        lengths.push(units);
        if units > source_limit {
            score += 80.0 + (units - source_limit) * 20.0;
        }
        if units < 4.0 {
            score += (4.0 - units) * 25.0 + 20.0;
        }
        if units > source_limit * 1.6 {
            score += 120.0;
        }
    }
    for window in ranges.windows(2) {
        let left = window[0];
        let right = window[1];
        let Some(left_token) = tokens.get(left.1) else {
            continue;
        };
        let Some(right_token) = tokens.get(right.0) else {
            continue;
        };
        let gap = (right_token.start - left_token.end).max(0.0);
        if gap >= SOFT_SPLIT_GAP_SECONDS || ends_with_sentence_punctuation(&left_token.text) {
            score -= 2.0;
        } else {
            score += 4.0;
        }
    }
    if lengths.len() >= 2 {
        let avg = lengths.iter().sum::<f64>() / lengths.len() as f64;
        if avg > 0.0 {
            for len in &lengths {
                let ratio = len / avg;
                if ratio > 2.4 || ratio < 0.35 {
                    score += 16.0;
                }
            }
        }
    }
    score
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

pub(super) fn enforce_source_limit_ranges(
    mut ranges: Vec<(usize, usize)>,
    tokens: &[Step5Token],
    source_lang: &str,
    source_limit: f64,
) -> Vec<(usize, usize)> {
    if ranges.is_empty() || tokens.is_empty() || source_limit <= 0.0 {
        return ranges;
    }
    let hard_limit = (source_limit * FORCE_SPLIT_MARGIN).max(1.0);
    let max_rounds = tokens.len().max(1);

    for _ in 0..max_rounds {
        let mut changed = false;
        let mut next_ranges = Vec::<(usize, usize)>::new();
        for (start, end) in ranges.iter().copied() {
            if start >= tokens.len() || end >= tokens.len() || end < start {
                continue;
            }
            let units = tokens[start..=end]
                .iter()
                .map(|token| text_length_units(&token.text, source_lang))
                .sum::<f64>();
            if units <= hard_limit || end == start {
                next_ranges.push((start, end));
                continue;
            }
            if let Some(split_after) =
                pick_force_split_after(start, end, tokens, source_lang, source_limit)
            {
                next_ranges.push((start, split_after));
                next_ranges.push((split_after + 1, end));
                changed = true;
            } else {
                next_ranges.push((start, end));
            }
        }
        ranges = next_ranges;
        if !changed {
            break;
        }
    }

    ranges
}

fn pick_force_split_after(
    start: usize,
    end: usize,
    tokens: &[Step5Token],
    source_lang: &str,
    source_limit: f64,
) -> Option<usize> {
    if end <= start {
        return None;
    }
    let mut total_units = 0.0f64;
    for token in &tokens[start..=end] {
        total_units += text_length_units(&token.text, source_lang);
    }
    if total_units <= source_limit {
        return None;
    }

    let target = (total_units / 2.0).min(source_limit);
    let mut best = None::<(usize, f64)>;
    let mut left_units = 0.0f64;
    for idx in start..end {
        left_units += text_length_units(&tokens[idx].text, source_lang);
        let right_units = (total_units - left_units).max(0.0);
        if left_units <= 0.0 || right_units <= 0.0 {
            continue;
        }
        let mut penalty = (left_units - target).abs() + (right_units - target).abs();
        if left_units > source_limit {
            penalty += (left_units - source_limit) * 8.0;
        }
        if right_units > source_limit {
            penalty += (right_units - source_limit) * 8.0;
        }
        let gap = (tokens[idx + 1].start - tokens[idx].end).max(0.0);
        if gap >= SOFT_SPLIT_GAP_SECONDS || ends_with_sentence_punctuation(&tokens[idx].text) {
            penalty -= 0.8;
        }
        match best {
            Some((_best_idx, best_penalty)) if best_penalty <= penalty => {}
            _ => {
                best = Some((idx, penalty));
            }
        }
    }
    best.map(|(idx, _)| idx)
}

pub(super) fn normalize_split_boundaries(
    candidate_boundaries: &[usize],
    token_count: usize,
    mandatory_boundaries: &[usize],
    fallback_boundaries: &[usize],
    min_parts: usize,
) -> Vec<usize> {
    if token_count <= 1 {
        return Vec::new();
    }
    let mut boundaries = candidate_boundaries
        .iter()
        .copied()
        .filter(|id| *id >= 1 && *id < token_count)
        .collect::<Vec<_>>();
    boundaries.extend(
        mandatory_boundaries
            .iter()
            .copied()
            .filter(|id| *id >= 1 && *id < token_count),
    );
    boundaries.sort_unstable();
    boundaries.dedup();

    let mut mandatory_sorted = mandatory_boundaries
        .iter()
        .copied()
        .filter(|id| *id >= 1 && *id < token_count)
        .collect::<Vec<_>>();
    mandatory_sorted.sort_unstable();
    mandatory_sorted.dedup();
    let mandatory_set = mandatory_sorted.iter().copied().collect::<HashSet<_>>();

    let required_boundaries = min_parts.saturating_sub(1);
    if boundaries.len() < required_boundaries {
        for id in fallback_boundaries
            .iter()
            .copied()
            .filter(|id| *id >= 1 && *id < token_count)
        {
            if boundaries.len() >= required_boundaries {
                break;
            }
            if !boundaries.contains(&id) {
                boundaries.push(id);
            }
        }
    }
    boundaries.sort_unstable();
    boundaries.dedup();

    let max_parts = (required_boundaries + 3).max(min_parts).min(token_count);
    let max_boundaries = max_parts.saturating_sub(1);
    if boundaries.len() > max_boundaries {
        let mut pruned = mandatory_sorted.clone();
        for id in fallback_boundaries
            .iter()
            .copied()
            .filter(|id| *id >= 1 && *id < token_count)
        {
            if pruned.len() >= max_boundaries {
                break;
            }
            if !pruned.contains(&id) {
                pruned.push(id);
            }
        }
        for id in boundaries.iter().copied() {
            if pruned.len() >= max_boundaries {
                break;
            }
            if !mandatory_set.contains(&id) && !pruned.contains(&id) {
                pruned.push(id);
            }
        }
        boundaries = pruned;
        boundaries.sort_unstable();
        boundaries.dedup();
    }

    if boundaries.len() < required_boundaries {
        let mut ranges = boundary_ids_to_ranges(&boundaries, token_count);
        let fake_tokens = (0..token_count)
            .map(|idx| Step5Token {
                text: idx.to_string(),
                start: idx as f64,
                end: idx as f64,
            })
            .collect::<Vec<_>>();
        ensure_min_split_ranges(&mut ranges, min_parts, &fake_tokens, "en");
        boundaries = ranges_to_boundary_ids(&ranges);
        for id in mandatory_sorted {
            if !boundaries.contains(&id) {
                boundaries.push(id);
            }
        }
        boundaries.sort_unstable();
        boundaries.dedup();
    }

    boundaries
}

pub(super) fn map_source_parts_to_boundaries(
    source_parts: &[String],
    tokens: &[Step5Token],
    source_lang: &str,
) -> Vec<usize> {
    if source_parts.len() <= 1 || tokens.len() <= 1 {
        return Vec::new();
    }
    let token_units = tokens
        .iter()
        .map(|token| text_length_units(&token.text, source_lang).max(0.5))
        .collect::<Vec<_>>();
    let mut prefix_units = Vec::<f64>::with_capacity(token_units.len() + 1);
    prefix_units.push(0.0);
    for unit in &token_units {
        let prev = *prefix_units.last().unwrap_or(&0.0);
        prefix_units.push(prev + *unit);
    }

    let mut part_units = source_parts
        .iter()
        .map(|part| text_length_units(part, source_lang).max(1.0))
        .collect::<Vec<_>>();
    let total_part_units = part_units.iter().sum::<f64>();
    let total_token_units = *prefix_units.last().unwrap_or(&0.0);
    if total_part_units > 0.0 && total_token_units > 0.0 {
        let scale = total_token_units / total_part_units;
        for unit in &mut part_units {
            *unit *= scale;
        }
    }

    let mut boundaries = Vec::<usize>::new();
    let mut start = 0usize;
    let mut consumed_target = 0.0f64;
    let boundary_count = source_parts.len().saturating_sub(1);
    for boundary_idx in 0..boundary_count {
        consumed_target += part_units
            .get(boundary_idx)
            .copied()
            .unwrap_or(1.0)
            .max(0.5);
        let remaining_boundaries = boundary_count.saturating_sub(boundary_idx + 1);
        let min_boundary = start.saturating_add(1);
        let max_boundary = tokens.len().saturating_sub(remaining_boundaries + 1);
        if min_boundary > max_boundary {
            break;
        }

        let mut best_boundary = min_boundary;
        let mut best_score = f64::MAX;
        for boundary in min_boundary..=max_boundary {
            let consumed_units = prefix_units.get(boundary).copied().unwrap_or(0.0);
            let mut score = (consumed_units - consumed_target).abs();

            if let (Some(left), Some(right)) =
                (tokens.get(boundary.saturating_sub(1)), tokens.get(boundary))
            {
                let gap = (right.start - left.end).max(0.0);
                if gap >= SOFT_SPLIT_GAP_SECONDS || ends_with_sentence_punctuation(&left.text) {
                    score -= 0.8;
                } else {
                    score += 0.5;
                }
            }

            let left_units = consumed_units - prefix_units.get(start).copied().unwrap_or(0.0);
            let right_units = total_token_units - consumed_units;
            if left_units < 1.5 {
                score += 3.0;
            }
            if right_units < 1.5 {
                score += 3.0;
            }

            if score < best_score {
                best_score = score;
                best_boundary = boundary;
            }
        }
        boundaries.push(best_boundary);
        start = best_boundary;
    }
    boundaries.sort_unstable();
    boundaries.dedup();
    boundaries
}

pub(super) fn split_token_ranges(
    tokens: &[Step5Token],
    source_lang: &str,
    source_limit: f64,
    target_limit: f64,
    source_units: f64,
    target_units: f64,
) -> Vec<(usize, usize)> {
    if tokens.is_empty() {
        return Vec::new();
    }
    if tokens.len() == 1 {
        return vec![(0, 0)];
    }

    let desired_parts = desired_split_parts(source_units, source_limit, target_units, target_limit);
    let dynamic_soft_limit = if desired_parts <= 1 {
        source_limit
    } else {
        (source_units / desired_parts as f64)
            .max(1.0)
            .min(source_limit.max(1.0))
    };

    let mut out = Vec::<(usize, usize)>::new();
    let mut chunk_start = 0usize;
    let mut chunk_units = 0.0f64;

    for index in 0..tokens.len() - 1 {
        let current = &tokens[index];
        let next = &tokens[index + 1];
        chunk_units += text_length_units(&current.text, source_lang);
        let current_len = index + 1 - chunk_start;
        let gap = (next.start - current.end).max(0.0);
        let hard_split = gap >= HARD_SPLIT_GAP_SECONDS;
        let can_soft_split = current_len >= MIN_TOKENS_FOR_SOFT_SPLIT
            && chunk_units >= dynamic_soft_limit
            && (gap >= SOFT_SPLIT_GAP_SECONDS || ends_with_sentence_punctuation(&current.text));
        let force_split = chunk_units >= source_limit * FORCE_SPLIT_MARGIN
            && current_len >= (MIN_TOKENS_FOR_SOFT_SPLIT / 2).max(2);

        if hard_split || can_soft_split || force_split {
            out.push((chunk_start, index));
            chunk_start = index + 1;
            chunk_units = 0.0;
        }
    }
    out.push((chunk_start, tokens.len() - 1));

    let mut out = out
        .into_iter()
        .filter(|(start, end)| end >= start)
        .collect::<Vec<_>>();
    ensure_min_split_ranges(&mut out, desired_parts, tokens, source_lang);
    out
}

pub(super) fn desired_split_parts(
    source_units: f64,
    source_limit: f64,
    target_units: f64,
    target_limit: f64,
) -> usize {
    let source_parts = if source_limit <= 0.0 {
        1usize
    } else {
        (source_units / source_limit).ceil().max(1.0) as usize
    };
    let target_parts = if target_limit <= 0.0 {
        1usize
    } else {
        (target_units / target_limit).ceil().max(1.0) as usize
    };
    source_parts.max(target_parts).max(1)
}

fn ensure_min_split_ranges(
    ranges: &mut Vec<(usize, usize)>,
    desired_parts: usize,
    tokens: &[Step5Token],
    source_lang: &str,
) {
    if desired_parts <= 1 || ranges.is_empty() {
        return;
    }
    while ranges.len() < desired_parts {
        let mut best_index = None::<usize>;
        let mut best_score = 0.0f64;
        for (idx, (start, end)) in ranges.iter().enumerate() {
            if end <= start {
                continue;
            }
            let unit_len = tokens[*start..=*end]
                .iter()
                .map(|token| text_length_units(&token.text, source_lang))
                .sum::<f64>();
            if unit_len > best_score {
                best_score = unit_len;
                best_index = Some(idx);
            }
        }
        let Some(idx) = best_index else {
            break;
        };
        let (start, end) = ranges[idx];
        if end <= start {
            break;
        }
        let mid = start + (end - start) / 2;
        if mid < start || mid >= end {
            break;
        }
        ranges[idx] = (start, mid);
        ranges.insert(idx + 1, (mid + 1, end));
    }
}
