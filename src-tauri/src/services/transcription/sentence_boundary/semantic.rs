use crate::services::transcribe::WordTokenDto;
use voxtrans_core::subtitle::text_rules::should_split_after_terminal_token;

use super::types::SplitReason;

/// A terminal-punctuation split that would leave a left segment at or below
/// this many words is suppressed if the right side still has substantial
/// content. This filters ASR punctuation noise like "Or." / "Um." / "So."
/// being isolated into single-word fragments when they're actually the start
/// of a longer utterance. A genuine short sentence (e.g. "God bless.") is
/// preserved because the *next* split or end-of-stream is close.
const MIN_LEFT_WORDS_FOR_SPLIT: usize = 3;
/// Right-side threshold: only suppress the split if the following content
/// exceeds this many words (i.e. the fragment was *not* a complete sentence).
const MIN_RIGHT_WORDS_TO_SUPPRESS: usize = 4;

/// Pre-split (hard boundaries): only terminal punctuation (`. ! ? 。`).
///
/// VAD silence crossings are deliberately NOT hard-split here. They are handled
/// by the DP cost function in `subtitle_layout.rs`, which only splits when a
/// span exceeds the length budget — preventing mid-sentence fragmentation on
/// short sentences that merely contain a breath pause.
///
/// ASR punctuation noise guard: a terminal-punctuation split that would leave
/// a very short left fragment (≤ `MIN_LEFT_WORDS_FOR_SPLIT`) with substantial
/// content following (≥ `MIN_RIGHT_WORDS_TO_SUPPRESS`) is suppressed. This
/// prevents filler words ("Or.", "Um.", "So.") from being isolated, while
/// preserving genuine short sentences ("God bless.", "You don't know that.").
pub(super) fn build_split_points_from_hard_boundaries(
    words: &[WordTokenDto],
) -> Vec<(usize, SplitReason)> {
    let mut out = Vec::<(usize, SplitReason)>::new();
    let mut last_cut: i64 = -1; // tracks the start of the current left segment
    for index in 0..words.len() {
        let next_word = words.get(index + 1).map(|word| word.word.as_str());
        if !should_split_after_terminal_token(&words[index].word, next_word) {
            continue;
        }
        let left_words = index as i64 - last_cut; // words in the would-be left segment
        let right_words = (words.len() as i64) - 1 - index as i64; // words remaining after
        // Suppress ASR-noise fragments: tiny left + substantial right => not a
        // real sentence boundary, let the DP decide on a larger context.
        if left_words < MIN_LEFT_WORDS_FOR_SPLIT as i64
            && right_words >= MIN_RIGHT_WORDS_TO_SUPPRESS as i64
        {
            continue;
        }
        push_split_point(&mut out, index, SplitReason::TerminalPunctuation);
        last_cut = index as i64;
    }
    out
}

#[cfg(test)]
pub(super) fn build_deterministic_split_points(
    words: &[WordTokenDto],
) -> Vec<(usize, SplitReason)> {
    build_split_points_from_hard_boundaries(words)
}

fn push_split_point(
    split_points: &mut Vec<(usize, SplitReason)>,
    index: usize,
    reason: SplitReason,
) {
    if split_points.last().map(|(end, _)| *end) == Some(index) {
        return;
    }
    split_points.push((index, reason));
}

pub(super) fn split_points_to_spans(
    word_total: usize,
    split_points: &[(usize, SplitReason)],
) -> Vec<(usize, usize)> {
    if word_total == 0 {
        return Vec::new();
    }

    let mut out = Vec::<(usize, usize)>::new();
    let mut cursor = 0usize;
    for (end, _) in split_points.iter().copied() {
        if end < cursor || end + 1 >= word_total {
            continue;
        }
        out.push((cursor, end));
        cursor = end + 1;
    }
    out.push((cursor, word_total - 1));
    out
}
