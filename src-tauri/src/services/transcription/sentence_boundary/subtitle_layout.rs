use crate::services::transcribe::WordTokenDto;
use voxtrans_core::subtitle::text_rules::has_break_terminal_punctuation;

use super::timing::gap_ms;
use super::types::SplitReason;

const PAUSE_BONUS_GAP_MS: u64 = 350;
const MAX_SOFT_OVER_RATIO: f64 = 1.5;
const KEEP_INTACT_RATIO: f64 = 1.15;

pub(super) fn build_subtitle_layout_split_points(
    words: &[WordTokenDto],
    semantic_spans: &[(usize, usize)],
    source_lang: &str,
    subtitle_length_preset: &str,
) -> Vec<(usize, SplitReason)> {
    if words.len() < 2 {
        return Vec::new();
    }
    let limit = crate::services::subtitle_length::source_limit_for_preset(
        source_lang,
        subtitle_length_preset,
    ) as f64;
    let mut out = Vec::<(usize, SplitReason)>::new();
    for (start, end) in semantic_spans.iter().copied() {
        if start >= words.len() || end >= words.len() || start >= end {
            continue;
        }
        for split_end in split_span_by_dp(words, start, end, source_lang, limit) {
            out.push((split_end, SplitReason::SubtitleLayout));
        }
    }
    out
}

fn split_span_by_dp(
    words: &[WordTokenDto],
    start: usize,
    end: usize,
    source_lang: &str,
    limit: f64,
) -> Vec<usize> {
    let total_units = span_units(words, start, end, source_lang);
    if total_units <= limit * KEEP_INTACT_RATIO {
        return Vec::new();
    }

    let n = end - start + 1;
    let mut prefix = Vec::<f64>::with_capacity(n + 1);
    prefix.push(0.0);
    for word in &words[start..=end] {
        let next = prefix.last().copied().unwrap_or(0.0) + token_units(&word.word, source_lang);
        prefix.push(next);
    }

    let mut dp = vec![f64::INFINITY; n + 1];
    let mut prev = vec![None::<usize>; n + 1];
    dp[0] = 0.0;

    for i in 1..=n {
        for j in 0..i {
            if !dp[j].is_finite() {
                continue;
            }
            let abs_start = start + j;
            let abs_end = start + i - 1;
            let is_final = i == n;
            let units = prefix[i] - prefix[j];
            if units <= 0.0 {
                continue;
            }
            let cost = segment_cost(
                words, abs_start, abs_end, start, end, units, limit, is_final,
            );
            let candidate = dp[j] + cost;
            if candidate < dp[i] {
                dp[i] = candidate;
                prev[i] = Some(j);
            }
        }
    }

    if !dp[n].is_finite() {
        return fallback_split(words, start, end, source_lang, limit);
    }

    let mut cuts = Vec::<usize>::new();
    let mut cursor = n;
    while let Some(j) = prev[cursor] {
        if j == 0 {
            break;
        }
        cuts.push(start + j - 1);
        cursor = j;
    }
    cuts.reverse();

    if cuts.is_empty() && total_units > limit * MAX_SOFT_OVER_RATIO {
        return fallback_split(words, start, end, source_lang, limit);
    }
    cuts
}

fn segment_cost(
    words: &[WordTokenDto],
    start: usize,
    end: usize,
    span_start: usize,
    span_end: usize,
    units: f64,
    limit: f64,
    is_final: bool,
) -> f64 {
    let ratio = units / limit.max(1.0);
    let mut cost = 1.5;
    if ratio > MAX_SOFT_OVER_RATIO {
        cost += 500.0 + (ratio - MAX_SOFT_OVER_RATIO) * 200.0;
    } else if ratio > 1.0 {
        cost += (ratio - 1.0).powi(2) * 35.0;
    } else {
        cost += (1.0 - ratio).powi(2) * 4.0;
    }

    let whole_span = start == span_start && end == span_end;
    let min_units = (limit * 0.35).max(3.0);
    if !whole_span && units < min_units {
        cost += (min_units - units) * 5.0;
    }

    let duration_ms = segment_duration_ms(words, start, end);
    if !whole_span && duration_ms < 800 {
        cost += 18.0;
    }
    if duration_ms > 8_000 {
        cost += ((duration_ms - 8_000) as f64 / 1000.0) * 4.0;
    }

    if !is_final {
        cost += boundary_cost(words, end);
    }

    cost
}

fn boundary_cost(words: &[WordTokenDto], end: usize) -> f64 {
    let Some(left) = words.get(end) else {
        return 100.0;
    };
    let Some(right) = words.get(end + 1) else {
        return 0.0;
    };

    if ends_with_opening_punctuation(&left.word) || starts_with_closing_punctuation(&right.word) {
        return 120.0;
    }
    if is_numeric_continuation(&left.word, &right.word) {
        return 100.0;
    }
    if has_break_terminal_punctuation(&left.word) {
        return -10.0;
    }
    if ends_with_soft_punctuation(&left.word) {
        return -7.0;
    }
    if gap_ms(left.end, right.start) >= PAUSE_BONUS_GAP_MS {
        return -4.0;
    }
    if is_connector_like(&right.word) && !is_connector_like(&left.word) {
        return -2.0;
    }
    if is_connector_like(&left.word) {
        return 14.0;
    }
    6.0
}

fn fallback_split(
    words: &[WordTokenDto],
    start: usize,
    end: usize,
    source_lang: &str,
    limit: f64,
) -> Vec<usize> {
    let mut out = Vec::<usize>::new();
    let mut cursor = start;
    while cursor < end {
        let mut best = None::<(usize, f64)>;
        for idx in cursor..end {
            let units = span_units(words, cursor, idx, source_lang);
            let distance = (limit - units).abs();
            let boundary = boundary_cost(words, idx);
            let score = distance + boundary.max(0.0);
            if best
                .map(|(_, best_score)| score < best_score)
                .unwrap_or(true)
            {
                best = Some((idx, score));
            }
            if units >= limit {
                break;
            }
        }
        let Some((split_end, _)) = best else {
            break;
        };
        if split_end >= end {
            break;
        }
        out.push(split_end);
        cursor = split_end + 1;
    }
    out
}

fn span_units(words: &[WordTokenDto], start: usize, end: usize, source_lang: &str) -> f64 {
    words[start..=end]
        .iter()
        .map(|word| token_units(&word.word, source_lang))
        .sum::<f64>()
}

fn token_units(token: &str, source_lang: &str) -> f64 {
    if use_char_units(source_lang, token) {
        let units = count_char_units(token);
        if units == 0 { 0.0 } else { units as f64 }
    } else if token
        .chars()
        .any(|ch| ch.is_alphanumeric() || is_cjk_char(ch))
    {
        1.0
    } else {
        0.0
    }
}

fn use_char_units(source_lang: &str, text: &str) -> bool {
    let lang = source_lang.trim().to_ascii_lowercase();
    lang.starts_with("zh")
        || lang.starts_with("yue")
        || lang.starts_with("ja")
        || lang.starts_with("ko")
        || text.chars().any(is_cjk_char)
}

fn count_char_units(text: &str) -> usize {
    let mut total = 0usize;
    let mut in_ascii_group = false;
    for ch in text.chars() {
        if ch.is_whitespace() {
            if in_ascii_group {
                total += 1;
                in_ascii_group = false;
            }
            continue;
        }
        if ch.is_ascii_alphanumeric() {
            in_ascii_group = true;
            continue;
        }
        if in_ascii_group {
            total += 1;
            in_ascii_group = false;
        }
        if is_counted_char(ch) {
            total += 1;
        }
    }
    if in_ascii_group {
        total += 1;
    }
    total
}

fn is_counted_char(ch: char) -> bool {
    is_cjk_char(ch) || ch.is_alphanumeric()
}

fn is_cjk_char(ch: char) -> bool {
    matches!(
        ch as u32,
        0x3040..=0x30FF
            | 0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xF900..=0xFAFF
            | 0xAC00..=0xD7AF
    )
}

fn segment_duration_ms(words: &[WordTokenDto], start: usize, end: usize) -> u64 {
    let Some(left) = words.get(start) else {
        return 0;
    };
    let Some(right) = words.get(end) else {
        return 0;
    };
    ((right.end.max(left.start) - left.start).max(0.0) * 1000.0).round() as u64
}

fn ends_with_soft_punctuation(token: &str) -> bool {
    token
        .trim_end()
        .chars()
        .last()
        .map(|ch| matches!(ch, ',' | ';' | ':' | '，' | '、' | '；' | '：'))
        .unwrap_or(false)
}

fn ends_with_opening_punctuation(token: &str) -> bool {
    token
        .trim_end()
        .chars()
        .last()
        .map(|ch| {
            matches!(
                ch,
                '(' | '[' | '{' | '（' | '【' | '「' | '『' | '《' | '“' | '‘'
            )
        })
        .unwrap_or(false)
}

fn starts_with_closing_punctuation(token: &str) -> bool {
    token
        .trim_start()
        .chars()
        .next()
        .map(|ch| {
            matches!(
                ch,
                ')' | ']' | '}' | '）' | '】' | '」' | '』' | '》' | '”' | '’'
            )
        })
        .unwrap_or(false)
}

fn is_numeric_continuation(left: &str, right: &str) -> bool {
    let left_has_digit = left.chars().any(|ch| ch.is_ascii_digit());
    let right_has_digit = right.chars().any(|ch| ch.is_ascii_digit());
    if !left_has_digit || !right_has_digit {
        return false;
    }
    let left_tail = left.trim_end().chars().last();
    let right_head = right.trim_start().chars().next();
    matches!(left_tail, Some('$' | '¥' | '€' | '£' | '.' | ',' | '%'))
        || matches!(right_head, Some('%' | '.' | ',' | '$' | '¥' | '€' | '£'))
}

fn is_connector_like(token: &str) -> bool {
    let lower = token
        .trim_matches(|ch: char| !ch.is_alphanumeric())
        .to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "and"
            | "but"
            | "or"
            | "so"
            | "because"
            | "when"
            | "while"
            | "which"
            | "that"
            | "if"
            | "then"
            | "though"
            | "although"
            | "however"
            | "therefore"
            | "pero"
            | "porque"
            | "donc"
            | "mais"
            | "und"
            | "oder"
            | "aber"
            | "quindi"
    )
}
