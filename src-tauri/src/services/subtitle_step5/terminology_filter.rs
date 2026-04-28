use std::collections::HashSet;

use super::language_units::is_cjk_char;
use super::types::Step5TerminologyEntry;

pub(super) fn select_terms_for_text(
    source: &str,
    entries: &[Step5TerminologyEntry],
    max_terms: usize,
) -> Vec<Step5TerminologyEntry> {
    if entries.is_empty() {
        return Vec::new();
    }
    if max_terms == 0 {
        return Vec::new();
    }
    let source_lower = source.to_lowercase();
    let mut seen = HashSet::<String>::new();
    let mut picked = Vec::<Step5TerminologyEntry>::new();
    for entry in entries {
        if picked.len() >= max_terms {
            break;
        }
        let key = entry.source.trim().to_lowercase();
        if key.is_empty() || !source_contains_terminology_term(&source_lower, &key) {
            continue;
        }
        if !seen.insert(key) {
            continue;
        }
        picked.push(entry.clone());
    }
    picked
}

pub(super) fn source_contains_terminology_term(
    source_lower: &str,
    term_source_lower: &str,
) -> bool {
    let term = term_source_lower.trim();
    if term.is_empty() {
        return false;
    }
    if term.chars().any(is_cjk_char) {
        return source_lower.contains(term);
    }

    let is_single_ascii_token_term = term.chars().any(|ch| ch.is_ascii_alphabetic())
        && !term.chars().any(|ch| ch.is_whitespace() || is_cjk_char(ch));
    if !is_single_ascii_token_term {
        return source_lower.contains(term);
    }

    let mut search_start = 0usize;
    while let Some(offset) = source_lower[search_start..].find(term) {
        let start = search_start + offset;
        let end = start + term.len();
        let prev_blocks = source_lower[..start]
            .chars()
            .next_back()
            .map(|ch| ch.is_ascii_alphabetic())
            .unwrap_or(false);
        let next_blocks = source_lower[end..]
            .chars()
            .next()
            .map(|ch| ch.is_ascii_alphabetic())
            .unwrap_or(false);
        if !prev_blocks && !next_blocks {
            return true;
        }
        search_start = end;
    }

    false
}
