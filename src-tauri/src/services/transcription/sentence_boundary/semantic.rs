use crate::services::transcribe::WordTokenDto;
use voxtrans_core::subtitle::text_rules::should_split_after_terminal_token;

use super::types::SplitReason;

/// Pre-split (hard boundaries): only terminal punctuation (`. ! ? 。`).
///
/// VAD silence crossings are deliberately NOT hard-split here. They are handled
/// by the DP cost function in `subtitle_layout.rs`, which only splits when a
/// span exceeds the length budget — preventing mid-sentence fragmentation on
/// short sentences that merely contain a breath pause.
pub(super) fn build_split_points_from_hard_boundaries(
    words: &[WordTokenDto],
) -> Vec<(usize, SplitReason)> {
    let mut out = Vec::<(usize, SplitReason)>::new();
    for index in 0..words.len() {
        let next_word = words.get(index + 1).map(|word| word.word.as_str());
        if should_split_after_terminal_token(&words[index].word, next_word) {
            push_split_point(&mut out, index, SplitReason::TerminalPunctuation);
        }
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
