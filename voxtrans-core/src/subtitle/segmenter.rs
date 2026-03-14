use parakeet_rs::TimedToken;

use super::srt::{SegmentWord, SubtitleSegment};

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
                // Attach punctuation to previous token and keep previous token timing.
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
    segments
        .iter()
        .map(|s| s.text.trim())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn split_by_semantic_boundary(words: &[WordToken], max_pause_ms: f64) -> Vec<Vec<WordToken>> {
    let mut segments: Vec<Vec<WordToken>> = Vec::new();
    let mut current: Vec<WordToken> = Vec::new();

    for (idx, word) in words.iter().enumerate() {
        current.push(word.clone());

        let punctuation_break = should_split_after_word(words, idx);
        let pause_break = if idx + 1 < words.len() {
            let pause_ms = (words[idx + 1].start - word.end).max(0.0) * 1000.0;
            pause_ms >= max_pause_ms
        } else {
            false
        };

        if punctuation_break || pause_break {
            segments.push(current);
            current = Vec::new();
        }
    }

    if !current.is_empty() {
        segments.push(current);
    }

    segments
}

fn recursive_balance_split(words: &[WordToken], max_words: usize) -> Vec<Vec<WordToken>> {
    if words.len() <= max_words {
        return vec![words.to_vec()];
    }

    let (split_idx, min_split_cost) = find_best_split_point(words);
    let over_limit = (words.len().saturating_sub(max_words)) as f64;
    let do_nothing_cost = over_limit * over_limit;

    if words.len() <= max_words * 2 && min_split_cost > do_nothing_cost {
        return vec![words.to_vec()];
    }

    let left = &words[..=split_idx];
    let right = &words[split_idx + 1..];
    if left.is_empty() || right.is_empty() {
        return vec![words.to_vec()];
    }

    let mut out = recursive_balance_split(left, max_words);
    out.extend(recursive_balance_split(right, max_words));
    out
}

fn find_best_split_point(words: &[WordToken]) -> (usize, f64) {
    let center = words.len() / 2;
    let mut best_idx = center.saturating_sub(1);
    let mut min_cost = f64::INFINITY;

    for i in 0..words.len().saturating_sub(1) {
        let balance_cost = ((i as isize - center as isize).abs() as f64) * 1.5;

        let mut semantic_bonus = 0.0_f64;
        if ends_with_comma(&words[i].word) {
            semantic_bonus += 15.0;
        }
        if is_connector_at(i + 1, words) {
            semantic_bonus += 12.0;
        }

        let structure_penalty = if i == 0 || i == words.len() - 2 {
            60.0
        } else {
            0.0
        };

        let total_cost = balance_cost - semantic_bonus + structure_penalty;
        if total_cost < min_cost {
            min_cost = total_cost;
            best_idx = i;
        }
    }

    (best_idx, min_cost)
}

fn segment_from_words(words: &[WordToken]) -> SubtitleSegment {
    let start = words.first().map(|w| w.start).unwrap_or(0.0);
    let end = words.last().map(|w| w.end).unwrap_or(start);
    let text = words_to_sentence_text(words);
    SubtitleSegment {
        start_sec: start,
        end_sec: end,
        text,
        words: words
            .iter()
            .map(|w| SegmentWord {
                start: w.start,
                end: w.end,
                word: w.word.clone(),
            })
            .collect(),
    }
}

fn words_to_sentence_text(words: &[WordToken]) -> String {
    let mut text = String::new();
    for token in words {
        let part = token.word.trim();
        if part.is_empty() {
            continue;
        }
        if text.is_empty() {
            text.push_str(part);
            continue;
        }

        if is_no_space_before(part) {
            text.push_str(part);
        } else {
            text.push(' ');
            text.push_str(part);
        }
    }
    text
}

fn should_split_after_word(words: &[WordToken], idx: usize) -> bool {
    let current_word = match words.get(idx) {
        Some(w) => w.word.trim(),
        None => return false,
    };
    let normalized = strip_trailing_closers(current_word);
    if normalized.is_empty() || !ends_with_terminal_punctuation(normalized) {
        return false;
    }
    if is_non_break_terminal_case(normalized) {
        return false;
    }

    if let Some(next) = words.get(idx + 1) {
        let next_token = strip_leading_openers(next.word.trim());
        if starts_with_lowercase(next_token) {
            return false;
        }
    }

    true
}

fn ends_with_terminal_punctuation(word: &str) -> bool {
    word.chars()
        .last()
        .map(is_terminal_punctuation)
        .unwrap_or(false)
}

fn ends_with_comma(word: &str) -> bool {
    strip_trailing_closers(word.trim())
        .chars()
        .last()
        .map(|c| matches!(c, ',' | '，' | '、'))
        .unwrap_or(false)
}

fn is_connector_at(idx: usize, words: &[WordToken]) -> bool {
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
    let token = match words.get(idx) {
        Some(w) => normalize_word_for_match(&w.word),
        None => return false,
    };
    CONNECTORS.contains(&token.as_str())
}

fn normalize_word_for_match(word: &str) -> String {
    strip_trailing_closers(word.trim())
        .trim_end_matches(|c: char| {
            matches!(
                c,
                '.' | ',' | '!' | '?' | ':' | ';' | '。' | '，' | '！' | '？' | '：' | '；' | '、'
            )
        })
        .to_ascii_lowercase()
}

fn is_no_space_before(word: &str) -> bool {
    matches!(
        word,
        "." | ","
            | "!"
            | "?"
            | ":"
            | ";"
            | ")"
            | "]"
            | "}"
            | "。"
            | "，"
            | "！"
            | "？"
            | "："
            | "；"
            | "、"
    )
}

fn is_standalone_punctuation_token(token: &str) -> bool {
    token
        .chars()
        .all(|c| !c.is_alphanumeric() && !c.is_whitespace())
}

fn strip_trailing_closers(token: &str) -> &str {
    token.trim_end_matches(|c: char| {
        matches!(
            c,
            '"' | '\'' | ')' | ']' | '}' | '”' | '’' | '》' | '」' | '』'
        )
    })
}

fn strip_leading_openers(token: &str) -> &str {
    token.trim_start_matches(|c: char| {
        matches!(
            c,
            '"' | '\'' | '(' | '[' | '{' | '“' | '‘' | '《' | '「' | '『'
        )
    })
}

fn is_terminal_punctuation(c: char) -> bool {
    matches!(
        c,
        '.' | '!' | '?' | '。' | '！' | '？' | '｡' | '؟' | '۔' | '።' | '။' | '…'
    )
}

fn is_non_break_terminal_case(token: &str) -> bool {
    is_common_abbreviation(token)
        || is_single_letter_initial(token)
        || looks_like_decimal_number(token)
}

fn is_common_abbreviation(token: &str) -> bool {
    let lower = token.to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "mr."
            | "mrs."
            | "ms."
            | "dr."
            | "prof."
            | "sr."
            | "jr."
            | "st."
            | "no."
            | "vs."
            | "etc."
            | "e.g."
            | "i.e."
            | "u.s."
            | "u.k."
            | "jan."
            | "feb."
            | "mar."
            | "apr."
            | "jun."
            | "jul."
            | "aug."
            | "sep."
            | "sept."
            | "oct."
            | "nov."
            | "dec."
    ) || is_single_letter_initial(&lower)
}

fn is_single_letter_initial(token: &str) -> bool {
    let chars: Vec<char> = token.chars().collect();
    chars.len() == 2 && chars[0].is_ascii_alphabetic() && chars[1] == '.'
}

fn looks_like_decimal_number(token: &str) -> bool {
    let t = strip_trailing_closers(token);
    let mut parts = t.split('.');
    let left = match parts.next() {
        Some(v) => v,
        None => return false,
    };
    let right = match parts.next() {
        Some(v) => v,
        None => return false,
    };
    if parts.next().is_some() {
        return false;
    }
    !left.is_empty()
        && !right.is_empty()
        && left.chars().all(|c| c.is_ascii_digit())
        && right.chars().all(|c| c.is_ascii_digit())
}

fn starts_with_lowercase(token: &str) -> bool {
    token
        .chars()
        .find(|c| c.is_alphabetic())
        .map(|c| c.is_lowercase())
        .unwrap_or(false)
}

fn round_millis(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}
