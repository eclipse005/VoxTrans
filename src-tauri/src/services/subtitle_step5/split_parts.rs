use super::source_text::build_source_from_tokens;
use super::text_utils::normalize_inline_text;
use super::types::{Step5DraftSegment, Step5SplitPart};

pub(super) fn build_single_split_part(segment: &Step5DraftSegment) -> Step5SplitPart {
    let source = if !segment.tokens.is_empty() {
        build_source_from_tokens(&segment.tokens)
    } else {
        normalize_inline_text(&segment.source)
    };
    Step5SplitPart {
        part_id: 1,
        start: segment.start,
        end: segment.end.max(segment.start),
        source,
        tokens: segment.tokens.clone(),
    }
}

pub(super) fn build_split_parts_from_ranges(
    segment: &Step5DraftSegment,
    ranges: &[(usize, usize)],
) -> Vec<Step5SplitPart> {
    ranges
        .iter()
        .enumerate()
        .map(|(index, (start_idx, end_idx))| {
            let tokens = segment.tokens[*start_idx..=*end_idx].to_vec();
            let part_start = tokens
                .first()
                .map(|token| token.start)
                .unwrap_or(segment.start);
            let part_end = tokens
                .last()
                .map(|token| token.end)
                .unwrap_or(segment.end.max(segment.start));
            let source = build_source_from_tokens(&tokens);
            Step5SplitPart {
                part_id: index + 1,
                start: part_start,
                end: part_end.max(part_start),
                source: if source.is_empty() {
                    normalize_inline_text(&segment.source)
                } else {
                    source
                },
                tokens,
            }
        })
        .collect::<Vec<_>>()
}

pub(super) fn ranges_to_boundary_ids(ranges: &[(usize, usize)]) -> Vec<usize> {
    if ranges.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::<usize>::new();
    for (index, (_start, end)) in ranges.iter().enumerate() {
        if index + 1 >= ranges.len() {
            continue;
        }
        out.push(end + 1);
    }
    out
}

pub(super) fn boundary_ids_to_ranges(
    boundaries: &[usize],
    token_len: usize,
) -> Vec<(usize, usize)> {
    if token_len == 0 {
        return Vec::new();
    }
    let mut sorted = boundaries
        .iter()
        .copied()
        .filter(|id| *id >= 1 && *id < token_len)
        .collect::<Vec<_>>();
    sorted.sort_unstable();
    sorted.dedup();
    let mut ranges = Vec::<(usize, usize)>::new();
    let mut start = 0usize;
    for boundary in sorted {
        let end = boundary.saturating_sub(1);
        if end < start {
            continue;
        }
        ranges.push((start, end));
        start = boundary;
    }
    if start < token_len {
        ranges.push((start, token_len - 1));
    }
    ranges
}
