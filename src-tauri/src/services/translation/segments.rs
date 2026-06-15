use super::text::normalize_inline_text;
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
