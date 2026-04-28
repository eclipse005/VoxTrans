use super::constants::MIN_READABLE_DURATION_SECONDS;
use super::language_units::text_length_units;
use super::source_residue::looks_like_source_residue;
use super::source_text::build_source_from_tokens;
use super::text_utils::normalize_inline_text;
use super::translation_candidate::is_unusable_translation;
use super::translation_split::split_translation_evenly_by_weights;
use super::types::{Step5FinalSegment, Step5Token};

pub(super) fn split_watchability_overlong_segments(
    segments: &mut Vec<Step5FinalSegment>,
    split_trigger: f64,
    target_lang: &str,
) {
    if segments.is_empty() || split_trigger <= 0.0 {
        return;
    }

    let mut split_segments = Vec::<Step5FinalSegment>::new();
    for segment in segments.drain(..) {
        split_segments.push(segment);
    }

    let mut output = Vec::<Step5FinalSegment>::new();
    let mut work_queue = split_segments;
    while let Some(segment) = work_queue.pop() {
        let target_len = text_length_units(&segment.translation, target_lang);
        if target_len <= split_trigger {
            output.push(segment);
            continue;
        }

        let Some((left, right)) = split_long_final_segment_for_watchability(&segment, target_lang)
        else {
            output.push(segment);
            continue;
        };

        if !is_safe_watchability_split_part(&left, target_lang)
            || !is_safe_watchability_split_part(&right, target_lang)
        {
            output.push(segment);
            continue;
        }

        let left_len = text_length_units(&left.translation, target_lang);
        let right_len = text_length_units(&right.translation, target_lang);
        if left_len <= 0.0 || right_len <= 0.0 {
            output.push(left);
            output.push(right);
            continue;
        }

        if left_len > split_trigger || right_len > split_trigger {
            work_queue.push(left);
            work_queue.push(right);
        } else {
            work_queue.push(right);
            work_queue.push(left);
        }
    }

    output.sort_by(|a, b| a.start.total_cmp(&b.start));
    for (index, segment) in output.iter_mut().enumerate() {
        segment.segment_id = index + 1;
    }
    *segments = output;
}

fn is_safe_watchability_split_part(segment: &Step5FinalSegment, target_lang: &str) -> bool {
    let translation = normalize_inline_text(&segment.translation);
    if translation.is_empty() || is_unusable_translation(&translation) {
        return false;
    }
    !looks_like_source_residue(&segment.source, &translation, target_lang)
}

fn split_long_final_segment_for_watchability(
    segment: &Step5FinalSegment,
    target_lang: &str,
) -> Option<(Step5FinalSegment, Step5FinalSegment)> {
    if segment.translation.trim().is_empty() {
        return None;
    }

    if segment.tokens.len() >= 2 {
        split_final_segment_by_source_tokens(segment, target_lang)
    } else {
        split_final_segment_without_tokens(segment)
    }
}

fn split_final_segment_by_source_tokens(
    segment: &Step5FinalSegment,
    target_lang: &str,
) -> Option<(Step5FinalSegment, Step5FinalSegment)> {
    let split_index = split_token_index_by_readability(&segment.tokens, target_lang)?;
    let (left_tokens, right_tokens) = segment.tokens.split_at(split_index + 1);
    let right_tokens = right_tokens.to_vec();
    if right_tokens.is_empty() {
        return None;
    }
    let source_left = normalize_inline_text(&build_source_from_tokens(left_tokens));
    let source_right = normalize_inline_text(&build_source_from_tokens(&right_tokens));
    if source_left.is_empty() || source_right.is_empty() {
        return None;
    }
    let left_source_units = text_length_units(&source_left, target_lang);
    let right_source_units = text_length_units(&source_right, target_lang).max(1.0);
    let weights = vec![left_source_units.max(1.0), right_source_units];
    let translations = split_translation_evenly_by_weights(&segment.translation, 2, &weights);
    if translations.len() != 2 {
        return None;
    }
    let translation_left = normalize_inline_text(&translations[0]);
    let translation_right = normalize_inline_text(&translations[1]);
    if translation_left.is_empty() || translation_right.is_empty() {
        return None;
    }
    let left_start = segment.start.max(0.0);
    let (left_end, right_start) = source_split_times(left_tokens, &right_tokens, segment);
    Some((
        Step5FinalSegment {
            segment_id: segment.segment_id,
            start: left_start,
            end: left_end,
            source: source_left,
            translation: translation_left,
            tokens: left_tokens.to_vec(),
        },
        Step5FinalSegment {
            segment_id: segment.segment_id,
            start: right_start,
            end: segment.end.max(right_start),
            source: source_right,
            translation: translation_right,
            tokens: right_tokens,
        },
    ))
}

fn split_final_segment_without_tokens(
    segment: &Step5FinalSegment,
) -> Option<(Step5FinalSegment, Step5FinalSegment)> {
    let translations = split_translation_evenly_by_weights(&segment.translation, 2, &[1.0, 1.0]);
    if translations.len() != 2 {
        return None;
    }
    let source_parts = split_translation_evenly_by_weights(&segment.source, 2, &[1.0, 1.0]);
    if source_parts.len() != 2 {
        return None;
    }
    let left_len = segment.end - segment.start;
    if !left_len.is_finite() || left_len <= 0.0 {
        return None;
    }
    let mid = segment.start + (left_len * 0.5);
    Some((
        Step5FinalSegment {
            segment_id: segment.segment_id,
            start: segment.start,
            end: mid,
            source: normalize_inline_text(&source_parts[0]),
            translation: normalize_inline_text(&translations[0]),
            tokens: Vec::new(),
        },
        Step5FinalSegment {
            segment_id: segment.segment_id,
            start: mid,
            end: segment.end,
            source: normalize_inline_text(&source_parts[1]),
            translation: normalize_inline_text(&translations[1]),
            tokens: Vec::new(),
        },
    ))
}

fn split_token_index_by_readability(tokens: &[Step5Token], target_lang: &str) -> Option<usize> {
    if tokens.len() < 2 {
        return None;
    }
    let mut target_units = 0.0f64;
    let mut token_units = Vec::<f64>::with_capacity(tokens.len());
    for token in tokens {
        let unit = text_length_units(&token.text, target_lang);
        target_units += unit;
        token_units.push(unit);
    }
    let mut preferred = target_units / 2.0;
    if !preferred.is_finite() || preferred <= 0.0 {
        preferred = (tokens.len() as f64) / 2.0;
    }
    let mut cumulative = 0.0f64;
    for (index, unit) in token_units.iter().enumerate() {
        cumulative += unit;
        if cumulative >= preferred {
            if index == tokens.len() - 1 {
                return Some(index.saturating_sub(1));
            }
            return Some(index);
        }
    }
    Some((tokens.len() / 2).max(1) - 1)
}

fn source_split_times(
    left_tokens: &[Step5Token],
    right_tokens: &[Step5Token],
    segment: &Step5FinalSegment,
) -> (f64, f64) {
    let left_end = left_tokens
        .last()
        .map(|token| token.end.max(token.start))
        .unwrap_or(segment.end)
        .max(segment.start)
        .min(segment.end.max(segment.start));
    let right_start = right_tokens
        .first()
        .map(|token| token.start.min(segment.end).max(segment.start))
        .unwrap_or(left_end)
        .max(left_end);
    if (right_start - left_end).abs() < MIN_READABLE_DURATION_SECONDS {
        let mid = (segment.start + segment.end.max(segment.start)) / 2.0;
        (mid, mid)
    } else {
        (left_end, right_start)
    }
}
