use std::collections::HashSet;

use super::numbers::{normalize_numeric_value, parse_ascii_number};
use super::text_utils::{is_meaningful_text_char, normalize_inline_text};

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

pub(super) fn prepend_missing_number_token(text: &str, number: &str) -> String {
    let normalized = sanitize_translation_candidate(text);
    if normalized.is_empty() {
        return normalized;
    }
    let mut numbers = Vec::<String>::new();
    if !number.trim().is_empty() {
        numbers.push(number.to_string());
    }
    let mut body = normalized.clone();
    for _ in 0..3 {
        let Some(leading) = leading_number_anchor(&body) else {
            break;
        };
        numbers.push(leading);
        let stripped = strip_leading_number_token(&body);
        if stripped.is_empty() || stripped == body {
            break;
        }
        body = stripped;
    }
    numbers.sort_by(|left, right| {
        let left_num = left.parse::<f64>().ok();
        let right_num = right.parse::<f64>().ok();
        match (left_num, right_num) {
            (Some(a), Some(b)) => a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal),
            _ => left.cmp(right),
        }
    });
    numbers.dedup();
    if numbers.is_empty() {
        return body;
    }
    let prefix = numbers.join("/");
    if body.is_empty() {
        return prefix;
    }
    normalize_inline_text(&format!("{prefix} {body}"))
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

pub(super) fn strip_leading_number_token(text: &str) -> String {
    let trimmed = text.trim_start();
    if trimmed.is_empty() {
        return String::new();
    }
    let chars = trimmed.char_indices().collect::<Vec<_>>();
    if chars.is_empty() {
        return String::new();
    }
    let mut number_end = 0usize;
    for (idx, ch) in &chars {
        if ch.is_ascii_digit() || *ch == '.' || *ch == ',' {
            number_end = idx + ch.len_utf8();
            continue;
        }
        break;
    }
    if number_end == 0 {
        return sanitize_translation_candidate(trimmed);
    }
    let remainder = trimmed
        .get(number_end..)
        .unwrap_or_default()
        .trim_start_matches(|value: char| {
            value.is_whitespace()
                || value == ','
                || value == '，'
                || value == '、'
                || value == ':'
                || value == '：'
                || value == '-'
        });
    sanitize_translation_candidate(remainder)
}

pub(super) fn trim_before_leaked_number_anchor(
    text: &str,
    leaked_numbers: &HashSet<String>,
) -> Option<String> {
    if leaked_numbers.is_empty() {
        return None;
    }
    let mut chars = text.char_indices().peekable();
    while let Some((start, ch)) = chars.next() {
        if !ch.is_ascii_digit() {
            continue;
        }
        let mut end = start + ch.len_utf8();
        while let Some((idx, next_ch)) = chars.peek().copied() {
            if next_ch.is_ascii_digit() || next_ch == '.' || next_ch == ',' {
                end = idx + next_ch.len_utf8();
                chars.next();
                continue;
            }
            break;
        }
        let raw = text.get(start..end).unwrap_or_default();
        let normalized = normalize_numeric_value(parse_ascii_number(raw));
        if normalized.is_empty() || !leaked_numbers.contains(&normalized) {
            continue;
        }
        let head = text
            .get(..start)
            .unwrap_or_default()
            .trim_end_matches(|value: char| {
                value.is_whitespace()
                    || value == ','
                    || value == '，'
                    || value == '、'
                    || value == '：'
                    || value == ':'
                    || value == '-'
            });
        let trimmed = sanitize_translation_candidate(head);
        if trimmed.is_empty() {
            return None;
        }
        return Some(trimmed);
    }
    None
}

pub(super) fn has_tail_ellipsis(text: &str) -> bool {
    let trimmed = text.trim_end();
    if trimmed.ends_with("...") || trimmed.ends_with('…') || trimmed.ends_with("。。") {
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

pub(super) fn is_unusable_translation(text: &str) -> bool {
    let normalized = normalize_inline_text(text);
    if normalized.is_empty() {
        return true;
    }
    if has_tail_ellipsis(&normalized) {
        return true;
    }
    is_punctuation_only(&normalized)
}

pub(super) fn is_punctuation_only(text: &str) -> bool {
    let normalized = normalize_inline_text(text);
    if normalized.is_empty() {
        return true;
    }
    !normalized.chars().any(is_meaningful_text_char)
}
