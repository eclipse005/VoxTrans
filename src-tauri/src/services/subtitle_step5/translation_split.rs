use super::clauses::split_clauses;
use super::language_units::count_word_units;
use super::quality::{
    has_empty_or_duplicated_long_line, line_fragment_penalty, split_line_quality_score,
};
use super::text_utils::normalize_inline_text;
use super::types::Step5SplitPart;

pub(super) fn heuristic_split_translation(
    text: &str,
    expected_count: usize,
    part_sources: Option<&[Step5SplitPart]>,
) -> Vec<String> {
    let normalized = normalize_inline_text(text);
    if expected_count <= 1 {
        return vec![normalized];
    }
    if normalized.is_empty() {
        return vec![String::new(); expected_count];
    }

    let clauses = split_clauses(&normalized);
    if clauses.is_empty() {
        return vec![normalized];
    }
    let mut candidates = Vec::<Vec<String>>::new();
    let clause_bucketed = bucket_split_clauses(&clauses, expected_count);
    if !clause_bucketed.is_empty() {
        candidates.push(clause_bucketed);
    }

    let weights = part_sources
        .filter(|parts| parts.len() == expected_count)
        .map(weighted_source_units_from_parts)
        .unwrap_or_else(|| vec![1.0; expected_count]);

    if let Some(clause_weighted) =
        split_translation_by_clause_weights(&clauses, expected_count, &weights)
    {
        candidates.push(clause_weighted);
    }
    candidates.push(split_translation_evenly_by_weights(
        &normalized,
        expected_count,
        &weights,
    ));

    let mut best = candidates
        .first()
        .cloned()
        .unwrap_or_else(|| vec![normalized.clone(); expected_count]);
    let mut best_score = split_line_quality_score(&best);
    for candidate in candidates.into_iter().skip(1) {
        if candidate.len() != expected_count {
            continue;
        }
        let score = split_line_quality_score(&candidate);
        if score > best_score {
            best = candidate;
            best_score = score;
        }
    }
    if has_empty_or_duplicated_long_line(&best) {
        return split_translation_evenly_by_weights(&normalized, expected_count, &weights);
    }
    best
}

fn bucket_split_clauses(clauses: &[String], expected_count: usize) -> Vec<String> {
    if expected_count == 0 {
        return Vec::new();
    }
    if clauses.is_empty() {
        return vec![String::new(); expected_count];
    }
    let clauses_total = clauses.len().max(1);
    let mut out = vec![String::new(); expected_count];
    for (index, clause) in clauses.iter().enumerate() {
        let bucket = index * expected_count / clauses_total;
        let target = bucket.min(expected_count - 1);
        if out[target].is_empty() {
            out[target] = clause.clone();
        } else {
            out[target].push(' ');
            out[target].push_str(clause);
        }
    }
    out.into_iter()
        .map(|line| normalize_inline_text(&line))
        .collect()
}

fn split_translation_by_clause_weights(
    clauses: &[String],
    expected_count: usize,
    weights: &[f64],
) -> Option<Vec<String>> {
    if expected_count == 0 || clauses.is_empty() || clauses.len() < expected_count {
        return None;
    }
    let mut normalized_weights = if weights.len() == expected_count {
        weights.to_vec()
    } else {
        vec![1.0; expected_count]
    };
    for value in &mut normalized_weights {
        if !value.is_finite() || *value <= 0.0 {
            *value = 1.0;
        }
    }
    let weight_total = normalized_weights.iter().sum::<f64>().max(1.0);
    let clause_units = clauses
        .iter()
        .map(|clause| count_word_units(clause).max(1) as f64)
        .collect::<Vec<_>>();
    let units_total = clause_units.iter().sum::<f64>().max(expected_count as f64);
    let target_units = normalized_weights
        .iter()
        .map(|weight| units_total * (*weight / weight_total))
        .collect::<Vec<_>>();

    let n = clauses.len();
    let m = expected_count;
    let mut prefix = vec![0.0f64; n + 1];
    for index in 0..n {
        prefix[index + 1] = prefix[index] + clause_units[index];
    }
    let neg_inf = -1.0e18f64;
    let mut dp = vec![vec![neg_inf; n + 1]; m + 1];
    let mut prev = vec![vec![usize::MAX; n + 1]; m + 1];
    dp[0][0] = 0.0;

    for part in 1..=m {
        for end in part..=n {
            let min_start = part - 1;
            let max_start = end - 1;
            for start in min_start..=max_start {
                if dp[part - 1][start] <= neg_inf / 2.0 {
                    continue;
                }
                let segment_units = prefix[end] - prefix[start];
                let segment_text = clauses[start..end].join(" ");
                let segment_text = normalize_inline_text(&segment_text);
                let mut segment_score = -((segment_units - target_units[part - 1]).abs() * 4.0);
                segment_score -= line_fragment_penalty(&segment_text) as f64 * 1.5;
                if segment_text.is_empty() {
                    segment_score -= 40.0;
                }
                let candidate = dp[part - 1][start] + segment_score;
                if candidate > dp[part][end] {
                    dp[part][end] = candidate;
                    prev[part][end] = start;
                }
            }
        }
    }
    if dp[m][n] <= neg_inf / 2.0 {
        return None;
    }
    let mut boundaries = Vec::<(usize, usize)>::with_capacity(m);
    let mut part = m;
    let mut end = n;
    while part > 0 {
        let start = prev[part][end];
        if start == usize::MAX || start >= end {
            return None;
        }
        boundaries.push((start, end));
        end = start;
        part -= 1;
    }
    boundaries.reverse();
    if boundaries.len() != expected_count {
        return None;
    }
    let out = boundaries
        .into_iter()
        .map(|(start, end)| normalize_inline_text(&clauses[start..end].join(" ")))
        .collect::<Vec<_>>();
    Some(out)
}

fn weighted_source_units_from_parts(parts: &[Step5SplitPart]) -> Vec<f64> {
    let mut weights = parts
        .iter()
        .map(|part| count_word_units(&part.source).max(1) as f64)
        .collect::<Vec<_>>();
    if weights.is_empty() {
        return weights;
    }
    if weights
        .iter()
        .all(|value| *value <= 0.0 || !value.is_finite())
    {
        weights.fill(1.0);
    }
    weights
}

pub(super) fn split_translation_evenly_by_weights(
    text: &str,
    expected_count: usize,
    weights: &[f64],
) -> Vec<String> {
    if expected_count == 0 {
        return Vec::new();
    }
    let normalized = normalize_inline_text(text);
    if normalized.is_empty() {
        return vec![String::new(); expected_count];
    }

    let words = normalized
        .split_whitespace()
        .map(|value| value.to_string())
        .collect::<Vec<_>>();
    let (tokens, join_with_space) = if words.len() >= expected_count {
        (words, true)
    } else {
        let chars = normalized
            .chars()
            .filter(|ch| !ch.is_whitespace())
            .map(|ch| ch.to_string())
            .collect::<Vec<_>>();
        if chars.is_empty() {
            (words, true)
        } else {
            (chars, false)
        }
    };

    let token_total = tokens.len();
    if token_total == 0 {
        return vec![String::new(); expected_count];
    }

    if token_total < expected_count {
        let mut out = vec![String::new(); expected_count];
        for index in 0..token_total {
            out[index] = tokens[index].clone();
        }
        return out
            .into_iter()
            .map(|line| normalize_inline_text(&line))
            .collect();
    }

    let mut normalized_weights = if weights.len() == expected_count {
        weights.to_vec()
    } else {
        vec![1.0; expected_count]
    };
    for value in &mut normalized_weights {
        if !value.is_finite() || *value <= 0.0 {
            *value = 1.0;
        }
    }

    let mut remaining_tokens = token_total;
    let mut remaining_weight = normalized_weights.iter().sum::<f64>().max(1.0);
    let mut start = 0usize;
    let mut out = vec![String::new(); expected_count];
    for index in 0..expected_count {
        let remaining_slots = expected_count - index;
        let take = if remaining_slots <= 1 {
            remaining_tokens
        } else {
            let weight = normalized_weights[index];
            let ideal = ((remaining_tokens as f64) * (weight / remaining_weight)).round() as usize;
            let min_take = 1usize;
            let max_take = remaining_tokens.saturating_sub(remaining_slots - 1);
            ideal.clamp(min_take, max_take.max(min_take))
        };
        let end = start + take.min(token_total.saturating_sub(start));
        let text = if join_with_space {
            tokens[start..end].join(" ")
        } else {
            tokens[start..end].join("")
        };
        out[index] = normalize_inline_text(&text);
        start = end;
        remaining_tokens = remaining_tokens.saturating_sub(take);
        remaining_weight = (remaining_weight - normalized_weights[index]).max(1.0);
    }
    out
}
