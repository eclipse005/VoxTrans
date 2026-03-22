use parakeet_rs::TimedToken;
use crate::subtitle::text_rules::strip_trailing_closers;

use super::srt::SubtitleSegment;

mod heuristic_cost;
mod semantic_split;
mod text_assembly;

use heuristic_cost::recursive_balance_split;
use semantic_split::split_by_semantic_boundary;
use text_assembly::{plain_text_from_segments as assemble_plain_text, segment_from_words};

#[derive(Debug, Clone)]
pub struct WordToken {
    pub start: f64,
    pub end: f64,
    pub word: String,
}

pub fn words_from_timed_tokens(tokens: &[TimedToken]) -> Vec<WordToken> {
    tokens
        .iter()
        .filter_map(|token| {
            let word = token.text.trim().to_string();
            if word.is_empty() {
                return None;
            }
            Some(WordToken {
                start: round_millis(token.start.max(0.0) as f64),
                end: round_millis(token.end.max(token.start) as f64),
                word,
            })
        })
        .collect()
}

pub fn normalize_word_tokens(raw_words: Vec<WordToken>) -> Vec<WordToken> {
    let mut out: Vec<WordToken> = Vec::with_capacity(raw_words.len());
    for token in raw_words {
        let text = token.word.trim().to_string();
        if text.is_empty() {
            continue;
        }
        if is_standalone_punctuation_token(&text) {
            if let Some(prev) = out.last_mut() {
                prev.word.push_str(&text);
                continue;
            }
        }
        out.push(WordToken {
            start: token.start,
            end: token.end,
            word: text,
        });
    }
    merge_compound_abbreviation_tokens(out)
}

pub fn split_english_segments(
    words: &[WordToken],
    max_words_per_segment: usize,
) -> Vec<SubtitleSegment> {
    if words.is_empty() {
        return Vec::new();
    }

    let max_words = max_words_per_segment.clamp(8, 40);
    let semantic_segments = split_by_semantic_boundary(words, 2000.0);
    let mut balanced: Vec<Vec<WordToken>> = Vec::new();
    for segment in semantic_segments {
        balanced.extend(recursive_balance_split(&segment, max_words));
    }

    balanced
        .into_iter()
        .filter(|group| !group.is_empty())
        .map(|group| segment_from_words(&group))
        .collect()
}

pub fn split_translate_segments(
    words: &[WordToken],
    subtitle_max_words_per_segment: usize,
) -> Vec<SubtitleSegment> {
    if words.is_empty() {
        return Vec::new();
    }

    let base_words = subtitle_max_words_per_segment.max(1);
    let overlong_threshold = base_words.saturating_mul(2).clamp(30, 300);
    let semantic_segments = split_by_semantic_boundary(words, 2000.0);
    let mut out_groups: Vec<Vec<WordToken>> = Vec::new();

    for segment in semantic_segments {
        out_groups.extend(split_overlong_translate_segment(&segment, overlong_threshold));
    }

    out_groups
        .into_iter()
        .filter(|group| !group.is_empty())
        .map(|group| segment_from_words(&group))
        .collect()
}

pub fn plain_text_from_segments(segments: &[SubtitleSegment]) -> String {
    assemble_plain_text(segments)
}

fn split_overlong_translate_segment(words: &[WordToken], threshold: usize) -> Vec<Vec<WordToken>> {
    if words.len() <= threshold || words.len() < 2 {
        return vec![words.to_vec()];
    }

    let split_idx = find_split_index_by_comma(words).or_else(|| find_split_index_before_connector(words));
    let Some(split_idx) = split_idx else {
        // If no comma/connector boundary exists, keep whole sentence for downstream LLM refinement.
        return vec![words.to_vec()];
    };

    if split_idx + 1 >= words.len() {
        return vec![words.to_vec()];
    }

    let left = &words[..=split_idx];
    let right = &words[split_idx + 1..];
    if left.is_empty() || right.is_empty() {
        return vec![words.to_vec()];
    }

    let mut out = split_overlong_translate_segment(left, threshold);
    out.extend(split_overlong_translate_segment(right, threshold));
    out
}

fn find_split_index_by_comma(words: &[WordToken]) -> Option<usize> {
    let min_side = 3usize;
    let mut candidates: Vec<usize> = Vec::new();
    for i in 0..words.len().saturating_sub(1) {
        let left_size = i + 1;
        let right_size = words.len().saturating_sub(left_size);
        if left_size < min_side || right_size < min_side {
            continue;
        }
        if ends_with_comma(&words[i].word) {
            candidates.push(i);
        }
    }
    choose_nearest_to_center(candidates, words.len())
}

fn find_split_index_before_connector(words: &[WordToken]) -> Option<usize> {
    let min_side = 3usize;
    let mut candidates: Vec<usize> = Vec::new();
    for i in 1..words.len() {
        let left_size = i;
        let right_size = words.len().saturating_sub(i);
        if left_size < min_side || right_size < min_side {
            continue;
        }
        if is_english_connector(&words[i].word) {
            candidates.push(i - 1);
        }
    }
    choose_nearest_to_center(candidates, words.len())
}

fn choose_nearest_to_center(candidates: Vec<usize>, len: usize) -> Option<usize> {
    if candidates.is_empty() || len < 2 {
        return None;
    }
    let center = len / 2;
    candidates.into_iter().min_by_key(|idx| idx.abs_diff(center))
}

fn ends_with_comma(word: &str) -> bool {
    strip_trailing_closers(word.trim())
        .chars()
        .last()
        .map(|c| matches!(c, ',' | '，' | '、'))
        .unwrap_or(false)
}

fn is_english_connector(word: &str) -> bool {
    const CONNECTORS: &[&str] = &[
        "and",
        "or",
        "but",
        "yet",
        "however",
        "because",
        "if",
        "when",
        "where",
        "while",
        "although",
        "though",
        "therefore",
        "consequently",
        "so",
        "then",
        "also",
        "meanwhile",
    ];
    let normalized = strip_trailing_closers(word.trim())
        .trim_matches(|c: char| {
            matches!(
                c,
                '.' | ',' | '!' | '?' | ':' | ';' | '。' | '，' | '！' | '？' | '：' | '；' | '、'
            )
        })
        .to_ascii_lowercase();
    CONNECTORS.contains(&normalized.as_str())
}

fn is_standalone_punctuation_token(token: &str) -> bool {
    token
        .chars()
        .all(|c| !c.is_alphanumeric() && !c.is_whitespace())
}

fn merge_compound_abbreviation_tokens(words: Vec<WordToken>) -> Vec<WordToken> {
    if words.len() < 2 {
        return words;
    }

    let mut out: Vec<WordToken> = Vec::with_capacity(words.len());
    let mut idx = 0usize;
    while idx < words.len() {
        if idx + 1 < words.len() {
            let left = words[idx].word.trim();
            let right = words[idx + 1].word.trim();
            if should_merge_abbreviation_pair(left, right) {
                out.push(WordToken {
                    start: words[idx].start,
                    end: words[idx + 1].end,
                    word: format!("{left}{right}"),
                });
                idx += 2;
                continue;
            }
        }

        out.push(words[idx].clone());
        idx += 1;
    }

    out
}

fn should_merge_abbreviation_pair(left: &str, right: &str) -> bool {
    if left.len() != 1 || !left.chars().all(|c| c.is_ascii_alphabetic()) {
        return false;
    }
    let l = left.to_ascii_lowercase();
    let r = right.to_ascii_lowercase();
    matches!(
        (l.as_str(), r.as_str()),
        ("a", ".m.") | ("p", ".m.") | ("u", ".s.") | ("u", ".k.")
    )
}

fn round_millis(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}
