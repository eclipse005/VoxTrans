use std::sync::Arc;

#[cfg(test)]
use crate::services::transcribe::WordTokenDto;
use voxtrans_core::subtitle::beautify::beautify_words_for_subtitle;

mod assembly;
mod refinement;
mod responses;
mod semantic;
mod semantic_boundaries;
mod semantic_candidates;
#[cfg(test)]
mod tests;
mod text;
mod timing;
mod types;
mod words;

use assembly::{
    build_boundaries_from_split_points, build_micro_chunks, build_sentences_from_word_spans,
};
use semantic::{
    build_split_points_with_optional_semantic_refinement, split_points_to_spans,
    translation_unit_word_limit,
};
#[cfg(test)]
use semantic::{should_refine_semantic_span, should_split_semantic_span};
#[cfg(test)]
use text::join_words;
use types::SourceSentenceStep2;
use words::{from_core_words, to_core_words};

pub use assembly::source_sentences_to_srt;
pub use types::{BoundaryDecisionKind, SentenceBoundaryRequest};

const HARD_SPLIT_GAP_MS: u64 = 2_000;
#[cfg(test)]
const DEFAULT_SUBTITLE_MAX_WORDS_PER_SEGMENT: u32 = 20;
const MAX_UNPUNCTUATED_DURATION_MS: u64 = 24_000;
const SOFT_SPLIT_GAP_MS: u64 = 350;
const MIN_SEMANTIC_SEGMENT_WORDS: usize = 5;
const MAX_LLM_SEMANTIC_CANDIDATES: usize = 16;

pub async fn build_source_sentences_from_words_with_progress(
    request: SentenceBoundaryRequest,
    on_progress: Option<Arc<dyn Fn(usize, usize) + Send + Sync>>,
) -> Result<SourceSentenceStep2, String> {
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

    let split_points =
        build_split_points_with_optional_semantic_refinement(&request, &normalized_words).await;
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

#[cfg(test)]
fn build_deterministic_sentence_spans(words: &[WordTokenDto]) -> Vec<(usize, usize)> {
    let split_points = build_deterministic_split_points(
        words,
        translation_unit_word_limit(DEFAULT_SUBTITLE_MAX_WORDS_PER_SEGMENT),
    );
    split_points_to_spans(words.len(), &split_points)
}

#[cfg(test)]
fn build_deterministic_split_points(
    words: &[WordTokenDto],
    length_fallback_word_limit: usize,
) -> Vec<(usize, types::SplitReason)> {
    semantic::build_deterministic_split_points(words, length_fallback_word_limit)
}
