use super::language_units::use_char_units;
use super::text_utils::normalize_inline_text;

const CJK_CONNECTORS: &[&str] = &[
    "然后", "而且", "并且", "因为", "所以", "但是", "如果", "为了", "以及", "还有", "并", "和",
    "与", "及", "或", "来", "去", "在", "对", "把", "将", "大约",
];
const ASCII_CONNECTORS: &[&str] = &[
    "and", "or", "to", "for", "with", "that", "which", "when", "if", "but", "so",
];

pub(super) fn line_fragment_penalty(text: &str, target_lang: &str) -> i64 {
    let normalized = normalize_inline_text(text);
    if normalized.is_empty() {
        return 0;
    }
    let char_count = normalized.chars().count();
    let ends_with_terminal = normalized
        .chars()
        .last()
        .map(is_terminal_punctuation)
        .unwrap_or(false);
    let starts_with_punct = normalized
        .chars()
        .next()
        .map(|ch| matches!(ch, ',' | '，' | '、' | '。' | ':' | '：' | ';' | '；'))
        .unwrap_or(false);
    let mut penalty = 0i64;
    if starts_with_punct {
        penalty += 8;
    }
    if char_count <= 4 && !ends_with_terminal {
        penalty += 6;
    }
    if ends_with_connector_like_fragment(&normalized, target_lang) {
        penalty += 8;
    }
    if char_count <= 8 && ends_with_short_dangling_fragment(&normalized) {
        penalty += 10;
    }
    penalty
}

pub(super) fn is_terminal_punctuation(ch: char) -> bool {
    matches!(
        ch,
        '.' | '!' | '?' | ';' | '。' | '！' | '？' | '；' | '，' | ','
    )
}

pub(super) fn ends_with_short_dangling_fragment(text: &str) -> bool {
    let normalized = normalize_inline_text(text);
    if normalized.is_empty() {
        return false;
    }
    let suffixes = ["一个", "做一个", "这个", "那个", "这笔", "那笔", "这", "那"];
    suffixes.iter().any(|suffix| normalized.ends_with(suffix))
}

pub(super) fn ends_with_connector_like_fragment(text: &str, target_lang: &str) -> bool {
    let normalized = normalize_inline_text(text);
    if normalized.is_empty() {
        return false;
    }
    let last = last_lexical_token(&normalized, target_lang);
    if last.is_empty() {
        return false;
    }
    if use_char_units(target_lang, &normalized) {
        return CJK_CONNECTORS.contains(&last.as_str());
    }
    ASCII_CONNECTORS.contains(&last.as_str())
}

/// Last grammatical token of a subtitle line. Latin uses the last whitespace
/// word; Chinese uses the last jieba word so "photo"/"共和国" are not treated
/// as dangling "to"/"和".
pub(super) fn last_lexical_token(text: &str, target_lang: &str) -> String {
    let normalized = normalize_inline_text(text);
    if normalized.is_empty() {
        return String::new();
    }
    if use_char_units(target_lang, &normalized) {
        let lower = target_lang.trim().to_ascii_lowercase();
        if lower.starts_with("zh") || lower.starts_with("yue") {
            return last_jieba_token(&normalized);
        }
        return last_cjk_char_run(&normalized);
    }
    last_latin_token(&normalized)
}

fn last_latin_token(text: &str) -> String {
    text.split_whitespace()
        .last()
        .map(|tok| {
            tok.trim_matches(|c: char| !c.is_alphanumeric())
                .to_ascii_lowercase()
        })
        .unwrap_or_default()
}

fn last_cjk_char_run(text: &str) -> String {
    CJK_CONNECTORS
        .iter()
        .filter(|c| text.ends_with(**c))
        .max_by_key(|c| c.chars().count())
        .map(|c| (*c).to_string())
        .unwrap_or_else(|| text.chars().last().map(String::from).unwrap_or_default())
}

fn last_jieba_token(text: &str) -> String {
    use jieba_rs::{Jieba, TokenizeMode};
    use std::sync::OnceLock;
    static JIEBA: OnceLock<Jieba> = OnceLock::new();
    let jieba = JIEBA.get_or_init(Jieba::new);
    jieba
        .tokenize(text, TokenizeMode::Default, true)
        .last()
        .map(|t| t.word.to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latin_connector_matches_word_not_suffix() {
        assert!(ends_with_connector_like_fragment("we had to stop and", "en"));
        assert!(!ends_with_connector_like_fragment("I took a photo", "en"));
        assert!(!ends_with_connector_like_fragment("walk into", "en"));
        assert!(!ends_with_connector_like_fragment("also", "en"));
    }
}
