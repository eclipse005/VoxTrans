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
        if is_standalone_punctuation_token(&text) && !is_numeric_prefix_symbol(&text) {
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
    let out = merge_compound_abbreviation_tokens(out);
    let out = merge_compound_numeric_tokens(out);
    let out = merge_prefixed_numeric_tokens(out);
    let out = merge_compound_numeric_tokens(out);
    merge_numeric_unit_suffix_tokens(out)
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

fn merge_compound_numeric_tokens(words: Vec<WordToken>) -> Vec<WordToken> {
    let mut current = words;
    loop {
        let (next, changed) = merge_compound_numeric_tokens_once(current);
        if !changed {
            return next;
        }
        current = next;
    }
}

fn merge_prefixed_numeric_tokens(words: Vec<WordToken>) -> Vec<WordToken> {
    if words.len() < 2 {
        return words;
    }

    let mut out: Vec<WordToken> = Vec::with_capacity(words.len());
    let mut idx = 0usize;
    while idx < words.len() {
        if idx + 1 < words.len() {
            let left = words[idx].word.trim();
            let right = words[idx + 1].word.trim();
            if should_merge_numeric_prefix_pair(left, right) {
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

fn merge_numeric_unit_suffix_tokens(words: Vec<WordToken>) -> Vec<WordToken> {
    if words.len() < 2 {
        return words;
    }

    let mut out: Vec<WordToken> = Vec::with_capacity(words.len());
    let mut idx = 0usize;
    while idx < words.len() {
        if idx + 1 < words.len() {
            let left = words[idx].word.trim();
            let right = words[idx + 1].word.trim();
            if should_merge_numeric_suffix_pair(left, right) {
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

fn merge_compound_numeric_tokens_once(words: Vec<WordToken>) -> (Vec<WordToken>, bool) {
    if words.len() < 2 {
        return (words, false);
    }

    let mut out: Vec<WordToken> = Vec::with_capacity(words.len());
    let mut idx = 0usize;
    let mut changed = false;
    while idx < words.len() {
        if idx + 1 < words.len() {
            let left = words[idx].word.trim();
            let right = words[idx + 1].word.trim();
            if should_merge_numeric_pair(left, right) {
                out.push(WordToken {
                    start: words[idx].start,
                    end: words[idx + 1].end,
                    word: format!("{left}{right}"),
                });
                idx += 2;
                changed = true;
                continue;
            }
        }

        out.push(words[idx].clone());
        idx += 1;
    }

    (out, changed)
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

fn should_merge_numeric_pair(left: &str, right: &str) -> bool {
    should_merge_decimal_pair(left, right)
        || should_merge_time_pair(left, right)
        || should_merge_thousands_pair(left, right)
        || should_merge_slash_pair(left, right)
        || should_merge_hyphen_pair(left, right)
}

fn should_merge_decimal_pair(left: &str, right: &str) -> bool {
    let Some(prefix) = left.strip_suffix('.') else {
        return false;
    };
    if prefix.is_empty() || !prefix.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }

    let mut right_chars = right.chars();
    let Some(first) = right_chars.next() else {
        return false;
    };
    if !first.is_ascii_digit() {
        return false;
    }

    right
        .chars()
        .all(|c| c.is_ascii_digit() || matches!(c, '%' | '.' | ',' | '+' | '-'))
}

fn should_merge_time_pair(left: &str, right: &str) -> bool {
    let Some(prefix) = left.strip_suffix(':') else {
        return false;
    };
    if prefix.is_empty() {
        return false;
    }
    let parts: Vec<&str> = prefix.split(':').collect();
    if parts.iter().any(|part| part.is_empty() || !part.chars().all(|c| c.is_ascii_digit())) {
        return false;
    }
    !right.is_empty()
        && right
            .split(':')
            .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
}

fn should_merge_thousands_pair(left: &str, right: &str) -> bool {
    let Some(prefix) = left.strip_suffix(',') else {
        return false;
    };
    if prefix.is_empty() {
        return false;
    }
    let prefix_parts: Vec<&str> = prefix.split(',').collect();
    if prefix_parts
        .iter()
        .any(|part| part.is_empty() || !part.chars().all(|c| c.is_ascii_digit()))
    {
        return false;
    }
    right.len() == 3 && right.chars().all(|c| c.is_ascii_digit())
}

fn should_merge_slash_pair(left: &str, right: &str) -> bool {
    let Some(prefix) = left.strip_suffix('/') else {
        return false;
    };
    if prefix.is_empty() {
        return false;
    }
    let parts: Vec<&str> = prefix.split('/').collect();
    if parts
        .iter()
        .any(|part| part.is_empty() || !part.chars().all(|c| c.is_ascii_digit()))
    {
        return false;
    }
    !right.is_empty()
        && right
            .split('/')
            .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
}

fn should_merge_hyphen_pair(left: &str, right: &str) -> bool {
    let Some(prefix) = left.strip_suffix('-') else {
        return false;
    };
    if prefix.is_empty() {
        return false;
    }
    let parts: Vec<&str> = prefix.split('-').collect();
    if parts
        .iter()
        .any(|part| part.is_empty() || !part.chars().all(|c| c.is_ascii_digit()))
    {
        return false;
    }
    !right.is_empty()
        && right
            .split('-')
            .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
}

fn should_merge_numeric_prefix_pair(left: &str, right: &str) -> bool {
    is_numeric_prefix_symbol(left) && starts_with_numeric_value(right)
}

fn should_merge_numeric_suffix_pair(left: &str, right: &str) -> bool {
    is_numeric_value(left) && is_numeric_unit_suffix(right)
}

fn is_numeric_prefix_symbol(token: &str) -> bool {
    matches!(token, "$" | "€" | "£" | "¥" | "￥" | "₹")
}

fn starts_with_numeric_value(token: &str) -> bool {
    token.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false)
}

fn is_numeric_value(token: &str) -> bool {
    let trimmed = token.trim();
    !trimmed.is_empty()
        && trimmed.chars().any(|c| c.is_ascii_digit())
        && trimmed.chars().all(|c| {
            c.is_ascii_digit()
                || matches!(
                    c,
                    '.' | ',' | ':' | '/' | '-' | '%' | '+' | '$' | '€' | '£' | '¥' | '￥' | '₹'
                )
        })
}

fn is_numeric_unit_suffix(token: &str) -> bool {
    const KNOWN_UNITS: &[&str] = &[
        "k", "m", "b", "t",
        "x", "s", "ms", "kg", "g", "mg", "lb", "lbs",
        "km", "m", "cm", "mm", "ft", "in",
        "h", "hr", "hrs", "min", "mins",
        "usd", "eur", "gbp", "jpy", "cny",
        "bp", "bps",
    ];
    let lower = token.trim().to_ascii_lowercase();
    !lower.is_empty()
        && KNOWN_UNITS.contains(&lower.as_str())
}

fn round_millis(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}

#[cfg(test)]
mod tests {
    use super::{normalize_word_tokens, WordToken};

    #[test]
    fn merges_am_pm_pairs() {
        let words = vec![
            WordToken { start: 0.0, end: 0.1, word: "p".to_string() },
            WordToken { start: 0.1, end: 0.2, word: ".m.".to_string() },
        ];

        let normalized = normalize_word_tokens(words);
        assert_eq!(normalized.len(), 1);
        assert_eq!(normalized[0].word, "p.m.");
        assert_eq!(normalized[0].start, 0.0);
        assert_eq!(normalized[0].end, 0.2);
    }

    #[test]
    fn merges_decimal_percentage_pairs() {
        let words = vec![
            WordToken { start: 0.0, end: 0.1, word: "2".to_string() },
            WordToken { start: 0.1, end: 0.15, word: ".".to_string() },
            WordToken { start: 0.15, end: 0.3, word: "5%".to_string() },
        ];

        let normalized = normalize_word_tokens(words);
        assert_eq!(normalized.len(), 1);
        assert_eq!(normalized[0].word, "2.5%");
        assert_eq!(normalized[0].start, 0.0);
        assert_eq!(normalized[0].end, 0.3);
    }

    #[test]
    fn merges_time_pairs() {
        let words = vec![
            WordToken { start: 0.0, end: 0.1, word: "6".to_string() },
            WordToken { start: 0.1, end: 0.15, word: ":".to_string() },
            WordToken { start: 0.15, end: 0.25, word: "20".to_string() },
        ];

        let normalized = normalize_word_tokens(words);
        assert_eq!(normalized.len(), 1);
        assert_eq!(normalized[0].word, "6:20");
        assert_eq!(normalized[0].start, 0.0);
        assert_eq!(normalized[0].end, 0.25);
    }

    #[test]
    fn merges_multi_part_times_across_multiple_passes() {
        let words = vec![
            WordToken { start: 0.0, end: 0.1, word: "12".to_string() },
            WordToken { start: 0.1, end: 0.15, word: ":".to_string() },
            WordToken { start: 0.15, end: 0.25, word: "30".to_string() },
            WordToken { start: 0.25, end: 0.3, word: ":".to_string() },
            WordToken { start: 0.3, end: 0.4, word: "45".to_string() },
        ];

        let normalized = normalize_word_tokens(words);
        assert_eq!(normalized.len(), 1);
        assert_eq!(normalized[0].word, "12:30:45");
        assert_eq!(normalized[0].start, 0.0);
        assert_eq!(normalized[0].end, 0.4);
    }

    #[test]
    fn merges_thousands_separator_pairs() {
        let words = vec![
            WordToken { start: 0.0, end: 0.1, word: "1".to_string() },
            WordToken { start: 0.1, end: 0.12, word: ",".to_string() },
            WordToken { start: 0.12, end: 0.2, word: "000".to_string() },
        ];

        let normalized = normalize_word_tokens(words);
        assert_eq!(normalized.len(), 1);
        assert_eq!(normalized[0].word, "1,000");
        assert_eq!(normalized[0].start, 0.0);
        assert_eq!(normalized[0].end, 0.2);
    }

    #[test]
    fn merges_date_slash_pairs() {
        let words = vec![
            WordToken { start: 0.0, end: 0.1, word: "03".to_string() },
            WordToken { start: 0.1, end: 0.12, word: "/".to_string() },
            WordToken { start: 0.12, end: 0.2, word: "23".to_string() },
            WordToken { start: 0.2, end: 0.22, word: "/".to_string() },
            WordToken { start: 0.22, end: 0.32, word: "2026".to_string() },
        ];

        let normalized = normalize_word_tokens(words);
        assert_eq!(normalized.len(), 1);
        assert_eq!(normalized[0].word, "03/23/2026");
        assert_eq!(normalized[0].start, 0.0);
        assert_eq!(normalized[0].end, 0.32);
    }

    #[test]
    fn merges_date_hyphen_pairs() {
        let words = vec![
            WordToken { start: 0.0, end: 0.1, word: "2026".to_string() },
            WordToken { start: 0.1, end: 0.12, word: "-".to_string() },
            WordToken { start: 0.12, end: 0.2, word: "03".to_string() },
            WordToken { start: 0.2, end: 0.22, word: "-".to_string() },
            WordToken { start: 0.22, end: 0.3, word: "23".to_string() },
        ];

        let normalized = normalize_word_tokens(words);
        assert_eq!(normalized.len(), 1);
        assert_eq!(normalized[0].word, "2026-03-23");
        assert_eq!(normalized[0].start, 0.0);
        assert_eq!(normalized[0].end, 0.3);
    }

    #[test]
    fn merges_fraction_pairs() {
        let words = vec![
            WordToken { start: 0.0, end: 0.1, word: "3".to_string() },
            WordToken { start: 0.1, end: 0.12, word: "/".to_string() },
            WordToken { start: 0.12, end: 0.2, word: "4".to_string() },
        ];

        let normalized = normalize_word_tokens(words);
        assert_eq!(normalized.len(), 1);
        assert_eq!(normalized[0].word, "3/4");
        assert_eq!(normalized[0].start, 0.0);
        assert_eq!(normalized[0].end, 0.2);
    }

    #[test]
    fn merges_currency_prefix_pairs() {
        let words = vec![
            WordToken { start: 0.0, end: 0.05, word: "$".to_string() },
            WordToken { start: 0.05, end: 0.2, word: "12.5".to_string() },
        ];

        let normalized = normalize_word_tokens(words);
        assert_eq!(normalized.len(), 1);
        assert_eq!(normalized[0].word, "$12.5");
        assert_eq!(normalized[0].start, 0.0);
        assert_eq!(normalized[0].end, 0.2);
    }

    #[test]
    fn keeps_currency_prefix_separate_from_previous_word() {
        let words = vec![
            WordToken { start: 0.0, end: 0.1, word: "cost".to_string() },
            WordToken { start: 0.1, end: 0.12, word: "$".to_string() },
            WordToken { start: 0.12, end: 0.2, word: "10".to_string() },
        ];

        let normalized = normalize_word_tokens(words);
        assert_eq!(normalized.len(), 2);
        assert_eq!(normalized[0].word, "cost");
        assert_eq!(normalized[1].word, "$10");
        assert_eq!(normalized[1].start, 0.1);
        assert_eq!(normalized[1].end, 0.2);
    }

    #[test]
    fn merges_currency_amount_with_thousands_separator() {
        let words = vec![
            WordToken { start: 0.0, end: 0.02, word: "$".to_string() },
            WordToken { start: 0.02, end: 0.1, word: "1".to_string() },
            WordToken { start: 0.1, end: 0.12, word: ",".to_string() },
            WordToken { start: 0.12, end: 0.2, word: "000".to_string() },
        ];

        let normalized = normalize_word_tokens(words);
        assert_eq!(normalized.len(), 1);
        assert_eq!(normalized[0].word, "$1,000");
        assert_eq!(normalized[0].start, 0.0);
        assert_eq!(normalized[0].end, 0.2);
    }

    #[test]
    fn merges_numeric_unit_suffix_pairs() {
        let words = vec![
            WordToken { start: 0.0, end: 0.15, word: "10".to_string() },
            WordToken { start: 0.15, end: 0.22, word: "kg".to_string() },
        ];

        let normalized = normalize_word_tokens(words);
        assert_eq!(normalized.len(), 1);
        assert_eq!(normalized[0].word, "10kg");
        assert_eq!(normalized[0].start, 0.0);
        assert_eq!(normalized[0].end, 0.22);
    }

    #[test]
    fn keeps_already_normalized_numeric_token_unchanged() {
        let words = vec![WordToken {
            start: 0.0,
            end: 0.2,
            word: "2.5%".to_string(),
        }];

        let normalized = normalize_word_tokens(words);
        assert_eq!(normalized.len(), 1);
        assert_eq!(normalized[0].word, "2.5%");
        assert_eq!(normalized[0].start, 0.0);
        assert_eq!(normalized[0].end, 0.2);
    }
}
