use std::collections::HashSet;

use super::constants::SOFT_SPLIT_GAP_SECONDS;
use super::language_units::text_length_units;
use super::responses::compact_for_split_match;
use super::source_text::build_source_from_tokens;
use super::split_parts::{boundary_ids_to_ranges, ranges_to_boundary_ids};
use super::text_utils::ends_with_sentence_punctuation;
use super::types::Step5Token;

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
    if let Some(boundaries) = map_source_parts_to_exact_boundaries(source_parts, tokens) {
        return boundaries;
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

fn map_source_parts_to_exact_boundaries(
    source_parts: &[String],
    tokens: &[Step5Token],
) -> Option<Vec<usize>> {
    let full = compact_for_split_match(&build_source_from_tokens(tokens));
    let parts_joined = compact_for_split_match(&source_parts.join(""));
    if full.is_empty() || full != parts_joined {
        return None;
    }

    let mut out = Vec::<usize>::new();
    let mut consumed = String::new();
    for part in source_parts
        .iter()
        .take(source_parts.len().saturating_sub(1))
    {
        consumed.push_str(&compact_for_split_match(part));
        if consumed.is_empty() {
            return None;
        }
        let mut matched = None::<usize>;
        for boundary in 1..tokens.len() {
            let prefix = compact_for_split_match(&build_source_from_tokens(&tokens[..boundary]));
            if prefix == consumed {
                matched = Some(boundary);
                break;
            }
        }
        let boundary = matched?;
        out.push(boundary);
    }
    out.sort_unstable();
    out.dedup();
    Some(out)
}

pub(super) fn ensure_min_split_ranges(
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
