use super::language_units::is_hangul_char;
use super::text_utils::normalize_inline_text;
use super::types::Step5Token;

pub(super) fn build_source_from_tokens(tokens: &[Step5Token]) -> String {
    let mut out = String::new();
    let mut prev_has_spacing_word = false;
    let mut prev_allows_space_after = false;

    for token in tokens {
        let text = token.text.trim();
        if text.is_empty() {
            continue;
        }
        let next_has_spacing_word = source_token_has_spacing_word(text);
        if !out.is_empty()
            && next_has_spacing_word
            && !source_token_starts_attached(text)
            && (prev_has_spacing_word || prev_allows_space_after)
        {
            out.push(' ');
        }
        out.push_str(text);
        prev_has_spacing_word = next_has_spacing_word;
        prev_allows_space_after = source_token_allows_space_after(text);
    }
    normalize_inline_text(&out)
}

fn source_token_has_spacing_word(token: &str) -> bool {
    token
        .chars()
        .any(|ch| ch.is_ascii_alphanumeric() || is_hangul_char(ch))
}

fn source_token_starts_attached(token: &str) -> bool {
    token
        .chars()
        .next()
        .map(|ch| ch == '\'' || ch == '’' || ch.is_ascii_punctuation())
        .unwrap_or(false)
}

fn source_token_allows_space_after(token: &str) -> bool {
    token
        .chars()
        .last()
        .map(|ch| {
            matches!(
                ch,
                ',' | ';' | ':' | '?' | '!' | '.' | '，' | '；' | '：' | '？' | '！' | '。'
            )
        })
        .unwrap_or(false)
}
