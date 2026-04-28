use super::constants::SOFT_SPLIT_GAP_SECONDS;
use super::language_units::text_length_units;
use super::source_text::build_source_from_tokens;
use super::text_utils::ends_with_sentence_punctuation;
use super::types::Step5Token;

pub(super) fn choose_preferred_split_ranges(
    llm_ranges: Vec<(usize, usize)>,
    fallback_ranges: Vec<(usize, usize)>,
    tokens: &[Step5Token],
    source_lang: &str,
    source_limit: f64,
) -> Vec<(usize, usize)> {
    if llm_ranges.is_empty() {
        return fallback_ranges;
    }
    if fallback_ranges.is_empty() {
        return llm_ranges;
    }
    let llm_score = score_split_ranges(&llm_ranges, tokens, source_lang, source_limit);
    let fallback_score = score_split_ranges(&fallback_ranges, tokens, source_lang, source_limit);
    if llm_score <= fallback_score * 1.05 {
        llm_ranges
    } else {
        fallback_ranges
    }
}

fn score_split_ranges(
    ranges: &[(usize, usize)],
    tokens: &[Step5Token],
    source_lang: &str,
    source_limit: f64,
) -> f64 {
    if ranges.is_empty() {
        return 1_000_000.0;
    }
    let mut score = 0.0f64;
    let mut lengths = Vec::<f64>::new();
    for (start, end) in ranges {
        if *start >= tokens.len() || *end >= tokens.len() || end < start {
            score += 1000.0;
            continue;
        }
        let text = build_source_from_tokens(&tokens[*start..=*end]);
        let units = text_length_units(&text, source_lang);
        lengths.push(units);
        if units > source_limit {
            score += 80.0 + (units - source_limit) * 20.0;
        }
        if units < 4.0 {
            score += (4.0 - units) * 25.0 + 20.0;
        }
        if units > source_limit * 1.6 {
            score += 120.0;
        }
    }
    for window in ranges.windows(2) {
        let left = window[0];
        let right = window[1];
        let Some(left_token) = tokens.get(left.1) else {
            continue;
        };
        let Some(right_token) = tokens.get(right.0) else {
            continue;
        };
        let gap = (right_token.start - left_token.end).max(0.0);
        if gap >= SOFT_SPLIT_GAP_SECONDS || ends_with_sentence_punctuation(&left_token.text) {
            score -= 2.0;
        } else {
            score += 4.0;
        }
    }
    if lengths.len() >= 2 {
        let avg = lengths.iter().sum::<f64>() / lengths.len() as f64;
        if avg > 0.0 {
            for len in &lengths {
                let ratio = len / avg;
                if ratio > 2.4 || ratio < 0.35 {
                    score += 16.0;
                }
            }
        }
    }
    score
}
