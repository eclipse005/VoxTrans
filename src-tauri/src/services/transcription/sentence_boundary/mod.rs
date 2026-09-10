use std::sync::Arc;

#[cfg(test)]
use crate::services::transcribe::WordTokenDto;
use voxtrans_core::subtitle::beautify::beautify_words_for_subtitle;

mod assembly;
mod boundary_rules;
mod digit_glue;
mod language;
mod punkt_map;
mod semantic;
mod subtitle_layout;
#[cfg(test)]
mod tests;
mod text;
mod timing;
mod types;
mod vad_align;
mod word_units;
mod words;

use assembly::{
    build_boundaries_from_split_points, build_micro_chunks, build_sentences_from_word_spans,
};
use semantic::{
    build_split_points_from_hard_boundaries, merge_leading_bound_particle_spans,
    split_points_to_spans,
};
use subtitle_layout::build_subtitle_layout_split_points;
use types::SourceSentenceStep2;
use words::{from_core_words, to_core_words};

pub use assembly::source_sentences_to_srt;
pub use types::{BoundaryDecisionKind, SentenceBoundaryRequest};
/// Language-aware word joining (spacing rules) shared with the translation
/// service, which rebuilds a semantic sentence's source text from its display
/// cue fragments.
pub(crate) use text::join_words;

pub async fn build_source_sentences_from_words_with_progress(
    request: SentenceBoundaryRequest,
    _on_progress: Option<Arc<dyn Fn(usize, usize) + Send + Sync>>,
) -> Result<SourceSentenceStep2, String> {
    if request.words.is_empty() {
        return Err("words is empty".to_string());
    }

    let normalized_words = digit_glue::unglue_fused_ja_copula(digit_glue::glue_asr_split_digits(
        from_core_words(beautify_words_for_subtitle(to_core_words(request.words.clone()))),
    ));
    if normalized_words.is_empty() {
        return Err("words is empty".to_string());
    }

    let vad_index = vad_align::SpeechSegmentIndex::new(request.vad_speech_segments.clone());
    let profile = language::profile_for_lang(&request.source_lang);
    let preset =
        crate::services::subtitle_length::subtitle_length_preset_from_id(
            &request.subtitle_length_preset,
        );

    let micro_chunks = build_micro_chunks(&normalized_words, &vad_index);
    if micro_chunks.is_empty() {
        return Err("failed to build micro chunks".to_string());
    }

    let hard_split_points =
        build_split_points_from_hard_boundaries(&normalized_words, &*profile);
    let semantic_spans = split_points_to_spans(normalized_words.len(), &hard_split_points);
    // Kinsoku at the semantic layer: no sentence may START with a bound
    // particle (see merge_leading_bound_particle_spans).
    let semantic_spans = merge_leading_bound_particle_spans(&normalized_words, &semantic_spans);
    let split_points = merge_split_points(
        hard_split_points,
        build_subtitle_layout_split_points(
            &normalized_words,
            &semantic_spans,
            &*profile,
            preset,
            &vad_index,
        ),
    );
    // Word atomicity, enforced once over the merged cut list: every boundary
    // falls *between* words. The layout's DP already cuts between words, but
    // punctuation and pause boundaries are token-index based and may land
    // inside one — which is how 「はい」 (tokenised は + いじゃあ) can be torn
    // apart. Word units come from the same aggregation the layout uses.
    let split_points = align_cuts_to_words(&normalized_words, &*profile, &semantic_spans, split_points);
    let spans = split_points_to_spans(normalized_words.len(), &split_points);
    if spans.is_empty() {
        return Err("failed to build sentence spans".to_string());
    }

    let translation_sentences =
        build_sentences_from_word_spans(&normalized_words, &spans, &semantic_spans);
    let boundaries = build_boundaries_from_split_points(&micro_chunks, &split_points);

    Ok(SourceSentenceStep2 {
        task_id: request.task_id,
        media_path: request.media_path,
        source_lang: request.source_lang,
        micro_chunk_total: micro_chunks.len(),
        boundary_total: boundaries.len(),
        sentence_total: translation_sentences.len(),
        micro_chunks,
        boundaries,
        translation_sentences,
        words: normalized_words,
    })
}

fn merge_split_points(
    mut base: Vec<(usize, types::SplitReason)>,
    extra: Vec<(usize, types::SplitReason)>,
) -> Vec<(usize, types::SplitReason)> {
    base.extend(extra);
    base.sort_by_key(|(index, reason)| (*index, split_reason_priority(*reason)));
    base.dedup_by_key(|(index, _)| *index);
    base
}

/// Word atomicity over the merged cut list.
///
/// A cut may only fall after a token that ends a word, where "word" is the
/// same advisor-aligned aggregation the layout lays out. A cut that would fall
/// inside one is dropped, which merges the fragments back into that word —
/// rather than pulling a particle across, which is what a content-only rule
/// does and why it must not run without this one.
fn align_cuts_to_words(
    words: &[crate::services::transcribe::WordTokenDto],
    profile: &dyn language::LanguageProfile,
    semantic_spans: &[(usize, usize)],
    points: Vec<(usize, types::SplitReason)>,
) -> Vec<(usize, types::SplitReason)> {
    use word_units::aggregate_span_words;

    let mut word_final = vec![false; words.len()];
    for &(span_start, span_end) in semantic_spans {
        if span_start >= words.len() || span_end >= words.len() || span_start >= span_end {
            continue;
        }
        let span_text: String = words[span_start..=span_end]
            .iter()
            .map(|word| word.word.as_str())
            .collect();
        let advisor = profile.word_boundary_advisor(&span_text);
        let span_len = span_end - span_start + 1;
        let mut byte_offsets = vec![0usize; span_len + 1];
        let mut acc = 0usize;
        for k in 0..span_len {
            acc += words[span_start + k].word.len();
            byte_offsets[k + 1] = acc;
        }
        let span = aggregate_span_words(&words[span_start..=span_end], &advisor, &byte_offsets);
        for &(_, token_end) in &span.token_ranges {
            // `token_end` is exclusive and relative to the span.
            let absolute = span_start + token_end - 1;
            if absolute < word_final.len() {
                word_final[absolute] = true;
            }
        }
    }

    let mut kept: Vec<(usize, types::SplitReason)> = Vec::with_capacity(points.len());
    for (index, reason) in points {
        let inside_word = index >= word_final.len() || !word_final[index];
        // The leading boundary has nothing before it to merge with; leaving it
        // alone keeps the first cue intact.
        if inside_word && !kept.is_empty() {
            continue;
        }
        kept.push((index, reason));
    }

    // Content: a cue whose words are all punctuation or bound particles has
    // nothing to read. Which way it merges is not ours to guess — the engine's
    // binding predicates know whether the fragment opens the next word
    // (ASR splits はい as は + いじゃあ: merge forward) or closes the previous
    // clause (ます: merge backward). Deciding this on token text alone is what
    // tore はい apart.
    use self::boundary_rules::{is_japanese_lexical_bind, is_line_start_bound_particle, strip_token};
    let contentless = |start: usize, end: usize| {
        start <= end
            && words[start..=end].iter().all(|word| {
                let core = strip_token(&word.word);
                core.is_empty() || is_line_start_bound_particle(&word.word)
            })
    };
    let mut resolved: Vec<(usize, types::SplitReason)> = Vec::with_capacity(kept.len());
    for (index, reason) in kept {
        let cue_start = resolved
            .last()
            .map(|(previous, _)| previous + 1)
            .unwrap_or(0);
        if resolved.is_empty() || !contentless(cue_start, index) {
            resolved.push((index, reason));
            continue;
        }
        let opens_next_word = index + 1 < words.len()
            && is_japanese_lexical_bind(&words[index].word, &words[index + 1].word);
        if opens_next_word {
            // Merge forward: drop this boundary so the fragment joins the word
            // it starts.
            continue;
        }
        // Merge backward: drop the previous boundary, keep this one.
        resolved.pop();
        resolved.push((index, reason));
    }
    resolved
}

fn split_reason_priority(reason: types::SplitReason) -> u8 {
    match reason {
        // Hard pause is the strongest display signal — it must win any
        // same-index dedup against layout preferences.
        types::SplitReason::HardPause => 0,
        types::SplitReason::TerminalPunctuation => 1,
        types::SplitReason::SubtitleLayout => 2,
    }
}

#[cfg(test)]
fn build_deterministic_sentence_spans(words: &[WordTokenDto]) -> Vec<(usize, usize)> {
    let split_points = build_deterministic_split_points(words);
    split_points_to_spans(words.len(), &split_points)
}

#[cfg(test)]
fn build_deterministic_split_points(
    words: &[WordTokenDto],
) -> Vec<(usize, types::SplitReason)> {
    semantic::build_deterministic_split_points(words)
}
