use super::language_units::is_cjk_char;

pub(super) fn normalize_inline_text(raw: &str) -> String {
    raw.replace(['\r', '\n'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string()
}

pub(super) fn is_meaningful_text_char(ch: char) -> bool {
    is_cjk_char(ch) || ch.is_ascii_alphanumeric() || ch.is_alphabetic() || ch.is_numeric()
}

pub(super) fn ends_with_sentence_punctuation(text: &str) -> bool {
    let t = text.trim_end();
    t.ends_with('.')
        || t.ends_with('!')
        || t.ends_with('?')
        || t.ends_with('。')
        || t.ends_with('！')
        || t.ends_with('？')
        || t.ends_with(';')
        || t.ends_with('；')
}
