//! L1 — word aggregation + hard pause split (token-atomic architecture).
//!
//! The ASR token is the minimal timing unit and is never split. The DP in
//! `subtitle_layout.rs` cuts only BETWEEN words; a word is a maximal run of
//! tokens that the language's word-boundary advisor keeps together (jieba for
//! zh, one token per word elsewhere). Word = token range by construction, so a
//! word-internal cut is not a representable DP candidate — the old
//! cost-time FORBIDDEN query becomes a structural guarantee.
//!
//! Hard pause split (product decision, ep6 pilot v2): an intra-sentence
//! speech pause ≥ [`PAUSE_SPLIT_SEC`] hard-splits the display sequence. Cuts
//! snap to word boundaries (VAD silence preferred, raw token gap fallback) —
//! a pause inside a word pulls the cut to the nearest word edge instead of
//! tearing the word apart.

use super::language::Advisor;
use super::vad_align::SpeechSegmentIndex;
use crate::services::transcribe::WordTokenDto;

/// Intra-sentence speech pause at or above this width (seconds) hard-splits
/// the display sequence. Speech-synced rhythm beats length budgets here.
pub(super) const PAUSE_SPLIT_SEC: f64 = 2.0;

/// Words of one semantic span plus their token ranges (relative to the span's
/// first token). Invariant: `token_ranges` is contiguous, non-overlapping and
/// covers the span exactly — a partition. Every token belongs to exactly one
/// word; text is always a derived projection of tokens, never the reverse.
pub(super) struct SpanWords {
    /// Pseudo word tokens fed to the DP in place of ASR tokens: `word` =
    /// concatenated token texts, `start`/`end` = first-token start and
    /// last-token end (the only timing a word may expose).
    pub words: Vec<WordTokenDto>,
    /// `token_ranges[i] = (token_start, token_end)` (exclusive end), relative
    /// to the span. `words[i]` spans exactly these tokens.
    pub token_ranges: Vec<(usize, usize)>,
}

/// Aggregate a semantic span's tokens into advisor-aligned words.
///
/// `byte_offsets[k]` must be the byte offset in the span's concatenated text
/// just after the (k-1)-th token (the DP's `byte_offset` convention), so the
/// advisor (jieba for zh) can tell whether the gap after a token falls inside
/// a segmented word: `Some(false)` → same word, `Some(true)`/`None` → new
/// word. For per-token advisors this is the identity aggregation.
pub(super) fn aggregate_span_words(
    tokens: &[WordTokenDto],
    advisor: &Advisor,
    byte_offsets: &[usize],
) -> SpanWords {
    let mut words = Vec::new();
    let mut token_ranges = Vec::new();
    let mut word_start = 0usize;
    for k in 0..tokens.len() {
        let is_last = k + 1 == tokens.len();
        let same_word_as_next =
            !is_last && matches!(advisor.is_word_boundary(byte_offsets[k + 1]), Some(false));
        if is_last || !same_word_as_next {
            let text: String = tokens[word_start..=k]
                .iter()
                .map(|w| w.word.as_str())
                .collect();
            words.push(WordTokenDto {
                start: tokens[word_start].start,
                end: tokens[k].end,
                word: text,
            });
            token_ranges.push((word_start, k + 1));
            word_start = k + 1;
        }
    }
    // A word is a *token range*: the ranges must be contiguous, non-empty and
    // cover the span exactly. Every downstream cut rule relies on this — if it
    // ever breaks, a cue can be cut inside a word (the はい / は+いじゃあ bug).
    #[cfg(debug_assertions)]
    debug_assert_word_partition(tokens.len(), &token_ranges);
    SpanWords {
        words,
        token_ranges,
    }
}

/// Split a span's words at speech pauses ≥ [`PAUSE_SPLIT_SEC`]. Returns
/// inclusive word-index chunks `(start, end)` covering the span in order —
/// a partition, so chunk boundaries are mandatory cuts the DP cannot merge.
pub(super) fn hard_pause_chunks(
    span: &SpanWords,
    vad_index: &SpeechSegmentIndex,
) -> Vec<(usize, usize)> {
    let n = span.words.len();
    if n == 0 {
        // No words → no chunks. Guarding here keeps `n - 1` below from
        // underflowing on an empty span.
        return Vec::new();
    }
    let mut chunks = Vec::new();
    let mut start = 0usize;
    for k in 0..n.saturating_sub(1) {
        let left = &span.words[k];
        let right = &span.words[k + 1];
        let gap = (right.start - left.end).max(0.0);
        let vad = if vad_index.crosses_silence(left.end, right.start) {
            vad_index.silence_duration_sec(left.end, right.start)
        } else {
            0.0
        };
        if gap.max(vad) >= PAUSE_SPLIT_SEC {
            chunks.push((start, k));
            start = k + 1;
        }
    }
    chunks.push((start, n - 1));

    // A pause cut must never manufacture a cue with nothing to read. The
    // semantic layer already merges pure function tails back into their
    // predecessor, but this is the layer that actually creates cue
    // boundaries — a ≥2 s pause right before a stranded particle used to
    // re-split exactly what that guard had just merged (real case: 今回 |
    // の | シンデレラ across three 6 s pauses). Enforce the same invariant
    // here, where the cut is made.
    let mut merged: Vec<(usize, usize)> = Vec::with_capacity(chunks.len());
    for (chunk_start, chunk_end) in chunks {
        match merged.last().copied() {
            Some((prev_start, prev_end))
                if chunk_start == prev_end + 1
                    && is_contentless_chunk(span, chunk_start, chunk_end) =>
            {
                merged.pop();
                merged.push((prev_start, chunk_end));
            }
            _ => merged.push((chunk_start, chunk_end)),
        }
    }
    // The pause cuts (plus the contentless merge above) must partition every
    // word: contiguous, in order, covering the span exactly.
    #[cfg(debug_assertions)]
    debug_assert_partition(&merged, n);
    merged
}

/// Debug-only: `ranges` must be an exact partition of `len` (contiguous,
/// non-empty, full coverage, in order).
#[cfg(debug_assertions)]
fn debug_assert_word_partition(len: usize, ranges: &[(usize, usize)]) {
    let mut cursor = 0usize;
    for (start, end) in ranges {
        debug_assert_eq!(*start, cursor, "word ranges must be contiguous");
        debug_assert!(*end > *start, "word ranges must be non-empty");
        cursor = *end;
    }
    debug_assert_eq!(cursor, len, "word ranges must cover every token exactly once");
}

/// Debug-only: inclusive `chunks` must be an exact partition of `0..len`.
#[cfg(debug_assertions)]
fn debug_assert_partition(chunks: &[(usize, usize)], len: usize) {
    let mut cursor = 0usize;
    for (start, end) in chunks {
        debug_assert_eq!(*start, cursor, "chunks must be contiguous");
        debug_assert!(*end >= *start, "chunks must be non-empty");
        cursor = *end + 1;
    }
    debug_assert_eq!(cursor, len, "chunks must cover every word exactly once");
}

/// Every word strips to punctuation or a bound particle — the chunk would
/// render as a bare particle row with no content to translate.
fn is_contentless_chunk(span: &SpanWords, start: usize, end: usize) -> bool {
    use super::boundary_rules::{is_line_start_bound_particle, strip_token};
    (start..=end).all(|k| {
        let core = strip_token(&span.words[k].word);
        core.is_empty() || is_line_start_bound_particle(&span.words[k].word)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tok(text: &str, start: f64, end: f64) -> WordTokenDto {
        WordTokenDto {
            start,
            end,
            word: text.to_string(),
        }
    }

    fn span_of(tokens: &[WordTokenDto]) -> SpanWords {
        let advisor = Advisor::Default(super::super::language::DefaultAdvisor);
        let mut byte_offsets = vec![0usize; tokens.len() + 1];
        let mut acc = 0usize;
        for (k, t) in tokens.iter().enumerate() {
            acc += t.word.len();
            byte_offsets[k + 1] = acc;
        }
        aggregate_span_words(tokens, &advisor, &byte_offsets)
    }

    /// Identity advisor → one token per word, exact partition.
    #[test]
    fn identity_aggregation_partitions_tokens() {
        let tokens = vec![tok("Really", 0.0, 0.3), tok("good.", 0.3, 0.7)];
        let span = span_of(&tokens);
        assert_eq!(span.words.len(), 2);
        assert_eq!(span.token_ranges, vec![(0, 1), (1, 2)]);
        assert_eq!(span.words[0].word, "Really");
        assert_eq!(span.words[1].word, "good.");
        // Word timing comes from token endpoints only.
        assert_eq!(span.words[1].start, 0.3);
        assert_eq!(span.words[1].end, 0.7);
    }

    /// zh + jieba: multi-char tokens merge into dictionary words; the token
    /// ranges stay an exact partition regardless of how jieba cuts.
    #[test]
    fn jieba_aggregation_keeps_partition() {
        use super::super::language::profile_for_lang;
        // 0.2s tokens: the fixture has to match the cadence the timing
        // assertions below assume (last token of "因为" ends at 0.4).
        let tokens: Vec<WordTokenDto> = ["因", "为", "想", "去", "，", "就", "去"]
            .iter()
            .enumerate()
            .map(|(k, s)| tok(s, k as f64 * 0.2, k as f64 * 0.2 + 0.2))
            .collect();
        let profile = profile_for_lang("zh");
        let span_text: String = tokens.iter().map(|w| w.word.as_str()).collect();
        let advisor = profile.word_boundary_advisor(&span_text);
        let mut byte_offsets = vec![0usize; tokens.len() + 1];
        let mut acc = 0usize;
        for (k, t) in tokens.iter().enumerate() {
            acc += t.word.len();
            byte_offsets[k + 1] = acc;
        }
        let span = aggregate_span_words(&tokens, &advisor, &byte_offsets);

        // Partition invariant: contiguous, non-overlapping, full coverage.
        let mut cursor = 0usize;
        for (s, e) in &span.token_ranges {
            assert_eq!(*s, cursor);
            assert!(*e > *s);
            cursor = *e;
        }
        assert_eq!(cursor, tokens.len());
        // Reconstructed text == concatenated tokens.
        let rebuilt: String = span.words.iter().map(|w| w.word.as_str()).collect();
        assert_eq!(rebuilt, span_text);
        // jieba must keep 因为 together as the first word.
        assert_eq!(span.words[0].word, "因为");
        // Multi-char words expose only token-endpoint timing.
        assert_eq!(span.words[0].start, 0.0);
        assert_eq!(span.words[0].end, 0.4);
    }

    /// ≥2s token gaps hard-split into chunks (VAD empty → gap fallback);
    /// sub-threshold gaps do not split; chunks partition the words.
    #[test]
    fn hard_pause_splits_only_over_threshold() {
        let span = SpanWords {
            words: vec![
                tok("a", 0.0, 1.0),
                tok("b", 1.5, 2.0), // gap 0.5s
                tok("c", 4.5, 5.0), // gap 2.5s → split
                tok("d", 5.1, 6.0), // gap 0.1s
                tok("e", 7.5, 8.0), // gap 1.5s → no split
            ],
            token_ranges: vec![(0, 1), (1, 2), (2, 3), (3, 4), (4, 5)],
        };
        let vad = SpeechSegmentIndex::new(Vec::new());
        let chunks = hard_pause_chunks(&span, &vad);
        // Only the 2.5s gap splits; c/d/e stay one chunk because their gaps
        // (0.1s and 1.5s) are under the threshold. The result must also be a
        // partition of every word.
        assert_eq!(chunks, vec![(0, 1), (2, 4)]);
        let mut cursor = 0usize;
        for (start, end) in &chunks {
            assert_eq!(*start, cursor);
            assert!(*end >= *start);
            cursor = *end + 1;
        }
        assert_eq!(cursor, span.words.len());
    }

    /// VAD silence ≥ threshold splits even when the raw token gap is tiny
    /// (aligner collapse). VAD segments are SPEECH segments; the silence is
    /// the gap between consecutive segments (±100ms tolerance on the cut
    /// midpoint).
    #[test]
    fn hard_pause_uses_vad_silence() {
        let span = SpanWords {
            words: vec![
                tok("a", 0.0, 1.0),
                tok("b", 4.0, 4.2), // cut midpoint 2.5s: inside the 3s silence
                tok("c", 4.3, 5.0), // cut midpoint 4.25s: outside (tol 0.1)
            ],
            token_ranges: vec![(0, 1), (1, 2), (2, 3)],
        };
        let vad = SpeechSegmentIndex::new(vec![(0.0, 1.0), (4.0, 5.0)]);
        let chunks = hard_pause_chunks(&span, &vad);
        assert_eq!(chunks, vec![(0, 0), (1, 2)]);
    }

    /// Regression (pause-cut fix): a ≥2s pause must not manufacture a cue with
    /// nothing to read. Real case: 今回 | の | シンデレラ across long pauses —
    /// the bare の cue merged back instead of being re-split by the pause.
    #[test]
    fn a_pause_cut_never_manufactures_a_contentless_cue() {
        let span = SpanWords {
            words: vec![
                tok("今回", 0.0, 0.5),
                tok("の", 6.0, 6.3),           // 5.5s pause before it
                tok("シンデレラ", 12.0, 12.8), // 5.7s pause before it
            ],
            token_ranges: vec![(0, 1), (1, 2), (2, 3)],
        };
        let vad = SpeechSegmentIndex::new(Vec::new());
        let chunks = hard_pause_chunks(&span, &vad);
        // Three pause cuts would give [(0,0),(1,1),(2,2)]; the contentless の
        // chunk merges, so no cue is left with nothing to read.
        assert_eq!(chunks, vec![(0, 1), (2, 2)]);
    }

    /// The pause-cut pass guards `n - 1`; an empty span must be inert.
    #[test]
    fn empty_span_yields_no_pause_chunks() {
        let span = SpanWords {
            words: Vec::new(),
            token_ranges: Vec::new(),
        };
        let vad = SpeechSegmentIndex::new(Vec::new());
        assert!(hard_pause_chunks(&span, &vad).is_empty());
    }
}
