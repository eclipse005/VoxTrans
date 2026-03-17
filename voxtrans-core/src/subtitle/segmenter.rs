use parakeet_rs::TimedToken;

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
    out
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

pub fn plain_text_from_segments(segments: &[SubtitleSegment]) -> String {
    assemble_plain_text(segments)
}

fn is_standalone_punctuation_token(token: &str) -> bool {
    token
        .chars()
        .all(|c| !c.is_alphanumeric() && !c.is_whitespace())
}

fn round_millis(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}
