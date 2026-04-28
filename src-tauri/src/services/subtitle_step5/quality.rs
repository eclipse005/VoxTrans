use std::collections::{HashMap, HashSet};

use super::text_utils::{is_meaningful_text_char, normalize_inline_text};
use super::translation_candidate::has_tail_ellipsis;

pub(super) fn split_line_quality_score(lines: &[String]) -> i64 {
    if lines.is_empty() {
        return i64::MIN / 8;
    }
    let signatures = line_signatures(lines);
    let signature_counts = signature_counts(&signatures);
    let mut unique = HashSet::<String>::new();
    let mut score = 0i64;
    for (index, line) in lines.iter().enumerate() {
        let normalized = normalize_inline_text(line);
        if normalized.is_empty() {
            score -= 40;
            continue;
        }
        score += 20;
        let signature = signatures.get(index).cloned().unwrap_or_default();
        if !signature.is_empty() {
            unique.insert(signature.clone());
        }
        if signature.len() >= 6 && signature_counts.get(&signature).copied().unwrap_or(0) >= 2 {
            score -= 18;
        }
        score -= line_fragment_penalty(&normalized);
    }
    score + (unique.len() as i64 * 6) - line_redundancy_penalty(&signatures)
}

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

pub(super) fn line_signatures(lines: &[String]) -> Vec<String> {
    lines.iter().map(|line| line_signature(line)).collect()
}

pub(super) fn signature_counts(signatures: &[String]) -> HashMap<String, usize> {
    let mut out = HashMap::<String, usize>::new();
    for signature in signatures {
        if signature.is_empty() {
            continue;
        }
        *out.entry(signature.clone()).or_insert(0) += 1;
    }
    out
}

pub(super) fn has_empty_or_duplicated_long_line(lines: &[String]) -> bool {
    if lines
        .iter()
        .any(|line| normalize_inline_text(line).is_empty() || has_tail_ellipsis(line))
    {
        return true;
    }
    let signatures = line_signatures(lines);
    let signature_counts = signature_counts(&signatures);
    signatures.iter().any(|signature| {
        signature.len() >= 6 && signature_counts.get(signature).copied().unwrap_or(0) >= 2
    })
}

fn line_redundancy_penalty(signatures: &[String]) -> i64 {
    if signatures.len() <= 1 {
        return 0;
    }
    let mut penalty = 0i64;
    for left_index in 0..signatures.len() {
        let left = signatures[left_index].as_str();
        if left.len() < 8 {
            continue;
        }
        for right_index in (left_index + 1)..signatures.len() {
            let right = signatures[right_index].as_str();
            if right.len() < 8 || left == right {
                continue;
            }
            let (shorter, longer) = if left.len() <= right.len() {
                (left, right)
            } else {
                (right, left)
            };
            if !longer.contains(shorter) {
                continue;
            }
            let overlap_ratio = shorter.len() as f64 / longer.len() as f64;
            if overlap_ratio >= 0.45 {
                penalty += if overlap_ratio >= 0.7 { 18 } else { 12 };
            }
        }
    }
    penalty
}

fn line_signature(text: &str) -> String {
    normalize_inline_text(text)
        .to_lowercase()
        .chars()
        .filter(|ch| is_meaningful_text_char(*ch))
        .collect::<String>()
}
