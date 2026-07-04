use super::numbers::{normalize_numeric_value, parse_ascii_number};
use super::text_utils::normalize_inline_text;

pub(super) fn sanitize_translation_candidate(raw: &str) -> String {
    let mut text = normalize_inline_text(raw);
    if text.is_empty() {
        return text;
    }
    if has_tail_ellipsis(&text) {
        let trimmed = strip_tail_ellipsis(&text);
        if !trimmed.is_empty() {
            text = trimmed;
        }
    }
    normalize_inline_text(&text)
}

pub(super) fn leading_number_anchor(text: &str) -> Option<String> {
    let trimmed = text.trim_start();
    if trimmed.is_empty() {
        return None;
    }
    let mut raw = String::new();
    for ch in trimmed.chars() {
        if ch.is_ascii_digit() || ch == '.' || ch == ',' {
            raw.push(ch);
            continue;
        }
        break;
    }
    if raw.is_empty() {
        return None;
    }
    let value = parse_ascii_number(&raw);
    let normalized = normalize_numeric_value(value);
    if normalized.is_empty() {
        return None;
    }
    Some(normalized)
}

pub(super) fn has_tail_ellipsis(text: &str) -> bool {
    let trimmed = text.trim_end();
    if trimmed.ends_with("...") || trimmed.ends_with('…') || trimmed.ends_with("。.") {
        return true;
    }
    let mut tail_marks = 0usize;
    for ch in trimmed.chars().rev() {
        if ch.is_whitespace() {
            continue;
        }
        if ch == '.' || ch == '。' || ch == '…' {
            tail_marks += 1;
            continue;
        }
        break;
    }
    tail_marks >= 2
}

pub(super) fn strip_tail_ellipsis(text: &str) -> String {
    let mut out = text.trim_end().chars().collect::<Vec<_>>();
    while let Some(ch) = out.last().copied() {
        if ch.is_whitespace() || ch == '.' || ch == '…' || ch == '。' {
            out.pop();
            continue;
        }
        break;
    }
    normalize_inline_text(&out.into_iter().collect::<String>())
}
