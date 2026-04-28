use super::language_units::is_cjk_char;
use super::quality::is_terminal_punctuation;
use super::text_utils::normalize_inline_text;
use super::translation_candidate::{
    has_tail_ellipsis, is_unusable_translation, sanitize_translation_candidate, strip_tail_ellipsis,
};
use super::types::Step5FinalSegment;

pub(super) fn repair_polished_translation(segment: &mut Step5FinalSegment) {
    let mut translation = sanitize_translation_candidate(&segment.translation);
    if is_unusable_translation(&translation) {
        translation = normalize_inline_text(&segment.source);
    }
    if has_tail_ellipsis(&translation) {
        let trimmed = strip_tail_ellipsis(&translation);
        if !trimmed.is_empty() {
            translation = trimmed;
        }
    }
    if is_unusable_translation(&translation) {
        translation = "[缺失译文]".to_string();
    }
    translation = append_missing_terminal_punctuation(&segment.source, &translation);
    segment.translation = translation;
}

fn append_missing_terminal_punctuation(source: &str, translation: &str) -> String {
    let translation = normalize_inline_text(translation);
    if translation.is_empty()
        || translation
            .chars()
            .last()
            .map(is_terminal_punctuation)
            .unwrap_or(false)
    {
        return translation;
    }

    let Some(source_terminal) = source.trim().chars().last() else {
        return translation;
    };
    if !is_terminal_punctuation(source_terminal) {
        return translation;
    }

    let mut out = translation;
    let punctuation = if out.chars().any(is_cjk_char) {
        match source_terminal {
            '?' | '？' => '？',
            '!' | '！' => '！',
            _ => '。',
        }
    } else {
        match source_terminal {
            '？' => '?',
            '！' => '!',
            '。' => '.',
            other => other,
        }
    };
    out.push(punctuation);
    out
}
