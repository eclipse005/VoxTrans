use crate::services::subtitle_srt::SubtitleSrtSegment;

use super::constants::{
    MIN_READABLE_DURATION_SECONDS, WATCHABILITY_MERGE_LEN_RATIO,
    WATCHABILITY_MERGE_TIME_BUDGET_SECONDS, WATCHABILITY_MERGE_TIME_GAP_SECONDS,
    WATCHABILITY_SPLIT_TRIGGER,
};
use super::language_units::{count_word_units, text_length_units, use_char_units};
use super::numbers::extract_numbers;
use super::quality::{
    ends_with_connector_like_fragment, ends_with_short_dangling_fragment, is_terminal_punctuation,
    line_fragment_penalty,
};
use super::source_residue::looks_like_source_residue;
use super::source_text::build_source_from_tokens;
use super::text_utils::normalize_inline_text;
use super::time_utils::seconds_to_millis;
use super::translation_candidate::{leading_number_anchor, sanitize_translation_candidate};
use super::translation_split::split_translation_evenly_by_weights;
use super::types::{Step5FinalSegment, Step5Token};

pub fn merge_watchability_subtitle_srt_segments(
    segments: &mut Vec<SubtitleSrtSegment>,
    subtitle_length_reference: u32,
    target_lang: &str,
) {
    let original_segments = segments.clone();
    let mut step_segments = segments
        .iter()
        .enumerate()
        .map(|(index, segment)| Step5FinalSegment {
            segment_id: index + 1,
            start: segment.start_ms as f64 / 1000.0,
            end: segment.end_ms.max(segment.start_ms) as f64 / 1000.0,
            source: normalize_inline_text(&segment.source_text),
            translation: normalize_inline_text(&segment.translated_text),
            tokens: Vec::new(),
        })
        .collect::<Vec<_>>();

    merge_watchability_fragments(&mut step_segments, subtitle_length_reference, target_lang);

    *segments = step_segments
        .into_iter()
        .map(|segment| SubtitleSrtSegment {
            start_ms: seconds_to_millis(segment.start),
            end_ms: seconds_to_millis(segment.end.max(segment.start)),
            source_text: original_segments
                .get(segment.segment_id.saturating_sub(1))
                .filter(|original| {
                    seconds_to_millis(segment.start) == original.start_ms
                        && seconds_to_millis(segment.end.max(segment.start)) == original.end_ms
                })
                .map(|original| original.source_text.clone())
                .unwrap_or(segment.source),
            translated_text: segment.translation,
        })
        .collect();
}

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
    if translation.is_empty() || super::translation_candidate::is_unusable_translation(&translation)
    {
        return false;
    }
    !looks_like_source_residue(&segment.source, &translation, target_lang)
}

fn merge_watchability_fragments(
    segments: &mut Vec<Step5FinalSegment>,
    subtitle_length_reference: u32,
    target_lang: &str,
) {
    if segments.len() < 2 {
        return;
    }

    let max_watch_units = (f64::from(subtitle_length_reference.max(1))
        * WATCHABILITY_MERGE_LEN_RATIO)
        .max(WATCHABILITY_SPLIT_TRIGGER);
    let mut merged = Vec::<Step5FinalSegment>::with_capacity(segments.len());
    let mut index = 0usize;

    while index < segments.len() {
        if index + 1 >= segments.len() {
            merged.push(segments[index].clone());
            break;
        }

        let left = &segments[index];
        let right = &segments[index + 1];

        if can_merge_watchability_fragments(left, right, max_watch_units, target_lang) {
            let merged_segment = merge_watchability_pair(left, right, target_lang);
            if is_watchability_fragment_issue(
                &merged_segment.source,
                &merged_segment.translation,
                target_lang,
            ) {
                merged.push(left.clone());
            } else {
                merged.push(merged_segment);
                index += 1;
            }
        } else {
            merged.push(left.clone());
        }
        index += 1;
    }

    if merged.len() == segments.len() {
        return;
    }
    for (index, segment) in merged.iter_mut().enumerate() {
        segment.segment_id = index + 1;
    }
    *segments = merged;
}

fn can_merge_watchability_fragments(
    left: &Step5FinalSegment,
    right: &Step5FinalSegment,
    max_watch_units: f64,
    target_lang: &str,
) -> bool {
    if left.translation.trim().is_empty() || right.translation.trim().is_empty() {
        return false;
    }
    if left.end > right.start {
        return false;
    }
    if right.start - left.end > WATCHABILITY_MERGE_TIME_GAP_SECONDS {
        return false;
    }
    if right.end - left.start > WATCHABILITY_MERGE_TIME_BUDGET_SECONDS {
        return false;
    }
    if is_terminal_punctuation(left.translation.trim().chars().last().unwrap_or_default()) {
        return false;
    }

    let left_frag = ends_with_short_dangling_fragment(&left.translation);
    if !left_frag && !is_watchability_fragment_issue(&left.source, &left.translation, target_lang) {
        return false;
    }

    if !starts_with_continuation_fragment(&right.translation, target_lang) {
        return false;
    }

    let merged_source = merge_watchability_text(&left.source, &right.source, " ", target_lang);
    if merged_source.is_empty() {
        return false;
    }
    let merged_translation =
        merge_watchability_text(&left.translation, &right.translation, "", target_lang);
    if merged_translation.is_empty() {
        return false;
    }

    if text_length_units(&merged_translation, target_lang) > max_watch_units {
        return false;
    }

    let repaired =
        repair_single_watchability_line(&merged_source, &merged_translation, target_lang);
    !is_watchability_fragment_issue(&merged_source, &repaired, target_lang)
}

fn merge_watchability_pair(
    left: &Step5FinalSegment,
    right: &Step5FinalSegment,
    target_lang: &str,
) -> Step5FinalSegment {
    let source = merge_watchability_text(&left.source, &right.source, " ", target_lang);
    let merged_translation =
        merge_watchability_text(&left.translation, &right.translation, "", target_lang);
    let translation = normalize_inline_text(&repair_single_watchability_line(
        &source,
        &merged_translation,
        target_lang,
    ));
    let mut tokens = left.tokens.clone();
    tokens.extend(right.tokens.iter().cloned());
    Step5FinalSegment {
        segment_id: left.segment_id,
        start: left.start,
        end: right.end.max(left.end),
        source,
        translation,
        tokens,
    }
}

fn starts_with_continuation_fragment(text: &str, target_lang: &str) -> bool {
    let normalized = normalize_inline_text(text);
    if normalized.is_empty() {
        return false;
    }
    if use_char_units(target_lang, &normalized) {
        let starters = [
            "个", "这个", "那个", "这", "那", "然后", "并且", "而且", "而", "并", "因为", "所以",
            "如果", "还", "继续", "将", "与", "和",
        ];
        return starters.iter().any(|prefix| normalized.starts_with(prefix));
    }

    let first_token = normalized
        .split_whitespace()
        .next()
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    if first_token.is_empty() {
        return false;
    }
    let starters = [
        "a", "an", "the", "to", "of", "and", "or", "with", "for", "this", "that", "if", "so",
        "then", "while", "it", "you", "we", "they",
    ];
    starters
        .iter()
        .any(|starter| first_token == *starter || normalized.starts_with(&format!("{starter} ")))
}

fn merge_watchability_text(left: &str, right: &str, separator: &str, _target_lang: &str) -> String {
    let left_clean = sanitize_translation_candidate(left);
    let right_clean = sanitize_translation_candidate(right);
    if left_clean.is_empty() {
        return right_clean;
    }
    if right_clean.is_empty() {
        return left_clean;
    }
    let mut merged = left_clean;
    if !separator.is_empty() {
        merged.push_str(separator);
    }
    merged.push_str(&right_clean);
    normalize_inline_text(&merged)
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

pub(super) fn is_watchability_fragment_issue(
    source: &str,
    translation: &str,
    target_lang: &str,
) -> bool {
    let normalized = normalize_inline_text(translation);
    if normalized.is_empty() {
        return false;
    }
    let source_units = count_word_units(source);
    if source_units < 6 {
        return false;
    }
    if let Some(leading_number) = leading_number_anchor(&normalized) {
        let source_numbers = extract_numbers(source);
        let source_matches = source_numbers.iter().any(|value| value == &leading_number);
        if !source_matches {
            return true;
        }
    }
    let has_terminal = normalized
        .chars()
        .last()
        .map(is_terminal_punctuation)
        .unwrap_or(false);
    if has_terminal {
        return false;
    }
    if ends_with_connector_like_fragment(&normalized)
        || ends_with_short_dangling_fragment(&normalized)
    {
        return true;
    }
    let fragment_penalty = line_fragment_penalty(&normalized);
    let line_units = text_length_units(&normalized, target_lang);
    fragment_penalty >= 8 && line_units <= 14.0
}

pub(super) fn repair_watchability_fragments(segments: &mut [Step5FinalSegment], target_lang: &str) {
    if segments.is_empty() {
        return;
    }
    let source_lines = segments
        .iter()
        .map(|segment| segment.source.clone())
        .collect::<Vec<_>>();
    let mut translation_lines = segments
        .iter()
        .map(|segment| segment.translation.clone())
        .collect::<Vec<_>>();
    repair_watchability_lines(&source_lines, &mut translation_lines, target_lang);
    for (segment, translation) in segments.iter_mut().zip(translation_lines.into_iter()) {
        segment.translation = translation;
    }
}

pub(super) fn apply_residual_watchability_overrides(
    segments: &mut [Step5FinalSegment],
    target_lang: &str,
) {
    for segment in segments.iter_mut() {
        let mut updated = sanitize_translation_candidate(&segment.translation);
        if is_watchability_fragment_issue(&segment.source, &updated, target_lang) {
            if let Some(trimmed) = trim_trailing_connector_fragment(&updated) {
                updated = trimmed;
            }
        }
        segment.translation = updated;
    }
}

pub(super) fn repair_watchability_lines(
    source_lines: &[String],
    translation_lines: &mut [String],
    target_lang: &str,
) {
    if source_lines.len() != translation_lines.len() {
        return;
    }

    for index in 0..translation_lines.len() {
        translation_lines[index] = repair_single_watchability_line(
            &source_lines[index],
            &translation_lines[index],
            target_lang,
        );
    }

    for index in 0..translation_lines.len() {
        translation_lines[index] = repair_single_watchability_line(
            &source_lines[index],
            &translation_lines[index],
            target_lang,
        );
    }
}

pub(super) fn repair_single_watchability_line(
    source: &str,
    translation: &str,
    target_lang: &str,
) -> String {
    let original = sanitize_translation_candidate(translation);
    if original.is_empty() {
        return original;
    }

    let mut updated = original.clone();

    if !is_watchability_fragment_issue(source, &updated, target_lang) {
        return updated;
    }

    if is_watchability_fragment_issue(source, &updated, target_lang) {
        if let Some(trimmed) = trim_trailing_connector_fragment(&updated) {
            updated = trimmed;
        }
    }
    updated
}

fn trim_trailing_connector_fragment(text: &str) -> Option<String> {
    let normalized = sanitize_translation_candidate(text);
    if normalized.is_empty() {
        return None;
    }
    let suffixes = [
        "而且",
        "并且",
        "因为",
        "所以",
        "但是",
        "如果",
        "为了",
        "以及",
        "还有",
        "并",
        "和",
        "与",
        "及",
        "或",
        "来",
        "去",
        "在",
        "对",
        "把",
        "将",
        "做一个",
        "大约",
    ];
    for suffix in suffixes {
        if !normalized.ends_with(suffix) {
            continue;
        }
        let trimmed = normalized
            .trim_end_matches(suffix)
            .trim_end_matches('，')
            .trim_end_matches(',')
            .trim();
        if !trimmed.is_empty() {
            return Some(normalize_inline_text(trimmed));
        }
    }
    None
}
