use crate::services::subtitle_srt::SubtitleSrtSegment;

use crate::services::transcription::is_discourse_marker_text;

use super::constants::{
    FLASH_SECONDS, ORPHAN_MERGE_GRACE_UNITS, ORPHAN_TAIL_CJK_UNITS, ORPHAN_TAIL_LATIN_UNITS,
    ORPHAN_TAIL_MAX_SECONDS, WATCHABILITY_MERGE_LEN_RATIO, WATCHABILITY_MERGE_TIME_BUDGET_SECONDS,
    WATCHABILITY_MERGE_TIME_GAP_SECONDS,
};
use super::language_units::{text_length_units, use_char_units};
use super::quality::{
    ends_with_connector_like_fragment, ends_with_short_dangling_fragment, is_terminal_punctuation,
    last_lexical_token,
};
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
    absorb_flash_segments(&mut step_segments, target_limit, target_lang);

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

    let left_norm = normalize_inline_text(&left.translation);
    if is_closed_sentence_pair(left, right, target_lang, &left_norm) {
        return false;
    }

    let right_orphan = is_orphan_tail(right, target_lang) && !is_orphan_tail(left, target_lang);
    if right_orphan {
        return !pair_exceeds_caps(left, right, max_watch_units, target_lang, ORPHAN_MERGE_GRACE_UNITS);
    }
    let left_orphan = is_orphan_tail(left, target_lang) && !is_orphan_tail(right, target_lang);
    if left_orphan {
        let grace = if right.end - right.start >= FLASH_SECONDS {
            ORPHAN_MERGE_GRACE_UNITS
        } else {
            0.0
        };
        return !pair_exceeds_caps(left, right, max_watch_units, target_lang, grace);
    }

    let last = last_lexical_token(&left_norm, target_lang);
    if ends_with_connector_like_fragment(&left.translation, target_lang)
        || is_latin_function_token(&last, target_lang)
    {
        let right_units = text_length_units(&right.translation, target_lang);
        let right_cap = if use_char_units(target_lang, &right.translation) {
            12.0
        } else {
            8.0
        };
        if right_units > 0.0 && right_units <= right_cap {
            return !pair_exceeds_caps(
                left,
                right,
                max_watch_units,
                target_lang,
                ORPHAN_MERGE_GRACE_UNITS,
            );
        }
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
    let merged_translation = merge_watchability_text(
        &left.translation,
        &right.translation,
        translation_separator(target_lang, &left.translation),
    );
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

fn absorb_flash_segments(
    segments: &mut Vec<Step5FinalSegment>,
    target_limit: u32,
    target_lang: &str,
) {
    if segments.len() < 2 {
        return;
    }
    let max_watch_units = f64::from(target_limit.max(1)) * WATCHABILITY_MERGE_LEN_RATIO;
    let mut out = Vec::<Step5FinalSegment>::with_capacity(segments.len());
    let mut index = 0usize;
    while index < segments.len() {
        let cur = &segments[index];
        let dur = cur.end - cur.start;
        if dur >= FLASH_SECONDS || index + 1 == segments.len() {
            if dur < FLASH_SECONDS
                && let Some(prev) = out.last()
                && can_flash_merge(prev, cur, max_watch_units, target_lang)
            {
                let merged = merge_watchability_pair(prev, cur, target_lang);
                let last = out.len() - 1;
                out[last] = merged;
                index += 1;
                continue;
            }
            out.push(cur.clone());
            index += 1;
            continue;
        }
        let next = &segments[index + 1];
        if can_flash_merge(cur, next, max_watch_units, target_lang) {
            let merged = merge_watchability_pair(cur, next, target_lang);
            if merged.end - merged.start < FLASH_SECONDS
                && let Some(prev) = out.last()
                && can_flash_merge(prev, &merged, max_watch_units, target_lang)
            {
                let glued = merge_watchability_pair(prev, &merged, target_lang);
                let last = out.len() - 1;
                out[last] = glued;
                index += 2;
                continue;
            }
            out.push(merged);
            index += 2;
            continue;
        }
        if let Some(prev) = out.last()
            && can_flash_merge(prev, cur, max_watch_units, target_lang)
        {
            let merged = merge_watchability_pair(prev, cur, target_lang);
            let last = out.len() - 1;
            out[last] = merged;
            index += 1;
            continue;
        }
        out.push(cur.clone());
        index += 1;
    }
    if out.len() == segments.len() {
        return;
    }
    for (index, segment) in out.iter_mut().enumerate() {
        segment.segment_id = index + 1;
    }
    *segments = out;
}

fn can_flash_merge(
    left: &Step5FinalSegment,
    right: &Step5FinalSegment,
    max_watch_units: f64,
    target_lang: &str,
) -> bool {
    if left.translation.trim().is_empty() || right.translation.trim().is_empty() {
        return false;
    }
    if right.start - left.end > WATCHABILITY_MERGE_TIME_GAP_SECONDS {
        return false;
    }
    if right.end - left.start > WATCHABILITY_MERGE_TIME_BUDGET_SECONDS {
        return false;
    }
    let left_norm = normalize_inline_text(&left.translation);
    if is_closed_sentence_pair(left, right, target_lang, &left_norm) {
        return false;
    }
    let grace = if is_orphan_tail(right, target_lang) && !is_orphan_tail(left, target_lang) {
        ORPHAN_MERGE_GRACE_UNITS
    } else {
        0.0
    };
    !pair_exceeds_caps(left, right, max_watch_units, target_lang, grace)
}

fn is_closed_sentence_pair(
    left: &Step5FinalSegment,
    right: &Step5FinalSegment,
    target_lang: &str,
    left_norm: &str,
) -> bool {
    let Some(last) = left_norm.chars().last() else {
        return false;
    };
    if !is_terminal_punctuation(last) {
        return false;
    }
    if is_short_interjection(left, target_lang) {
        return false;
    }
    let right_units = text_length_units(&right.translation, target_lang);
    let afterthought = right_units > 0.0
        && right_units
            <= if use_char_units(target_lang, &right.translation) {
                4.0
            } else {
                2.0
            };
    if afterthought || is_short_interjection(right, target_lang) {
        return false;
    }
    true
}

fn is_short_interjection(seg: &Step5FinalSegment, target_lang: &str) -> bool {
    let units = text_length_units(&seg.translation, target_lang);
    let max = if use_char_units(target_lang, &seg.translation) {
        6.0
    } else {
        2.0
    };
    if units <= 0.0 || units > max {
        return false;
    }
    let n = normalize_inline_text(&seg.translation);
    is_discourse_marker_text(&n)
        || n.chars().last().is_some_and(is_terminal_punctuation)
}

fn is_orphan_tail(seg: &Step5FinalSegment, target_lang: &str) -> bool {
    let dur = seg.end - seg.start;
    if dur <= 0.0 || dur > ORPHAN_TAIL_MAX_SECONDS {
        return false;
    }
    let units = text_length_units(&seg.translation, target_lang);
    let cap = if use_char_units(target_lang, &seg.translation) {
        ORPHAN_TAIL_CJK_UNITS
    } else {
        ORPHAN_TAIL_LATIN_UNITS
    };
    units > 0.0 && units <= cap
}

fn pair_exceeds_caps(
    left: &Step5FinalSegment,
    right: &Step5FinalSegment,
    max_watch_units: f64,
    target_lang: &str,
    unit_grace: f64,
) -> bool {
    let merged_translation = merge_watchability_text(
        &left.translation,
        &right.translation,
        translation_separator(target_lang, &left.translation),
    );
    text_length_units(&merged_translation, target_lang) > max_watch_units + unit_grace
}

fn is_latin_function_token(token: &str, target_lang: &str) -> bool {
    if token.is_empty() || use_char_units(target_lang, token) {
        return false;
    }
    matches!(
        token,
        "a" | "an"
            | "the"
            | "to"
            | "of"
            | "and"
            | "or"
            | "with"
            | "for"
            | "in"
            | "on"
            | "at"
            | "by"
            | "from"
    )
}

fn merge_watchability_pair(
    left: &Step5FinalSegment,
    right: &Step5FinalSegment,
    target_lang: &str,
) -> Step5FinalSegment {
    let source = merge_watchability_text(&left.source, &right.source, " ");
    let merged_translation = merge_watchability_text(
        &left.translation,
        &right.translation,
        translation_separator(target_lang, &left.translation),
    );
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

/// CJK targets join translations without a space (char-based units);
/// everything else joins with a single space (word-based units).
fn translation_separator(target_lang: &str, left_translation: &str) -> &'static str {
    if use_char_units(target_lang, left_translation) {
        ""
    } else {
        " "
    }
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
