use super::language_units::{count_word_units, text_length_units};
use super::numbers::extract_numbers;
use super::quality::{
    ends_with_connector_like_fragment, ends_with_short_dangling_fragment, is_terminal_punctuation,
    line_fragment_penalty,
};
use super::text_utils::normalize_inline_text;
use super::translation_candidate::{leading_number_anchor, sanitize_translation_candidate};

pub(super) fn is_watchability_fragment_issue(
    source: &str,
    translation: &str,
    target_lang: &str,
) -> bool {
    let normalized = normalize_inline_text(translation);
    if normalized.is_empty() {
        return false;
    }
    let source_units = count_word_units(source);
    if source_units < 6 {
        return false;
    }
    if let Some(leading_number) = leading_number_anchor(&normalized) {
        let source_numbers = extract_numbers(source);
        let source_matches = source_numbers.iter().any(|value| value == &leading_number);
        if !source_matches {
            return true;
        }
    }
    let has_terminal = normalized
        .chars()
        .last()
        .map(is_terminal_punctuation)
        .unwrap_or(false);
    if has_terminal {
        return false;
    }
    if ends_with_connector_like_fragment(&normalized)
        || ends_with_short_dangling_fragment(&normalized)
    {
        return true;
    }
    let fragment_penalty = line_fragment_penalty(&normalized);
    let line_units = text_length_units(&normalized, target_lang);
    fragment_penalty >= 8 && line_units <= 14.0
}

pub(super) fn repair_watchability_lines(
    source_lines: &[String],
    translation_lines: &mut [String],
    target_lang: &str,
) {
    if source_lines.len() != translation_lines.len() {
        return;
    }

    for index in 0..translation_lines.len() {
        translation_lines[index] = repair_single_watchability_line(
            &source_lines[index],
            &translation_lines[index],
            target_lang,
        );
    }

    for index in 0..translation_lines.len() {
        translation_lines[index] = repair_single_watchability_line(
            &source_lines[index],
            &translation_lines[index],
            target_lang,
        );
    }
}

pub(super) fn repair_single_watchability_line(
    source: &str,
    translation: &str,
    target_lang: &str,
) -> String {
    let original = sanitize_translation_candidate(translation);
    if original.is_empty() {
        return original;
    }

    let mut updated = original.clone();

    if !is_watchability_fragment_issue(source, &updated, target_lang) {
        return updated;
    }

    if is_watchability_fragment_issue(source, &updated, target_lang) {
        if let Some(trimmed) = trim_trailing_connector_fragment(&updated) {
            updated = trimmed;
        }
    }
    updated
}

fn trim_trailing_connector_fragment(text: &str) -> Option<String> {
    let normalized = sanitize_translation_candidate(text);
    if normalized.is_empty() {
        return None;
    }
    let suffixes = [
        "而且",
        "并且",
        "因为",
        "所以",
        "但是",
        "如果",
        "为了",
        "以及",
        "还有",
        "并",
        "和",
        "与",
        "及",
        "或",
        "来",
        "去",
        "在",
        "对",
        "把",
        "将",
        "做一个",
        "大约",
    ];
    for suffix in suffixes {
        if !normalized.ends_with(suffix) {
            continue;
        }
        let trimmed = normalized
            .trim_end_matches(suffix)
            .trim_end_matches('，')
            .trim_end_matches(',')
            .trim();
        if !trimmed.is_empty() {
            return Some(normalize_inline_text(trimmed));
        }
    }
    None
}
