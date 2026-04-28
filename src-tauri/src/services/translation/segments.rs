use super::text::{
    count_ascii_words, first_ascii_word_lower, is_hard_sentence_terminal, last_ascii_word_lower,
    normalize_inline_text,
};
use super::types::{NormalizedSegment, TranslationSegmentInput};

pub(super) fn normalize_segments(segments: &[TranslationSegmentInput]) -> Vec<NormalizedSegment> {
    let mut out = Vec::<NormalizedSegment>::new();
    for (index, segment) in segments.iter().enumerate() {
        let source = normalize_inline_text(&segment.segment);
        let source = if source.is_empty() {
            let fallback = segment
                .tokens
                .iter()
                .map(|token| token.text.trim())
                .filter(|token| !token.is_empty())
                .collect::<Vec<_>>()
                .join(" ");
            normalize_inline_text(&fallback)
        } else {
            source
        };
        if source.is_empty() {
            continue;
        }
        out.push(NormalizedSegment {
            segment_id: index + 1,
            start: segment.start,
            end: segment.end.max(segment.start),
            source,
            tokens: segment.tokens.clone(),
        });
    }
    out
}

pub(super) fn merge_dangling_source_segments(
    segments: Vec<NormalizedSegment>,
) -> Vec<NormalizedSegment> {
    if segments.len() < 2 {
        return segments;
    }

    let mut merged = Vec::<NormalizedSegment>::new();
    let mut index = 0usize;
    while index < segments.len() {
        let mut current = segments[index].clone();
        while index + 1 < segments.len()
            && can_merge_dangling_source_pair(&current, &segments[index + 1])
        {
            current = merge_source_pair(&current, &segments[index + 1]);
            index += 1;
        }
        merged.push(current);
        index += 1;
    }

    for (index, segment) in merged.iter_mut().enumerate() {
        segment.segment_id = index + 1;
    }
    merged
}

fn can_merge_dangling_source_pair(left: &NormalizedSegment, right: &NormalizedSegment) -> bool {
    let left_text = left.source.trim();
    let right_text = right.source.trim();
    if left_text.is_empty() || right_text.is_empty() {
        return false;
    }
    if left.end > right.start || right.start - left.end > 1.0 {
        return false;
    }

    let combined_words = count_ascii_words(left_text) + count_ascii_words(right_text);
    if combined_words > 38 {
        return false;
    }
    if right.end - left.start > 12.0 {
        return false;
    }

    let left_last = left_text.chars().last().unwrap_or_default();
    if is_hard_sentence_terminal(left_last) {
        return false;
    }

    if left_last == ',' || left_last == ';' || left_last == ':' {
        return starts_with_lowercase_or_connector(right_text)
            || starts_with_continuation_word(right_text);
    }

    if ends_with_dangling_source_word(left_text) {
        return true;
    }

    starts_with_subordinate_clause(left_text) && starts_with_lowercase_or_connector(right_text)
}

fn merge_source_pair(left: &NormalizedSegment, right: &NormalizedSegment) -> NormalizedSegment {
    let mut tokens = left.tokens.clone();
    tokens.extend(right.tokens.iter().cloned());
    NormalizedSegment {
        segment_id: left.segment_id,
        start: left.start,
        end: right.end.max(left.end),
        source: normalize_inline_text(&format!("{} {}", left.source, right.source)),
        tokens,
    }
}

fn starts_with_lowercase_or_connector(text: &str) -> bool {
    text.chars()
        .next()
        .map(|ch| ch.is_ascii_lowercase())
        .unwrap_or(false)
        || starts_with_continuation_word(text)
}

fn starts_with_continuation_word(text: &str) -> bool {
    let first = first_ascii_word_lower(text);
    matches!(
        first.as_str(),
        "and"
            | "or"
            | "but"
            | "so"
            | "then"
            | "because"
            | "which"
            | "that"
            | "to"
            | "for"
            | "with"
            | "plus"
    )
}

fn ends_with_dangling_source_word(text: &str) -> bool {
    let last = last_ascii_word_lower(text);
    matches!(
        last.as_str(),
        "a" | "an"
            | "the"
            | "this"
            | "that"
            | "these"
            | "those"
            | "my"
            | "your"
            | "his"
            | "her"
            | "their"
            | "our"
            | "of"
            | "to"
            | "for"
            | "with"
            | "and"
            | "or"
            | "but"
            | "because"
            | "if"
            | "when"
            | "which"
            | "who"
            | "you"
            | "i"
            | "we"
            | "they"
    )
}

fn starts_with_subordinate_clause(text: &str) -> bool {
    let first = first_ascii_word_lower(text);
    matches!(
        first.as_str(),
        "if" | "when" | "because" | "although" | "while" | "once" | "unless"
    )
}
