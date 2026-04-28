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

    let batch_text = current_segments
        .iter()
        .map(|segment| segment.source.as_str())
        .collect::<Vec<_>>()
        .join("\n")
        .to_lowercase();

    let mut matched = entries
        .iter()
        .filter(|entry| {
            let source = entry.source.trim().to_lowercase();
            !source.is_empty() && batch_text.contains(&source)
        })
        .take(max_terms)
        .cloned()
        .collect::<Vec<_>>();

    if matched.len() >= max_terms {
        return matched;
    }

    for entry in entries {
        if matched.len() >= max_terms {
            break;
        }
        if matched
            .iter()
            .any(|existing| existing.source.eq_ignore_ascii_case(&entry.source))
        {
            continue;
        }
        matched.push(entry.clone());
    }

    matched
}
