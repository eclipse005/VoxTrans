use std::collections::HashSet;
use std::sync::Arc;

use crate::services::prompts::translation::{TranslationPromptLine, TranslationPromptTerm};

use super::types::{BatchWindow, NormalizedSegment, TranslationTerminologyEntry};
use super::{MAX_TERMS_PER_BATCH, NEXT_CONTEXT_LINES, PREV_CONTEXT_LINES};

/// Next split size after a window of `len` lines fails.
/// 20→10, 10→5, 5→1. `None` means this window cannot split (1 line).
pub(super) fn split_chunk_size(len: usize) -> Option<usize> {
    if len <= 1 {
        None
    } else if len > 10 {
        Some(10)
    } else if len > 5 {
        Some(5)
    } else {
        Some(1)
    }
}

/// Split `window` into consecutive sub-windows of `chunk` lines (last may be shorter).
/// Each slice keeps prev3/next2 relative to its own currentLines.
pub(super) fn split_window(window: &BatchWindow, chunk: usize) -> Vec<BatchWindow> {
    let n = window.local_to_global.len();
    if chunk == 0 || n == 0 {
        return Vec::new();
    }
    if chunk >= n {
        return vec![window.clone()];
    }
    let mut out = Vec::new();
    let mut start = 0usize;
    while start < n {
        let end = (start + chunk).min(n);
        out.push(slice_window(window, start, end));
        start = end;
    }
    out
}

fn slice_window(window: &BatchWindow, start: usize, end: usize) -> BatchWindow {
    let count = end.saturating_sub(start);
    let current_lines: Vec<TranslationPromptLine> = window.current_lines[start..end]
        .iter()
        .enumerate()
        .map(|(index, line)| TranslationPromptLine {
            id: index + 1,
            text: line.text.clone(),
        })
        .collect();

    let mut before: Vec<(usize, String)> = window.prev_lines.iter().cloned().collect();
    for idx in 0..start {
        before.push((
            window.local_to_global[idx],
            window.current_lines[idx].text.clone(),
        ));
    }
    let prev_off = before.len().saturating_sub(PREV_CONTEXT_LINES);
    let prev_lines = before[prev_off..].to_vec();

    let mut after: Vec<(usize, String)> = Vec::new();
    for idx in end..window.local_to_global.len() {
        after.push((
            window.local_to_global[idx],
            window.current_lines[idx].text.clone(),
        ));
    }
    after.extend(window.next_lines.iter().cloned());
    let next_len = after.len().min(NEXT_CONTEXT_LINES);
    let next_lines = after[..next_len].to_vec();

    BatchWindow {
        batch_id: window.batch_id,
        local_ids: (1..=count).collect(),
        local_to_global: window.local_to_global[start..end].to_vec(),
        current_lines: Arc::from(current_lines),
        prev_lines: Arc::from(prev_lines),
        next_lines: Arc::from(next_lines),
        terms: Arc::clone(&window.terms),
        style_guide: window.style_guide.clone(),
        source_lang: window.source_lang.clone(),
        target_lang: window.target_lang.clone(),
    }
}

/// Compute the (start, end) index ranges for each batch.
pub(super) fn batch_index_ranges(
    segments: &[NormalizedSegment],
    batch_size: usize,
) -> Vec<(usize, usize)> {
    if segments.is_empty() || batch_size == 0 {
        return Vec::new();
    }
    let mut ranges = Vec::new();
    let mut start = 0usize;
    while start < segments.len() {
        let end = (start + batch_size).min(segments.len());
        ranges.push((start, end));
        start = end;
    }
    ranges
}

pub(super) fn build_batch_windows(
    segments: &[NormalizedSegment],
    batch_size: usize,
    source_lang: &str,
    target_lang: &str,
    style_guide: &str,
    terminology_entries: &[TranslationTerminologyEntry],
) -> Vec<BatchWindow> {
    if segments.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::<BatchWindow>::new();
    for (batch_start, batch_end) in batch_index_ranges(segments, batch_size) {
        let current = &segments[batch_start..batch_end];

        let prev_start = batch_start.saturating_sub(PREV_CONTEXT_LINES);
        let prev = &segments[prev_start..batch_start];

        let next_end = (batch_end + NEXT_CONTEXT_LINES).min(segments.len());
        let next = &segments[batch_end..next_end];

        let terms = select_batch_terms(current, terminology_entries, MAX_TERMS_PER_BATCH);
        // Keep (segment_id, source) pairs so the prompt can be rebuilt at call
        // time with any translations that became known since window creation
        // (resumed batches, or batches completed earlier in this run).
        let prev_lines = prev
            .iter()
            .map(|segment| (segment.segment_id, segment.source.clone()))
            .collect::<Vec<_>>();
        let current_lines = current
            .iter()
            .enumerate()
            .map(|(index, segment)| TranslationPromptLine {
                id: index + 1,
                text: segment.source.clone(),
            })
            .collect::<Vec<_>>();
        let next_lines = next
            .iter()
            .map(|segment| (segment.segment_id, segment.source.clone()))
            .collect::<Vec<_>>();
        let prompt_terms = terms
            .iter()
            .map(|term| TranslationPromptTerm {
                source: term.source.clone(),
                target: term.target.clone(),
                note: term.note.clone(),
            })
            .collect::<Vec<_>>();

        let batch_index = out.len();

        out.push(BatchWindow {
            batch_id: batch_index,
            local_ids: (1..=current.len()).collect(),
            local_to_global: current.iter().map(|segment| segment.segment_id).collect(),
            current_lines: Arc::from(current_lines),
            prev_lines: Arc::from(prev_lines),
            next_lines: Arc::from(next_lines),
            terms: Arc::from(prompt_terms),
            style_guide: style_guide.to_string(),
            source_lang: source_lang.to_string(),
            target_lang: target_lang.to_string(),
        });
    }

    out
}

fn select_batch_terms(
    current_segments: &[NormalizedSegment],
    entries: &[TranslationTerminologyEntry],
    max_terms: usize,
) -> Vec<TranslationTerminologyEntry> {
    if entries.is_empty() {
        return Vec::new();
    }

    // Recall-oriented fuzzy match. Normalize (lowercase + drop whitespace) so a
    // term covers its spacing/case variants — "orderblock" matches "order
    // block" / "Order Block" in the batch. Every term that appears in this
    // batch MUST be sent: never drop a relevant term (可以多送不能少送).
    // Over-inclusion is acceptable; the translator LLM ignores terms that
    // don't fit a given line.
    let batch_norm = normalize_for_match(
        &current_segments
            .iter()
            .map(|segment| segment.source.as_str())
            .collect::<Vec<_>>()
            .join("\n"),
    );

    let mut selected = Vec::<TranslationTerminologyEntry>::new();
    let mut seen = HashSet::<String>::new();

    // 1. All fuzzy-matched terms — uncapped (不能少送).
    for entry in entries {
        let src_norm = normalize_for_match(&entry.source);
        if src_norm.is_empty() || !batch_norm.contains(&src_norm) {
            continue;
        }
        if seen.insert(src_norm) {
            selected.push(entry.clone());
        }
    }

    // 2. Backfill up to max_terms with the rest (broader context for the LLM).
    for entry in entries {
        if selected.len() >= max_terms {
            break;
        }
        let src_norm = normalize_for_match(&entry.source);
        if src_norm.is_empty() || !seen.insert(src_norm) {
            continue;
        }
        selected.push(entry.clone());
    }

    selected
}

/// Normalize so term matching is invariant to spacing, capitalization, and
/// fullwidth ASCII variants ("orderblock" ≡ "order block" ≡ "Order Block" ≡
/// "order（block）", Ａ→a, （→(, …). Lowercase and whitespace removal are safe
/// for script-counting callers: the folding only touches ASCII-equivalent
/// characters, never kana/han/hangul. Shared with guard and name memory so
/// every same-shaped string compares alike.
pub(super) fn normalize_for_match(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|ch| match ch as u32 {
            0xFF01..=0xFF5E => char::from_u32(ch as u32 - 0xFEE0).unwrap_or(ch),
            _ => ch,
        })
        .filter(|c| !c.is_whitespace())
        .collect()
}

/// Targets of the terms that actually appear in `current_text` (same fuzzy
/// match as stage 1 of `select_batch_terms`). Only matched terms' targets are
/// enforced verbatim in the output; backfilled terms are pure context for the
/// LLM and must not blanket-exempt lines from the source-script leak guard.
pub(super) fn matched_term_targets(
    current_text: &str,
    terms: &[TranslationPromptTerm],
) -> Vec<String> {
    let batch_norm = normalize_for_match(current_text);
    terms
        .iter()
        .filter(|term| {
            let src_norm = normalize_for_match(&term.source);
            !src_norm.is_empty() && batch_norm.contains(&src_norm)
        })
        .map(|term| term.target.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::select_batch_terms;
    use super::super::types::{NormalizedSegment, TranslationTerminologyEntry, TranslationToken};

    fn term(source: &str, target: &str) -> TranslationTerminologyEntry {
        TranslationTerminologyEntry {
            source: source.to_string(),
            target: target.to_string(),
            note: String::new(),
        }
    }

    fn seg(source: &str) -> NormalizedSegment {
        NormalizedSegment {
            segment_id: 1,
            start: 0.0,
            end: 1.0,
            source: source.to_string(),
            tokens: Vec::<TranslationToken>::new(),
        }
    }

    #[test]
    fn fuzzy_matches_spacing_and_case_variants() {
        // The regression: a configured "orderblock" must match "order block"
        // (with a space) in the batch. Exact substring matching dropped it.
        let entries = vec![term("orderblock", "订单块")];
        let batch = [seg("We trade into the order block here.")];
        let selected = select_batch_terms(&batch, &entries, 16);
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].target, "订单块");
    }

    #[test]
    fn split_ladder_is_10_then_5_then_1() {
        use super::split_chunk_size;
        assert_eq!(split_chunk_size(20), Some(10));
        assert_eq!(split_chunk_size(13), Some(10));
        assert_eq!(split_chunk_size(11), Some(10));
        assert_eq!(split_chunk_size(10), Some(5));
        assert_eq!(split_chunk_size(6), Some(5));
        assert_eq!(split_chunk_size(5), Some(1));
        assert_eq!(split_chunk_size(3), Some(1));
        assert_eq!(split_chunk_size(1), None);
        assert_eq!(split_chunk_size(0), None);
    }

    #[test]
    fn split_window_rewrites_local_ids_and_keeps_neighbor_context() {
        use super::split_window;
        use super::super::types::BatchWindow;
        use crate::services::prompts::translation::TranslationPromptLine;
        use std::sync::Arc;

        let current: Vec<TranslationPromptLine> = (0..6)
            .map(|i| TranslationPromptLine {
                id: i + 1,
                text: format!("c{i}"),
            })
            .collect();
        let window = BatchWindow {
            batch_id: 2,
            local_ids: (1..=6).collect(),
            local_to_global: vec![10, 11, 12, 13, 14, 15],
            current_lines: Arc::from(current),
            prev_lines: Arc::from(vec![
                (7, "p1".into()),
                (8, "p2".into()),
                (9, "p3".into()),
            ]),
            next_lines: Arc::from(vec![(16, "n1".into()), (17, "n2".into())]),
            terms: Arc::from(Vec::new()),
            style_guide: String::new(),
            source_lang: "en".into(),
            target_lang: "zh-CN".into(),
        };

        let parts = split_window(&window, 3);
        assert_eq!(parts.len(), 2);

        assert_eq!(parts[0].local_ids, vec![1, 2, 3]);
        assert_eq!(parts[0].local_to_global, vec![10, 11, 12]);
        assert_eq!(
            parts[0]
                .current_lines
                .iter()
                .map(|l| l.text.as_str())
                .collect::<Vec<_>>(),
            vec!["c0", "c1", "c2"]
        );
        assert_eq!(
            parts[0].prev_lines.as_ref(),
            &[(7, "p1".into()), (8, "p2".into()), (9, "p3".into())]
        );
        assert_eq!(
            parts[0].next_lines.as_ref(),
            &[(13, "c3".into()), (14, "c4".into())]
        );

        assert_eq!(parts[1].local_ids, vec![1, 2, 3]);
        assert_eq!(parts[1].local_to_global, vec![13, 14, 15]);
        assert_eq!(
            parts[1].prev_lines.as_ref(),
            &[(10, "c0".into()), (11, "c1".into()), (12, "c2".into())]
        );
        assert_eq!(
            parts[1].next_lines.as_ref(),
            &[(16, "n1".into()), (17, "n2".into())]
        );
    }

    #[test]
    fn split_keeps_translate_prompt_with_fewer_current_lines() {
        use super::split_window;
        use super::super::types::BatchWindow;
        use crate::services::prompts::translation::TranslationPromptLine;
        use std::collections::HashMap;
        use std::sync::Arc;

        let current: Vec<TranslationPromptLine> = (0..20)
            .map(|i| TranslationPromptLine {
                id: i + 1,
                text: format!("line {i}"),
            })
            .collect();
        let window = BatchWindow {
            batch_id: 0,
            local_ids: (1..=20).collect(),
            local_to_global: (1..=20).collect(),
            current_lines: Arc::from(current),
            prev_lines: Arc::from(Vec::new()),
            next_lines: Arc::from(Vec::new()),
            terms: Arc::from(Vec::new()),
            style_guide: String::new(),
            source_lang: "en".into(),
            target_lang: "zh-CN".into(),
        };
        let parts = split_window(&window, 10);
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].local_to_global.len(), 10);
        assert_eq!(parts[1].local_to_global.len(), 10);

        let prompt = parts[0].build_prompt(&HashMap::new(), &[], &[]);
        let parsed: serde_json::Value = serde_json::from_str(&prompt).unwrap();
        assert_eq!(parsed["currentLines"].as_array().unwrap().len(), 10);
        let instruction = parsed["instruction"].as_str().unwrap();
        assert!(instruction.contains("Translate currentLines"));
        assert!(!instruction.contains("Retry"));
        assert!(!instruction.contains("reshape"));
        assert!(!instruction.contains("repair"));
    }

    #[test]
    fn never_drops_a_matched_term_even_past_max() {
        // 可以多送不能少送: matched terms are kept even when they exceed
        // max_terms (max_terms only bounds the backfill, not the matches).
        let entries = vec![
            term("orderblock", "订单块"),
            term("FVG", "公允价值缺口"),
        ];
        let batch = [seg("order block and FVG together")];
        let selected = select_batch_terms(&batch, &entries, 1);
        assert_eq!(selected.len(), 2);
    }

    #[test]
    fn normalize_folds_fullwidth_ascii() {
        use super::normalize_for_match;
        assert_eq!(
            normalize_for_match("LastCall（ラストコール）"),
            "lastcall(ラストコール)"
        );
        assert_eq!(normalize_for_match("Ｌａｓｔ Ｃａｌｌ"), "lastcall");
        assert_eq!(normalize_for_match("キャニオン"), "キャニオン");
    }

    #[test]
    fn matched_term_targets_excludes_unmatched_backfilled_terms() {
        use super::matched_term_targets;
        use crate::services::prompts::translation::TranslationPromptTerm;
        let terms = [
            TranslationPromptTerm {
                source: "order block".to_string(),
                target: "订单块".to_string(),
                note: String::new(),
            },
            TranslationPromptTerm {
                source: "nonexistent".to_string(),
                target: "找不到（ない）".to_string(),
                note: String::new(),
            },
        ];
        let targets = matched_term_targets("We trade the order block here.", &terms);
        assert_eq!(targets, vec!["订单块".to_string()]);
        // Fullwidth variant of the term source still matches.
        let targets = matched_term_targets("ここではＳＮＳを扱う", &[TranslationPromptTerm {
            source: "SNS".to_string(),
            target: "社交媒体".to_string(),
            note: String::new(),
        }]);
        assert_eq!(targets, vec!["社交媒体".to_string()]);
    }
}
