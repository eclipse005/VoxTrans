use std::collections::HashSet;

use super::constants::{
    HARD_MIN_SEGMENT_DURATION_SECONDS, MIN_READABLE_DURATION_SECONDS, MIN_READABLE_UNITS,
};
use super::language_units::text_length_units;
use super::types::Step5Token;

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
