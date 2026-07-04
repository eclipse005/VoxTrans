use super::text_utils::normalize_inline_text;

pub(super) fn line_fragment_penalty(text: &str) -> i64 {
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
    if ends_with_connector_like_fragment(&normalized) {
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

pub(super) fn ends_with_connector_like_fragment(text: &str) -> bool {
    let normalized = normalize_inline_text(text);
    if normalized.is_empty() {
        return false;
    }
    let cjk_connectors = [
        "然后", "而且", "并且", "因为", "所以", "但是", "如果", "为了", "以及", "还有", "并", "和",
        "与", "及", "或", "来", "去", "在", "对", "把", "将", "大约",
    ];
    if cjk_connectors
        .iter()
        .any(|suffix| normalized.ends_with(suffix))
    {
        return true;
    }
    let lower = normalized.to_ascii_lowercase();
    let ascii_connectors = [
        "and", "or", "to", "for", "with", "that", "which", "when", "if", "but", "so",
    ];
    ascii_connectors
        .iter()
        .any(|suffix| lower.ends_with(suffix))
}
