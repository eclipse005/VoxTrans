use std::collections::HashSet;

use super::terminology::TerminologyEntry;
use super::terminology_text::normalize_inline_text;

pub(super) fn merge_terms_with_user_priority(
    user_terms: &[TerminologyEntry],
    extracted_terms: &[TerminologyEntry],
) -> Vec<TerminologyEntry> {
    let mut out = Vec::<TerminologyEntry>::new();
    let mut seen_source = HashSet::<String>::new();

    for entry in user_terms {
        let key = entry.source.to_ascii_lowercase();
        if key.is_empty() || !seen_source.insert(key) {
            continue;
        }
        out.push(entry.clone());
    }

    for entry in extracted_terms {
        let key = entry.source.to_ascii_lowercase();
        if key.is_empty() || !seen_source.insert(key) {
            continue;
        }
        out.push(entry.clone());
    }

    out
}

pub(super) fn normalize_entries(entries: Vec<TerminologyEntry>) -> Vec<TerminologyEntry> {
    let mut out = Vec::<TerminologyEntry>::new();
    let mut seen = HashSet::<(String, String)>::new();

    for entry in entries {
        let source = normalize_inline_text(&entry.source);
        let target = normalize_inline_text(&entry.target);
        let note = normalize_inline_text(&entry.note);
        if source.is_empty() || target.is_empty() {
            continue;
        }
        let key = (source.to_ascii_lowercase(), target.to_ascii_lowercase());
        if !seen.insert(key) {
            continue;
        }
        out.push(TerminologyEntry {
            source,
            target,
            note,
        });
    }

    out
}
