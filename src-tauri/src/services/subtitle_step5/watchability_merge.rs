use crate::services::subtitle_srt::SubtitleSrtSegment;

use super::constants::{
    WATCHABILITY_MERGE_LEN_RATIO, WATCHABILITY_MERGE_TIME_BUDGET_SECONDS,
    WATCHABILITY_MERGE_TIME_GAP_SECONDS,
};
use super::language_units::{text_length_units, use_char_units};
use super::quality::{ends_with_short_dangling_fragment, is_terminal_punctuation};
use super::text_utils::normalize_inline_text;
use super::time_utils::seconds_to_millis;
use super::translation_candidate::sanitize_translation_candidate;
use super::types::Step5FinalSegment;
use super::watchability::{is_watchability_fragment_issue, repair_single_watchability_line};

pub fn merge_watchability_subtitle_srt_segments(
    segments: &mut Vec<SubtitleSrtSegment>,
    subtitle_length_preset: &str,
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

    let target_limit = crate::services::subtitle_length::target_limit_for_preset(
        target_lang,
        subtitle_length_preset,
    );
    merge_watchability_fragments(&mut step_segments, target_limit, target_lang);

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

fn merge_watchability_fragments(
    segments: &mut Vec<Step5FinalSegment>,
    target_limit: u32,
    target_lang: &str,
) {
    if segments.len() < 2 {
        return;
    }

    let max_watch_units = f64::from(target_limit.max(1)) * WATCHABILITY_MERGE_LEN_RATIO;
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

    let merged_source = merge_watchability_text(&left.source, &right.source, " ");
    if merged_source.is_empty() {
        return false;
    }
    let merged_translation = merge_watchability_text(&left.translation, &right.translation, "");
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
    let source = merge_watchability_text(&left.source, &right.source, " ");
    let merged_translation = merge_watchability_text(&left.translation, &right.translation, "");
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

fn merge_watchability_text(left: &str, right: &str, separator: &str) -> String {
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
