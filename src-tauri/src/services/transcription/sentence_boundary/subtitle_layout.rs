//! Subtitle-length-aware segmentation of semantic spans.
//!
//! After `semantic.rs` splits the word stream at hard boundaries (terminal
//! punctuation, long pauses), some resulting spans still exceed the subtitle
//! length budget. This module re-splits those overlong spans using a greedy
//! constraint-satisfaction algorithm that:
//!   - never produces fragments below `min_units`,
//!   - never produces segments above `limit * KEEP_INTACT_RATIO`,
//!   - prefers high-quality boundaries (terminal punctuation > clause
//!     punctuation > comma > connector > pause > plain word boundary).
//!
//! All length accounting is language-aware via `token_units` / `use_char_units`
//! (CJK languages count characters; Latin/Germanic count words), so the same
//! algorithm works across every supported source language.

use crate::services::transcribe::WordTokenDto;
use voxtrans_core::subtitle::text_rules::has_break_terminal_punctuation;

use super::timing::gap_ms;
use super::types::SplitReason;

const PAUSE_BONUS_GAP_MS: u64 = 350;
/// Hard ceiling: a segment up to this ratio of `limit` is kept intact. Above
/// it, the greedy splitter must find a cut.
const KEEP_INTACT_RATIO: f64 = 1.15;

/// Split overlong semantic spans into subtitle-length segments.
///
/// Returns absolute word indices; each index is the last word of a left
/// segment (i.e. a cut happens *after* that word).
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
    let min_units = (limit * 0.5).max(3.0);
    let max_units = limit * KEEP_INTACT_RATIO;

    let mut out = Vec::<(usize, SplitReason)>::new();
    for &(span_start, span_end) in semantic_spans {
        if span_start >= words.len() || span_end >= words.len() || span_start >= span_end {
            continue;
        }
        for cut in greedy_split_span(
            words,
            span_start,
            span_end,
            source_lang,
            limit,
            min_units,
            max_units,
        ) {
            out.push((cut, SplitReason::SubtitleLayout));
        }
    }
    out
}

/// Greedily split `start..=end`. Each resulting segment stays within
/// `[min_units, max_units]`. Returns absolute cut indices (the last word
/// index of each left segment).
fn greedy_split_span(
    words: &[WordTokenDto],
    start: usize,
    end: usize,
    source_lang: &str,
    limit: f64,
    min_units: f64,
    max_units: f64,
) -> Vec<usize> {
    let total = span_units(words, start, end, source_lang);
    if total <= max_units {
        return Vec::new();
    }

    let mut cuts = Vec::new();
    let mut seg_start = start;
    let mut guard = 0usize;
    loop {
        guard += 1;
        if guard > words.len() + 1 {
            break; // safety: never loop forever
        }
        let remaining = span_units(words, seg_start, end, source_lang);
        if remaining <= max_units {
            break;
        }
        match find_best_greedy_cut(words, seg_start, end, source_lang, limit, min_units) {
            Some(i) => {
                if i >= end {
                    break;
                }
                cuts.push(i);
                seg_start = i + 1;
            }
            None => {
                // No candidate satisfied both-side min_units AND landed within
                // limit*1.5. The span is overlong but every balanced cut leaves
                // a too-short tail. Force a cut at the position closest to
                // `limit` so we make progress instead of keeping an overlong line.
                match force_cut_near_limit(words, seg_start, end, source_lang, limit, min_units) {
                    Some(i) => {
                        if i >= end {
                            break;
                        }
                        cuts.push(i);
                        seg_start = i + 1;
                    }
                    None => break,
                }
            }
        }
    }
    cuts
}

/// Find the best cut position after some index in `[seg_start, end)`.
///
/// Scans the whole span (no early break), collecting candidates by quality:
///   - `best_ideal`: left in `[min_units, limit]`, right `>= min_units`
///   - `best_extended`: left in `(limit, limit*1.5]`, right `>= min_units`
///
/// Returns the best candidate, or `None` if no position satisfies both-side
/// `min_units` (caller falls back to `force_cut_near_limit`).
fn find_best_greedy_cut(
    words: &[WordTokenDto],
    seg_start: usize,
    end: usize,
    source_lang: &str,
    limit: f64,
    min_units: f64,
) -> Option<usize> {
    let total_from = span_units(words, seg_start, end, source_lang);
    let scan_cap = (limit * 1.5).max(total_from);

    let mut acc = 0.0_f64;
    let mut best_ideal: Option<(usize, u8, f64)> = None;
    let mut best_extended: Option<(usize, u8, f64)> = None;

    for i in seg_start..end {
        acc += token_units(&words[i].word, source_lang);
        if acc > scan_cap {
            break;
        }
        if acc < min_units {
            continue;
        }
        let right = total_from - acc;
        if right < min_units {
            continue;
        }

        let rank = boundary_rank(words, i);
        let candidate = (i, rank, acc);

        if acc <= limit {
            best_ideal = pick_better(best_ideal, candidate);
        } else {
            best_extended = pick_better(best_extended, candidate);
        }
    }

    best_ideal.or(best_extended).map(|(i, _, _)| i)
}

/// Last-resort cut: used when `find_best_greedy_cut` returns `None` (no
/// position satisfies both-side `min_units`). Picks the position closest to
/// `limit` that keeps left `>= min_units`, ignoring the right-side constraint.
/// This guarantees progress on overlong spans where every balanced split
/// leaves a short tail.
fn force_cut_near_limit(
    words: &[WordTokenDto],
    seg_start: usize,
    end: usize,
    source_lang: &str,
    limit: f64,
    min_units: f64,
) -> Option<usize> {
    let mut acc = 0.0_f64;
    let mut best: Option<(usize, f64)> = None; // (index, distance to limit)
    for i in seg_start..end {
        acc += token_units(&words[i].word, source_lang);
        if acc < min_units {
            continue;
        }
        if acc > limit * 2.0 {
            break;
        }
        let dist = (acc - limit).abs();
        match best {
            None => best = Some((i, dist)),
            Some((_, bd)) if dist < bd => best = Some((i, dist)),
            _ => {}
        }
    }
    best.map(|(i, _)| i)
}

/// Tie-breaker helper: lower rank wins; among equal ranks, prefer left units
/// closer to `limit` (i.e. larger left within budget, to fill the line).
fn pick_better(current: Option<(usize, u8, f64)>, candidate: (usize, u8, f64)) -> Option<(usize, u8, f64)> {
    match current {
        None => Some(candidate),
        Some((_cur_i, cur_rank, cur_units)) => {
            let (_cand_i, cand_rank, cand_units) = candidate;
            if cand_rank < cur_rank || (cand_rank == cur_rank && cand_units > cur_units) {
                Some(candidate)
            } else {
                current
            }
        }
    }
}

/// Classify the boundary after word `i`. Lower rank = better place to cut.
fn boundary_rank(words: &[WordTokenDto], i: usize) -> u8 {
    let Some(left) = words.get(i) else {
        return 9;
    };
    let Some(right) = words.get(i + 1) else {
        return 0;
    };

    // Never cut inside paired punctuation or numbers.
    if ends_with_opening_punctuation(&left.word) || starts_with_closing_punctuation(&right.word) {
        return 9;
    }
    if is_numeric_continuation(&left.word, &right.word) {
        return 9;
    }

    // Rank 1: terminal punctuation (. ! ? 。 ！ ？) — best split point.
    if has_break_terminal_punctuation(&left.word) {
        return 1;
    }
    // Rank 2: soft clause punctuation (; : ， ； ：).
    if ends_with_soft_punctuation(&left.word) {
        return 2;
    }
    // Rank 3: comma.
    if left.word.trim_end().ends_with(',') {
        return 3;
    }
    // Rank 4: before a connector word (and/but/so/because ...).
    if is_connector_like(&right.word) && !is_connector_like(&left.word) {
        return 4;
    }
    // Rank 5: notable pause (>= 350ms).
    if gap_ms(left.end, right.start) >= PAUSE_BONUS_GAP_MS {
        return 5;
    }
    // Rank 6: plain word boundary.
    6
}

// ---- language-aware length accounting (shared) ----

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

/// CJK/Korean languages count characters; everything else counts words.
/// A token containing any CJK char also uses char units regardless of the
/// declared language (handles mixed-language tokens).
fn use_char_units(source_lang: &str, text: &str) -> bool {
    let lang = source_lang.trim().to_ascii_lowercase();
    lang.starts_with("zh")
        || lang.starts_with("yue")
        || lang.starts_with("ja")
        || lang.starts_with("ko")
        || text.chars().any(is_cjk_char)
}

/// Character-based unit count for CJK: each CJK/alphabetic char is 1 unit;
/// a run of ASCII alphanumeric chars (e.g. "GDP") counts as 1 unit so Latin
/// words embedded in CJK text don't inflate the count.
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
