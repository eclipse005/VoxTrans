use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::services::transcribe::WordTokenDto;
use voxtrans_core::subtitle::beautify::beautify_words_for_subtitle;
use voxtrans_core::subtitle::segmenter::WordToken;
use voxtrans_core::subtitle::srt::{SrtCue, to_srt_from_cues};

const HARD_SPLIT_GAP_MS: u64 = 2_000;
#[cfg(test)]
const DEFAULT_SUBTITLE_MAX_WORDS_PER_SEGMENT: u32 = 20;
const LENGTH_FALLBACK_WORD_MULTIPLIER: usize = 2;
const MAX_UNPUNCTUATED_DURATION_MS: u64 = 24_000;

#[derive(Debug, Clone)]
pub struct SentenceBoundaryRequest {
    pub task_id: String,
    pub media_path: String,
    pub source_lang: String,
    pub words: Vec<WordTokenDto>,
    pub subtitle_max_words_per_segment: u32,
    pub translate_api_key: String,
    pub translate_base_url: String,
    pub translate_model: String,
    pub llm_concurrency: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceSentenceStep2 {
    pub task_id: String,
    pub media_path: String,
    pub source_lang: String,
    pub hard_split_gap_ms: u64,
    pub micro_chunk_total: usize,
    pub boundary_total: usize,
    pub sentence_total: usize,
    pub micro_chunks: Vec<MicroChunk>,
    pub boundaries: Vec<BoundaryDecision>,
    pub translation_sentences: Vec<SourceSentence>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MicroChunk {
    pub chunk_id: usize,
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
    pub word_start: usize,
    pub word_end: usize,
    pub gap_before_ms: u64,
    pub gap_after_ms: u64,
    pub hard_split_before: bool,
    pub hard_split_after: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BoundaryDecision {
    pub left_chunk_id: usize,
    pub right_chunk_id: usize,
    pub gap_ms: u64,
    pub rule_decision: BoundaryDecisionKind,
    pub llm_decision: BoundaryDecisionKind,
    pub final_decision: BoundaryDecisionKind,
    pub confidence: f64,
    pub reason_tag: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceSentence {
    pub sentence_id: usize,
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
    pub word_start: usize,
    pub word_end: usize,
    pub chunk_start: usize,
    pub chunk_end: usize,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BoundaryDecisionKind {
    HardSplit,
    Split,
    Merge,
    Unsure,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SplitReason {
    TerminalPunctuation,
    HardPause,
    LengthFallback,
}

pub async fn build_source_sentences_from_words_with_progress(
    request: SentenceBoundaryRequest,
    on_progress: Option<Arc<dyn Fn(usize, usize) + Send + Sync>>,
) -> Result<SourceSentenceStep2, String> {
    let _compat_llm_fields = (
        &request.translate_api_key,
        &request.translate_base_url,
        &request.translate_model,
        request.llm_concurrency,
    );

    if request.words.is_empty() {
        return Err("words is empty".to_string());
    }

    let total = 4usize;
    if let Some(callback) = on_progress.as_ref() {
        callback(0, total);
    }

    let normalized_words = from_core_words(beautify_words_for_subtitle(to_core_words(
        request.words.clone(),
    )));
    if normalized_words.is_empty() {
        return Err("words is empty".to_string());
    }
    if let Some(callback) = on_progress.as_ref() {
        callback(1, total);
    }

    let micro_chunks = build_micro_chunks(&normalized_words);
    if micro_chunks.is_empty() {
        return Err("failed to build micro chunks".to_string());
    }
    if let Some(callback) = on_progress.as_ref() {
        callback(2, total);
    }

    let split_points = build_deterministic_split_points(
        &normalized_words,
        length_fallback_word_limit(request.subtitle_max_words_per_segment),
    );
    let spans = split_points_to_spans(normalized_words.len(), &split_points);
    if spans.is_empty() {
        return Err("failed to build sentence spans".to_string());
    }
    if let Some(callback) = on_progress.as_ref() {
        callback(3, total);
    }

    let translation_sentences = build_sentences_from_word_spans(&normalized_words, &spans);
    let boundaries = build_boundaries_from_split_points(&micro_chunks, &split_points);
    if let Some(callback) = on_progress.as_ref() {
        callback(4, total);
    }

    Ok(SourceSentenceStep2 {
        task_id: request.task_id,
        media_path: request.media_path,
        source_lang: request.source_lang,
        hard_split_gap_ms: HARD_SPLIT_GAP_MS,
        micro_chunk_total: micro_chunks.len(),
        boundary_total: boundaries.len(),
        sentence_total: translation_sentences.len(),
        micro_chunks,
        boundaries,
        translation_sentences,
    })
}

pub fn source_sentences_to_srt(step2: &SourceSentenceStep2) -> String {
    let cues = step2
        .translation_sentences
        .iter()
        .map(|sentence| SrtCue {
            index: sentence.sentence_id,
            start_ms: sentence.start_ms,
            end_ms: sentence.end_ms,
            text: sentence.text.clone(),
        })
        .collect::<Vec<_>>();
    to_srt_from_cues(&cues)
}

#[cfg(test)]
fn build_deterministic_sentence_spans(words: &[WordTokenDto]) -> Vec<(usize, usize)> {
    let split_points = build_deterministic_split_points(
        words,
        length_fallback_word_limit(DEFAULT_SUBTITLE_MAX_WORDS_PER_SEGMENT),
    );
    split_points_to_spans(words.len(), &split_points)
}

fn build_deterministic_split_points(
    words: &[WordTokenDto],
    length_fallback_word_limit: usize,
) -> Vec<(usize, SplitReason)> {
    if words.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::<(usize, SplitReason)>::new();
    let mut sentence_start = 0usize;
    for index in 0..words.len() {
        let high_priority_reason = if ends_with_terminal_punctuation(&words[index].word) {
            Some(SplitReason::TerminalPunctuation)
        } else if index + 1 < words.len()
            && gap_ms(words[index].end, words[index + 1].start) >= HARD_SPLIT_GAP_MS
        {
            Some(SplitReason::HardPause)
        } else {
            None
        };

        if let Some(reason) = high_priority_reason {
            push_length_fallback_splits(
                &mut out,
                words,
                sentence_start,
                index,
                length_fallback_word_limit,
            );
            push_split_point(&mut out, index, reason);
            sentence_start = index + 1;
        }
    }
    if sentence_start < words.len() {
        push_length_fallback_splits(
            &mut out,
            words,
            sentence_start,
            words.len() - 1,
            length_fallback_word_limit,
        );
    }

    out
}

fn push_length_fallback_splits(
    split_points: &mut Vec<(usize, SplitReason)>,
    words: &[WordTokenDto],
    start: usize,
    end: usize,
    length_fallback_word_limit: usize,
) {
    if words.is_empty() || start >= words.len() || start > end {
        return;
    }

    let end = end.min(words.len() - 1);
    let limit = length_fallback_word_limit.max(1);
    let mut cursor = start;
    while cursor <= end {
        let remaining_words = end.saturating_sub(cursor) + 1;
        let remaining_duration = span_duration_ms(words, cursor, end);
        if remaining_words <= limit && remaining_duration <= MAX_UNPUNCTUATED_DURATION_MS {
            break;
        }

        let split_end = if remaining_words <= limit {
            choose_length_fallback_split(
                words,
                cursor,
                end,
                (limit / LENGTH_FALLBACK_WORD_MULTIPLIER).max(1),
            )
        } else {
            choose_length_fallback_split(words, cursor, end, limit)
        };
        if split_end < cursor || split_end >= end {
            break;
        }

        push_split_point(split_points, split_end, SplitReason::LengthFallback);
        cursor = split_end + 1;
    }
}

fn choose_length_fallback_split(
    words: &[WordTokenDto],
    start: usize,
    end: usize,
    length_fallback_word_limit: usize,
) -> usize {
    let limit_end = start
        .saturating_add(length_fallback_word_limit)
        .saturating_sub(1)
        .min(end);
    find_soft_punctuation_split(words, start, limit_end, length_fallback_word_limit)
        .unwrap_or(limit_end)
}

fn find_soft_punctuation_split(
    words: &[WordTokenDto],
    start: usize,
    end: usize,
    length_fallback_word_limit: usize,
) -> Option<usize> {
    if start >= end {
        return None;
    }

    let minimum_end = start
        .saturating_add((length_fallback_word_limit / 2).max(1))
        .min(end);
    (minimum_end..=end)
        .rev()
        .find(|index| ends_with_soft_punctuation(&words[*index].word))
}

fn length_fallback_word_limit(subtitle_max_words_per_segment: u32) -> usize {
    subtitle_max_words_per_segment.clamp(8, 40) as usize * LENGTH_FALLBACK_WORD_MULTIPLIER
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

fn split_points_to_spans(
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

fn build_micro_chunks(words: &[WordTokenDto]) -> Vec<MicroChunk> {
    words
        .iter()
        .enumerate()
        .map(|(index, word)| {
            let gap_before_ms = index
                .checked_sub(1)
                .and_then(|prev| words.get(prev))
                .map(|prev| gap_ms(prev.end, word.start))
                .unwrap_or(0);
            let gap_after_ms = words
                .get(index + 1)
                .map(|next| gap_ms(word.end, next.start))
                .unwrap_or(0);
            MicroChunk {
                chunk_id: index + 1,
                start_ms: seconds_to_ms(word.start),
                end_ms: seconds_to_ms(word.end.max(word.start)),
                text: word.word.clone(),
                word_start: index,
                word_end: index,
                gap_before_ms,
                gap_after_ms,
                hard_split_before: gap_before_ms >= HARD_SPLIT_GAP_MS,
                hard_split_after: gap_after_ms >= HARD_SPLIT_GAP_MS,
            }
        })
        .collect()
}

fn build_sentences_from_word_spans(
    words: &[WordTokenDto],
    spans: &[(usize, usize)],
) -> Vec<SourceSentence> {
    spans
        .iter()
        .filter_map(|(start, end)| {
            if *start >= words.len() || *end >= words.len() || start > end {
                return None;
            }
            Some((*start, *end))
        })
        .enumerate()
        .map(|(index, (start, end))| SourceSentence {
            sentence_id: index + 1,
            start_ms: seconds_to_ms(words[start].start),
            end_ms: seconds_to_ms(words[end].end.max(words[start].start)),
            text: join_words(words[start..=end].iter().map(|word| word.word.as_str())),
            word_start: start,
            word_end: end,
            chunk_start: start + 1,
            chunk_end: end + 1,
        })
        .collect()
}

fn build_boundaries_from_split_points(
    micro_chunks: &[MicroChunk],
    split_points: &[(usize, SplitReason)],
) -> Vec<BoundaryDecision> {
    if micro_chunks.len() < 2 {
        return Vec::new();
    }

    let mut split_by_end = std::collections::HashMap::<usize, SplitReason>::new();
    for (end, reason) in split_points.iter().copied() {
        split_by_end.insert(end, reason);
    }

    (0..micro_chunks.len() - 1)
        .map(|index| {
            let left = &micro_chunks[index];
            let right = &micro_chunks[index + 1];
            let split_reason = split_by_end.get(&index).copied();
            let (rule_decision, confidence, reason_tag) = match split_reason {
                Some(SplitReason::TerminalPunctuation) => {
                    (BoundaryDecisionKind::Split, 1.0, "terminal_punctuation")
                }
                Some(SplitReason::HardPause) => {
                    (BoundaryDecisionKind::HardSplit, 1.0, "hard_pause")
                }
                Some(SplitReason::LengthFallback) => {
                    (BoundaryDecisionKind::Split, 0.82, "length_fallback")
                }
                None => (BoundaryDecisionKind::Merge, 0.95, "merge"),
            };
            BoundaryDecision {
                left_chunk_id: left.chunk_id,
                right_chunk_id: right.chunk_id,
                gap_ms: gap_ms(
                    (left.end_ms as f64) / 1000.0,
                    (right.start_ms as f64) / 1000.0,
                ),
                rule_decision,
                llm_decision: BoundaryDecisionKind::Unknown,
                final_decision: rule_decision,
                confidence,
                reason_tag: reason_tag.to_string(),
            }
        })
        .collect()
}

fn span_duration_ms(words: &[WordTokenDto], start: usize, end: usize) -> u64 {
    if start >= words.len() || end >= words.len() || start > end {
        return 0;
    }
    ((words[end].end - words[start].start).max(0.0) * 1000.0).round() as u64
}

fn ends_with_terminal_punctuation(word: &str) -> bool {
    word.trim_end()
        .chars()
        .last()
        .map(|ch| matches!(ch, '.' | '!' | '?' | '。' | '！' | '？'))
        .unwrap_or(false)
}

fn ends_with_soft_punctuation(word: &str) -> bool {
    word.trim_end()
        .chars()
        .last()
        .map(|ch| matches!(ch, ',' | ';' | ':' | '，' | '；' | '：' | '、'))
        .unwrap_or(false)
}

fn join_words<'a>(parts: impl Iterator<Item = &'a str>) -> String {
    let mut out = String::new();
    let mut prev_has_spacing_word = false;
    let mut prev_allows_space_after = false;

    for raw in parts {
        let token = raw.trim();
        if token.is_empty() {
            continue;
        }
        let next_has_spacing_word = token_has_spacing_word(token);
        if !out.is_empty()
            && next_has_spacing_word
            && (prev_has_spacing_word || prev_allows_space_after)
        {
            out.push(' ');
        }
        out.push_str(token);
        prev_has_spacing_word = next_has_spacing_word;
        prev_allows_space_after = token_allows_space_after(token);
    }

    out.replace(" ,", ",")
        .replace(" .", ".")
        .replace(" !", "!")
        .replace(" ?", "?")
        .replace(" :", ":")
        .replace(" ;", ";")
}

fn token_allows_space_after(token: &str) -> bool {
    token
        .chars()
        .last()
        .map(|ch| matches!(ch, ',' | ';' | ':' | '?' | '!' | '.' | '，' | '；' | '：' | '？' | '！' | '。'))
        .unwrap_or(false)
}

fn token_has_spacing_word(token: &str) -> bool {
    token
        .chars()
        .any(|ch| ch.is_ascii_alphanumeric() || is_hangul(ch))
}

fn is_hangul(ch: char) -> bool {
    matches!(
        ch as u32,
        0x1100..=0x11FF
            | 0x3130..=0x318F
            | 0xA960..=0xA97F
            | 0xAC00..=0xD7AF
            | 0xD7B0..=0xD7FF
    )
}

fn gap_ms(left_end_sec: f64, right_start_sec: f64) -> u64 {
    ((right_start_sec - left_end_sec).max(0.0) * 1000.0).round() as u64
}

fn seconds_to_ms(value: f64) -> u64 {
    (value.max(0.0) * 1000.0).round() as u64
}

fn to_core_words(words: Vec<WordTokenDto>) -> Vec<WordToken> {
    words
        .into_iter()
        .map(|word| WordToken {
            start: word.start,
            end: word.end,
            word: word.word,
        })
        .collect()
}

fn from_core_words(words: Vec<WordToken>) -> Vec<WordTokenDto> {
    words
        .into_iter()
        .map(|word| WordTokenDto {
            start: word.start,
            end: word.end,
            word: word.word,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        BoundaryDecisionKind, DEFAULT_SUBTITLE_MAX_WORDS_PER_SEGMENT, HARD_SPLIT_GAP_MS,
        build_deterministic_sentence_spans, build_micro_chunks,
        build_source_sentences_from_words_with_progress, ends_with_terminal_punctuation,
    };
    use crate::services::transcribe::WordTokenDto;

    fn w(index: usize, text: &str) -> WordTokenDto {
        let start = index as f64 * 0.5;
        WordTokenDto {
            start,
            end: start + 0.3,
            word: text.to_string(),
        }
    }

    fn request(words: Vec<WordTokenDto>) -> super::SentenceBoundaryRequest {
        super::SentenceBoundaryRequest {
            task_id: "task-1".to_string(),
            media_path: "demo.mp4".to_string(),
            source_lang: "en".to_string(),
            words,
            subtitle_max_words_per_segment: DEFAULT_SUBTITLE_MAX_WORDS_PER_SEGMENT,
            translate_api_key: String::new(),
            translate_base_url: String::new(),
            translate_model: String::new(),
            llm_concurrency: 16,
        }
    }

    #[test]
    fn deterministic_spans_split_on_terminal_punctuation() {
        let words = vec![
            w(0, "Hello"),
            w(1, "world."),
            w(2, "Next"),
            w(3, "sentence?"),
        ];

        let spans = build_deterministic_sentence_spans(&words);

        assert_eq!(spans, vec![(0, 1), (2, 3)]);
    }

    #[test]
    fn terminal_punctuation_beats_length_fallback_within_reasonable_limit() {
        let words = "All right, in this video, we're going to be talking about base hits and how it affects your discipline and your mindset with trading."
            .split_whitespace()
            .enumerate()
            .map(|(index, token)| w(index, token))
            .collect::<Vec<_>>();

        let spans = build_deterministic_sentence_spans(&words);

        assert_eq!(spans, vec![(0, words.len() - 1)]);
    }

    #[test]
    fn length_fallback_prefers_soft_punctuation_for_very_long_runs() {
        let words = (0..45)
            .map(|index| {
                let token = if index == 29 { "checkpoint," } else { "word" };
                w(index, token)
            })
            .collect::<Vec<_>>();

        let spans = build_deterministic_sentence_spans(&words);

        assert_eq!(spans, vec![(0, 29), (30, 44)]);
    }

    #[test]
    fn duration_fallback_splits_slow_unpunctuated_runs_under_word_limit() {
        let words = (0..30)
            .map(|index| WordTokenDto {
                start: index as f64,
                end: index as f64 + 0.2,
                word: format!("w{index}"),
            })
            .collect::<Vec<_>>();

        let spans = build_deterministic_sentence_spans(&words);

        assert_eq!(spans, vec![(0, 19), (20, 29)]);
    }

    #[test]
    fn deterministic_spans_split_long_unpunctuated_runs_without_llm() {
        let words = (0..45)
            .map(|index| w(index, &format!("w{index}")))
            .collect::<Vec<_>>();

        let spans = build_deterministic_sentence_spans(&words);

        assert!(spans.len() > 1, "long unpunctuated ASR run should be split");
        assert_eq!(spans.first(), Some(&(0, 39)));
        assert_eq!(spans.last(), Some(&(40, 44)));
    }

    #[test]
    fn short_unpunctuated_fragment_merges_into_next_punctuated_sentence() {
        let words = vec![w(0, "well"), w(1, "let's"), w(2, "start.")];

        let spans = build_deterministic_sentence_spans(&words);

        assert_eq!(spans, vec![(0, 2)]);
    }

    #[test]
    fn hard_pause_splits_even_without_punctuation() {
        let words = vec![
            WordTokenDto {
                start: 0.0,
                end: 0.2,
                word: "Okay".to_string(),
            },
            WordTokenDto {
                start: 2.4,
                end: 2.7,
                word: "next".to_string(),
            },
        ];

        let spans = build_deterministic_sentence_spans(&words);

        assert_eq!(spans, vec![(0, 0), (1, 1)]);
    }

    #[test]
    fn step2_builds_same_response_shape_without_llm_settings() {
        let words = vec![w(0, "Hello"), w(1, "world."), w(2, "Again.")];

        let response = tauri::async_runtime::block_on(
            build_source_sentences_from_words_with_progress(request(words), None),
        )
        .expect("step2 should not require llm settings");

        assert_eq!(response.sentence_total, 2);
        assert_eq!(response.translation_sentences[0].text, "Hello world.");
        assert_eq!(response.translation_sentences[1].text, "Again.");
        assert_eq!(response.boundary_total, 2);
        assert_eq!(
            response.boundaries[1].final_decision,
            BoundaryDecisionKind::Split
        );
        assert_eq!(
            response.boundaries[1].reason_tag,
            "terminal_punctuation".to_string()
        );
    }

    #[test]
    fn hard_pause_forces_micro_chunk_boundary() {
        let words = vec![
            WordTokenDto {
                start: 0.0,
                end: 0.2,
                word: "Hello".to_string(),
            },
            WordTokenDto {
                start: 2.4,
                end: 2.7,
                word: "world".to_string(),
            },
        ];

        let chunks = build_micro_chunks(&words);
        assert_eq!(chunks.len(), 2);
        assert!(chunks[0].hard_split_after);
        assert_eq!(chunks[0].gap_after_ms, HARD_SPLIT_GAP_MS + 200);
    }

    #[test]
    fn punctuation_still_closes_atom_when_available() {
        assert!(ends_with_terminal_punctuation("you."));
        assert!(ends_with_terminal_punctuation("真的吗？"));
        assert!(!ends_with_terminal_punctuation("because"));
    }

    #[test]
    fn standalone_ascii_punctuation_keeps_following_space() {
        let words = vec![w(0, "Alright"), w(1, ","), w(2, "welcome.")];

        let response = tauri::async_runtime::block_on(
            build_source_sentences_from_words_with_progress(request(words), None),
        )
        .expect("step2 should build sentence");

        assert_eq!(response.translation_sentences[0].text, "Alright, welcome.");
    }
}
