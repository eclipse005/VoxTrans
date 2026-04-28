pub(super) fn normalize_inline_text(raw: &str) -> String {
    raw.replace('\r', " ")
        .replace('\n', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string()
}

pub(super) fn is_hard_sentence_terminal(ch: char) -> bool {
    matches!(ch, '.' | '?' | '!' | '。' | '？' | '！')
}

pub(super) fn count_ascii_words(text: &str) -> usize {
    text.split_whitespace()
        .filter(|token| token.chars().any(|ch| ch.is_ascii_alphanumeric()))
        .count()
}

pub(super) fn last_ascii_word_lower(text: &str) -> String {
    text.split_whitespace()
        .rev()
        .find_map(|token| {
            let cleaned = token
                .trim_matches(|ch: char| !ch.is_ascii_alphanumeric() && ch != '\'')
                .to_ascii_lowercase();
            if cleaned.is_empty() {
                None
            } else {
                Some(cleaned)
            }
        })
        .unwrap_or_default()
}

pub(super) fn first_ascii_word_lower(text: &str) -> String {
    text.split_whitespace()
        .find_map(|token| {
            let cleaned = token
                .trim_matches(|ch: char| !ch.is_ascii_alphanumeric() && ch != '\'')
                .to_ascii_lowercase();
            if cleaned.is_empty() {
                None
            } else {
                Some(cleaned)
            }
        })
        .unwrap_or_default()
}
