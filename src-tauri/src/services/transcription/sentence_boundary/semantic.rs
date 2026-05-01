use crate::services::transcribe::WordTokenDto;
use voxtrans_core::subtitle::text_rules::should_split_after_terminal_token;

use super::HARD_SPLIT_GAP_MS;
use super::timing::gap_ms;
use super::types::SplitReason;

pub(super) fn build_split_points_from_hard_boundaries(
    words: &[WordTokenDto],
) -> Vec<(usize, SplitReason)> {
    build_high_priority_split_points(words)
}

#[cfg(test)]
pub(super) fn build_deterministic_split_points(
    words: &[WordTokenDto],
) -> Vec<(usize, SplitReason)> {
    build_high_priority_split_points(words)
}

fn build_high_priority_split_points(words: &[WordTokenDto]) -> Vec<(usize, SplitReason)> {
    let mut out = Vec::<(usize, SplitReason)>::new();
    for index in 0..words.len() {
        let next_word = words.get(index + 1).map(|word| word.word.as_str());
        let high_priority_reason =
            if should_split_after_terminal_token(&words[index].word, next_word) {
                Some(SplitReason::TerminalPunctuation)
            } else if index + 1 < words.len()
                && gap_ms(words[index].end, words[index + 1].start) >= HARD_SPLIT_GAP_MS
            {
                Some(SplitReason::HardPause)
            } else {
                None
            };

        if let Some(reason) = high_priority_reason {
            push_split_point(&mut out, index, reason);
        }
    }
    out
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
