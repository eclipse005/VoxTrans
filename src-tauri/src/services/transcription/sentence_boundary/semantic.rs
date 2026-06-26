use crate::services::transcribe::WordTokenDto;
use voxtrans_core::subtitle::text_rules::{ends_with_terminal_punctuation, strip_trailing_closers};

use super::language::LanguageProfile;
use super::types::SplitReason;

/// Pre-split (hard boundaries): only terminal punctuation (`. ! ? 。`).
///
/// A terminal-punctuation boundary is always a sentence end: every `.` / `!`
/// / `?` produces a split, unless the token is a known abbreviation for the
/// current language (queried via [`LanguageProfile::abbreviations`]), or a
/// single-letter initial that forms a chain with the next token (`J. K.`).
///
/// VAD silence crossings are deliberately NOT hard-split here. They are handled
/// by the DP cost function in `subtitle_layout.rs`, which only splits when a
/// span exceeds the length budget — preventing mid-sentence fragmentation on
/// short sentences that merely contain a breath pause.
pub(super) fn build_split_points_from_hard_boundaries(
    words: &[WordTokenDto],
    profile: &dyn LanguageProfile,
) -> Vec<(usize, SplitReason)> {
    let mut out = Vec::<(usize, SplitReason)>::new();
    let abbrs = profile.abbreviations();
    for index in 0..words.len() {
        if !is_terminal_end(&words[index].word, abbrs) {
            continue;
        }
        // Single-letter dotted token (B./A./J.): only suppress the split when
        // it forms an initial chain with the next token. An isolated single-
        // letter token is a real sentence end (e.g. "step one B.").
        if is_single_letter_dotted(&words[index].word) {
            let continues = words
                .get(index + 1)
                .map(|next| is_single_letter_dotted(&next.word))
                .unwrap_or(false);
            if continues {
                continue;
            }
        }
        push_split_point(&mut out, index, SplitReason::TerminalPunctuation);
    }
    out
}

/// Does `token` end with a sentence-terminal mark that should force a split?
/// Returns false for language-specific abbreviations in `abbrs`.
fn is_terminal_end(token: &str, abbrs: &[&str]) -> bool {
    let normalized = strip_trailing_closers(token.trim());
    if normalized.is_empty() || !ends_with_terminal_punctuation(normalized) {
        return false;
    }
    let lower = normalized.to_ascii_lowercase();
    !abbrs.iter().any(|a| *a == lower.as_str())
}

/// Is `token` a single ASCII letter followed by a dot (e.g. `B.`, `A.`)?
fn is_single_letter_dotted(token: &str) -> bool {
    let chars: Vec<char> = strip_trailing_closers(token.trim()).chars().collect();
    chars.len() == 2 && chars[0].is_ascii_alphabetic() && chars[1] == '.'
}

#[cfg(test)]
pub(super) fn build_deterministic_split_points(
    words: &[WordTokenDto],
) -> Vec<(usize, SplitReason)> {
    use super::language::profile_for_lang;
    build_split_points_from_hard_boundaries(words, &*profile_for_lang("en"))
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
