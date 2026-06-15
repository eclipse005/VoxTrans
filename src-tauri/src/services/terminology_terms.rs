use std::collections::HashSet;

use super::terminology::TerminologyEntry;
use super::terminology_text::normalize_inline_text;

/// Merge user terminology (authoritative, always included) with the
/// model-extracted terms.
///
/// ALL user terms are kept unconditionally — the translator LLM decides
/// relevance and matches spacing/capitalization variants (a configured
/// "orderblock" covers "order block" / "Order Block" in the source). We
/// deliberately do NOT code-level filter user terms by presence: an exact
/// substring check would drop a term whose source form differs from the
/// transcript (the orderblock vs "order block" bug). When a user term and an
/// extracted term share a source, the user's target wins.
pub(super) fn force_include_user_terms(
    user_terms: &[TerminologyEntry],
    extracted: &[TerminologyEntry],
) -> Vec<TerminologyEntry> {
    let mut out = Vec::<TerminologyEntry>::new();
    let mut claimed: HashSet<String> = HashSet::new();

    // User terms first (authoritative, all included).
    for entry in user_terms {
        let src = normalize_inline_text(&entry.source);
        if src.is_empty() {
            continue;
        }
        let key = src.to_ascii_lowercase();
        if claimed.insert(key) {
            out.push(entry.clone());
        }
    }

    // Extracted terms, skipping any source already claimed by a user term.
    for entry in extracted {
        let src = normalize_inline_text(&entry.source);
        if src.is_empty() {
            continue;
        }
        let key = src.to_ascii_lowercase();
        if !claimed.insert(key) {
            continue;
        }
        out.push(entry.clone());
    }

    out
}

/// Normalize and de-duplicate terminology entries by (source, target),
/// case-insensitively. Drops entries with empty source or target.
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
