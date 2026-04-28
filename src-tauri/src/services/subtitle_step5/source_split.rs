use super::constants::{
    FORCE_SPLIT_MARGIN, HARD_SPLIT_GAP_SECONDS, MIN_TOKENS_FOR_SOFT_SPLIT, SOFT_SPLIT_GAP_SECONDS,
};
use super::language_units::text_length_units;
use super::source_split_boundaries::ensure_min_split_ranges;
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
