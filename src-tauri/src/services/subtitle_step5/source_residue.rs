use std::collections::HashSet;

use super::language_units::is_cjk_char;

pub(super) fn target_prefers_cjk(target_lang: &str) -> bool {
    let normalized = target_lang.trim().to_ascii_lowercase();
    normalized.starts_with("zh") || normalized.starts_with("ja") || normalized.starts_with("ko")
}

pub(super) fn looks_like_source_residue(
    source: &str,
    translation: &str,
    target_lang: &str,
) -> bool {
    if !target_prefers_cjk(target_lang) {
        return false;
    }
    let translation_words = extract_ascii_words(translation);
    if translation_words.len() < 4 {
        return false;
    }
    let source_words = extract_ascii_words(source)
        .into_iter()
        .collect::<HashSet<_>>();
    if source_words.is_empty() {
        return false;
    }
    let overlap = translation_words
        .iter()
        .filter(|word| source_words.contains(*word))
        .count();
    let overlap_ratio = overlap as f64 / translation_words.len() as f64;
    let cjk_count = translation.chars().filter(|ch| is_cjk_char(*ch)).count();
    overlap_ratio >= 0.6 && cjk_count <= 2
}

fn extract_ascii_words(text: &str) -> Vec<String> {
    let mut out = Vec::<String>::new();
    let mut current = String::new();
    for ch in text.chars() {
        if ch.is_ascii_alphabetic() {
            current.push(ch.to_ascii_lowercase());
            continue;
        }
        if current.len() >= 2 {
            out.push(current.clone());
        }
        current.clear();
    }
    if current.len() >= 2 {
        out.push(current);
    }
    out
}
