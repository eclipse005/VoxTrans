use std::collections::HashSet;

use crate::services::prompts::translation::{
    TranslationPromptLine, TranslationPromptTerm, build_batch_translate_prompt,
};

use super::types::{BatchWindow, NormalizedSegment, TranslationTerminologyEntry};
use super::{CONTEXT_LINE_LIMIT, MAX_TERMS_PER_BATCH};

pub(super) fn build_batch_windows(
    segments: &[NormalizedSegment],
    batch_size: usize,
    source_lang: &str,
    target_lang: &str,
    theme_summary: &str,
    terminology_entries: &[TranslationTerminologyEntry],
) -> Vec<BatchWindow> {
    if segments.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::<BatchWindow>::new();
    let mut batch_start = 0usize;
    while batch_start < segments.len() {
        let batch_end = (batch_start + batch_size).min(segments.len());
        let current = &segments[batch_start..batch_end];

        let prev_start = batch_start.saturating_sub(CONTEXT_LINE_LIMIT);
        let prev = &segments[prev_start..batch_start];

        let next_end = (batch_end + CONTEXT_LINE_LIMIT).min(segments.len());
        let next = &segments[batch_end..next_end];

        let terms = select_batch_terms(current, terminology_entries, MAX_TERMS_PER_BATCH);
        let prev_lines = prev
            .iter()
            .map(|segment| segment.source.clone())
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
            .map(|segment| segment.source.clone())
            .collect::<Vec<_>>();
        let prompt_terms = terms
            .iter()
            .map(|term| TranslationPromptTerm {
                source: term.source.clone(),
                target: term.target.clone(),
                note: term.note.clone(),
            })
            .collect::<Vec<_>>();
        let prompt = build_batch_translate_prompt(
            source_lang,
            target_lang,
            theme_summary,
            &prev_lines,
            &current_lines,
            &next_lines,
            &prompt_terms,
        );

        out.push(BatchWindow {
            batch_id: out.len(),
            local_ids: (1..=current.len()).collect(),
            local_to_global: current.iter().map(|segment| segment.segment_id).collect(),
            prompt,
        });

        batch_start = batch_end;
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

/// Lowercase and drop whitespace so term matching is invariant to spacing and
/// capitalization ("orderblock" ≡ "order block" ≡ "Order Block").
fn normalize_for_match(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .filter(|c| !c.is_whitespace())
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
}
