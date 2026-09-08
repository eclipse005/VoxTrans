//! Translation-unit batching: display cues are grouped into sentence-level
//! units (two-stage pipeline), windows batch units, and the split ladder
//! shrinks a failed window 8→4→2→1 units.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::services::prompts::translation::{TranslationPromptLine, TranslationPromptTerm};
use crate::services::transcription::join_words;

use super::types::{BatchWindow, NormalizedSegment, TranslationTerminologyEntry, UnitSlot};
use super::{MAX_TERMS_PER_BATCH, NEXT_CONTEXT_LINES, PREV_CONTEXT_LINES};

/// Next split size after a window of `len` units fails.
/// 8→4, 4→2, 2→1. `None` means this window cannot split (1 unit).
pub(super) fn split_chunk_size(len: usize) -> Option<usize> {
    if len <= 1 {
        None
    } else if len > 4 {
        Some(4)
    } else if len > 2 {
        Some(2)
    } else {
        Some(1)
    }
}

/// Group ordered display cues into sentence-level translation units: a
/// maximal run of adjacent segments sharing a non-zero `sentence_id`
/// (semantic-sentence grouping from the boundary stage). `sentence_id` 0 —
/// imported cues, legacy artifacts — yields single-cue units. Returns the
/// member segment ids per unit, in order.
pub(super) fn build_translation_units(segments: &[NormalizedSegment]) -> Vec<Vec<usize>> {
    let mut units = Vec::<Vec<usize>>::new();
    let mut group_keys = Vec::<usize>::new();
    for segment in segments {
        let grouped = segment.sentence_id != 0;
        if grouped && group_keys.last() == Some(&segment.sentence_id) {
            units
                .last_mut()
                .expect("group_keys and units stay in sync")
                .push(segment.segment_id);
        } else {
            units.push(vec![segment.segment_id]);
            group_keys.push(if grouped { segment.sentence_id } else { 0 });
        }
    }
    units
}

/// Split `total` units into consecutive sub-windows of `chunk` units (last may
/// be shorter). Each slice keeps prev/next context relative to its own units.
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
        local_to_global: window.local_to_global[start..end].to_vec(),
        current_lines: Arc::from(current_lines),
        unit_slots: Arc::from(window.unit_slots[start..end].to_vec()),
        prev_lines: Arc::from(prev_lines),
        next_lines: Arc::from(next_lines),
        terms: Arc::clone(&window.terms),
        style_guide: window.style_guide.clone(),
        source_lang: window.source_lang.clone(),
        target_lang: window.target_lang.clone(),
    }
}

/// Compute the (start, end) unit-index ranges for each batch.
fn unit_index_ranges(total_units: usize, batch_size: usize) -> Vec<(usize, usize)> {
    if total_units == 0 || batch_size == 0 {
        return Vec::new();
    }
    let mut ranges = Vec::new();
    let mut start = 0usize;
    while start < total_units {
        let end = (start + batch_size).min(total_units);
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

    let units = build_translation_units(segments);
    let by_id: HashMap<usize, &NormalizedSegment> =
        segments.iter().map(|s| (s.segment_id, s)).collect();
    // Unit source text = its slot cue texts joined with language-aware
    // spacing (the same join the boundary stage used to build cue texts).
    let unit_sources: Vec<String> = units
        .iter()
        .map(|ids| {
            join_words(
                ids.iter()
                    .filter_map(|id| by_id.get(id))
                    .map(|s| s.source.as_str()),
            )
        })
        .collect();

    let mut out = Vec::<BatchWindow>::new();
    for (batch_start, batch_end) in unit_index_ranges(units.len(), batch_size) {
        let unit_range = batch_start..batch_end;

        let prev_start = batch_start.saturating_sub(PREV_CONTEXT_LINES);
        let prev: Vec<(usize, String)> = (prev_start..batch_start)
            .map(|i| (i + 1, unit_sources[i].clone()))
            .collect();
        let next_end = (batch_end + NEXT_CONTEXT_LINES).min(units.len());
        let next: Vec<(usize, String)> = (batch_end..next_end)
            .map(|i| (i + 1, unit_sources[i].clone()))
            .collect();

        let current_lines: Vec<TranslationPromptLine> = unit_range
            .clone()
            .enumerate()
            .map(|(index, i)| TranslationPromptLine {
                id: index + 1,
                text: unit_sources[i].clone(),
            })
            .collect();

        let unit_slots: Vec<Vec<UnitSlot>> = unit_range
            .clone()
            .map(|i| {
                units[i]
                    .iter()
                    .map(|id| {
                        let segment = by_id[id];
                        (*id, segment.source.clone())
                    })
                    .collect()
            })
            .collect();

        // Terminology matched against this batch's unit texts.
        let batch_unit_texts: Vec<&str> =
            unit_range.clone().map(|i| unit_sources[i].as_str()).collect();
        let terms = select_batch_terms(&batch_unit_texts, terminology_entries, MAX_TERMS_PER_BATCH);
        let prompt_terms = terms
            .iter()
            .map(|term| TranslationPromptTerm {
                source: term.source.clone(),
                target: term.target.clone(),
                note: term.note.clone(),
            })
            .collect::<Vec<_>>();

        out.push(BatchWindow {
            batch_id: out.len(),
            local_to_global: unit_range.clone().map(|i| i + 1).collect(),
            current_lines: Arc::from(current_lines),
            unit_slots: Arc::from(unit_slots),
            prev_lines: Arc::from(prev),
            next_lines: Arc::from(next),
            terms: Arc::from(prompt_terms),
            style_guide: style_guide.to_string(),
            source_lang: source_lang.to_string(),
            target_lang: target_lang.to_string(),
        });
    }

    out
}

fn select_batch_terms(
    unit_sources: &[&str],
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
    // don't fit a given unit.
    let batch_norm = normalize_for_match(&unit_sources.join("\n"));

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
    use super::build_translation_units;
    use super::select_batch_terms;
    use super::super::types::{NormalizedSegment, TranslationTerminologyEntry, TranslationToken};

    fn term(source: &str, target: &str) -> TranslationTerminologyEntry {
        TranslationTerminologyEntry {
            source: source.to_string(),
            target: target.to_string(),
            note: String::new(),
        }
    }

    fn seg(id: usize, source: &str, sentence_id: usize) -> NormalizedSegment {
        NormalizedSegment {
            segment_id: id,
            sentence_id,
            start: 0.0,
            end: 1.0,
            source: source.to_string(),
            tokens: Vec::<TranslationToken>::new(),
        }
    }

    #[test]
    fn units_group_adjacent_same_sentence_segments() {
        // Segments 1-2 belong to semantic sentence 7; 3 is its own sentence;
        // 4-5 carry no grouping (0) and stay single-cue units.
        let segments = vec![
            seg(1, "Hence", 7),
            seg(2, "this video.", 7),
            seg(3, "Next one.", 8),
            seg(4, "Imported cue A.", 0),
            seg(5, "Imported cue B.", 0),
        ];
        let units = build_translation_units(&segments);
        assert_eq!(units, vec![vec![1, 2], vec![3], vec![4], vec![5]]);
    }

    #[test]
    fn units_do_not_group_across_intervening_segments() {
        // Same sentence_id repeated non-adjacently must NOT merge.
        let segments = vec![seg(1, "a", 7), seg(2, "b", 8), seg(3, "c", 7)];
        let units = build_translation_units(&segments);
        assert_eq!(units, vec![vec![1], vec![2], vec![3]]);
    }

    #[test]
    fn fuzzy_matches_spacing_and_case_variants() {
        // The regression: a configured "orderblock" must match "order block"
        // (with a space) in the batch. Exact substring matching dropped it.
        let entries = vec![term("orderblock", "订单块")];
        let units = ["We trade into the order block here."];
        let selected = select_batch_terms(&units, &entries, 16);
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].target, "订单块");
    }

    #[test]
    fn split_ladder_is_4_then_2_then_1() {
        use super::split_chunk_size;
        assert_eq!(split_chunk_size(20), Some(4));
        assert_eq!(split_chunk_size(8), Some(4));
        assert_eq!(split_chunk_size(5), Some(4));
        assert_eq!(split_chunk_size(4), Some(2));
        assert_eq!(split_chunk_size(3), Some(2));
        assert_eq!(split_chunk_size(2), Some(1));
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
                text: format!("u{i}"),
            })
            .collect();
        let window = BatchWindow {
            batch_id: 2,
            local_to_global: vec![10, 11, 12, 13, 14, 15],
            current_lines: Arc::from(current),
            unit_slots: Arc::from(vec![
                vec![(101, "s0".to_string())],
                vec![(102, "s1".to_string())],
                vec![(103, "s2".to_string())],
                vec![(104, "s3".to_string())],
                vec![(105, "s4".to_string())],
                vec![(106, "s5".to_string())],
            ]),
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

        assert_eq!(parts[0].local_to_global, vec![10, 11, 12]);
        assert_eq!(
            parts[0].unit_slots.as_ref(),
            &[
                vec![(101, "s0".to_string())],
                vec![(102, "s1".to_string())],
                vec![(103, "s2".to_string())],
            ]
        );
        assert_eq!(
            parts[0].prev_lines.as_ref(),
            &[(7, "p1".into()), (8, "p2".into()), (9, "p3".into())]
        );
        assert_eq!(
            parts[0].next_lines.as_ref(),
            &[(13, "u3".into()), (14, "u4".into())]
        );

        assert_eq!(parts[1].local_to_global, vec![13, 14, 15]);
        assert_eq!(
            parts[1].prev_lines.as_ref(),
            &[(10, "u0".into()), (11, "u1".into()), (12, "u2".into())]
        );
        assert_eq!(
            parts[1].next_lines.as_ref(),
            &[(16, "n1".into()), (17, "n2".into())]
        );
    }

    #[test]
    fn never_drops_a_matched_term_even_past_max() {
        // 可以多送不能少送: matched terms are kept even when they exceed
        // max_terms (max_terms only bounds the backfill, not the matches).
        let entries = vec![
            term("orderblock", "订单块"),
            term("FVG", "公允价值缺口"),
        ];
        let units = ["order block and FVG together"];
        let selected = select_batch_terms(&units, &entries, 1);
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
