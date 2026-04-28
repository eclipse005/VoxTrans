use super::transcription_grouping::build_grouped_sentence_segments;
use std::sync::Arc;

pub use super::transcription_types::{
    BoundaryDecisionCommandDto, BuildSourceSentencesCommandRequest,
    BuildSourceSentencesCommandResponse, GroupedSentenceSegmentCommandDto,
    GroupedSentenceTokenCommandDto, MicroChunkCommandDto, SourceSentenceCommandDto,
    WordTokenCommandDto,
};
pub(super) use super::transcription_types::{
    default_llm_concurrency, default_subtitle_max_words_per_segment,
};

#[tauri::command]
pub async fn build_source_sentences(
    request: BuildSourceSentencesCommandRequest,
) -> Result<BuildSourceSentencesCommandResponse, String> {
    build_source_sentences_with_progress(request, None).await
}

pub async fn build_source_sentences_with_progress(
    request: BuildSourceSentencesCommandRequest,
    on_progress: Option<Arc<dyn Fn(usize, usize) + Send + Sync>>,
) -> Result<BuildSourceSentencesCommandResponse, String> {
    let original_words = request.words.clone();
    let step2 = crate::services::transcription::build_source_sentences_from_words_with_progress(
        crate::services::transcription::SentenceBoundaryRequest {
            task_id: request.task_id,
            media_path: request.audio_path,
            source_lang: request.source_lang,
            words: request.words.into_iter().map(to_service_word).collect(),
            subtitle_max_words_per_segment: request.subtitle_max_words_per_segment,
            translate_api_key: request.translate_api_key,
            translate_base_url: request.translate_base_url,
            translate_model: request.translate_model,
            llm_concurrency: request.llm_concurrency,
        },
        on_progress,
    )
    .await?;
    let srt = crate::services::transcription::source_sentences_to_srt(&step2);
    let translation_sentences = step2
        .translation_sentences
        .into_iter()
        .map(|sentence| SourceSentenceCommandDto {
            sentence_id: sentence.sentence_id,
            start_ms: sentence.start_ms,
            end_ms: sentence.end_ms,
            text: sentence.text,
            word_start: sentence.word_start,
            word_end: sentence.word_end,
            chunk_start: sentence.chunk_start,
            chunk_end: sentence.chunk_end,
        })
        .collect::<Vec<_>>();
    let segments = build_grouped_sentence_segments(&original_words, &translation_sentences);
    Ok(BuildSourceSentencesCommandResponse {
        hard_split_gap_ms: step2.hard_split_gap_ms,
        micro_chunk_total: step2.micro_chunk_total,
        boundary_total: step2.boundary_total,
        sentence_total: step2.sentence_total,
        micro_chunks: step2
            .micro_chunks
            .iter()
            .cloned()
            .map(|chunk| MicroChunkCommandDto {
                chunk_id: chunk.chunk_id,
                start_ms: chunk.start_ms,
                end_ms: chunk.end_ms,
                text: chunk.text,
                word_start: chunk.word_start,
                word_end: chunk.word_end,
                gap_before_ms: chunk.gap_before_ms,
                gap_after_ms: chunk.gap_after_ms,
                hard_split_before: chunk.hard_split_before,
                hard_split_after: chunk.hard_split_after,
            })
            .collect(),
        boundaries: step2
            .boundaries
            .into_iter()
            .map(|boundary| BoundaryDecisionCommandDto {
                left_chunk_id: boundary.left_chunk_id,
                right_chunk_id: boundary.right_chunk_id,
                gap_ms: boundary.gap_ms,
                rule_decision: boundary.rule_decision,
                llm_decision: boundary.llm_decision,
                final_decision: boundary.final_decision,
                confidence: boundary.confidence,
                reason_tag: boundary.reason_tag,
            })
            .collect(),
        translation_sentences,
        segments,
        srt,
    })
}

fn to_service_word(word: WordTokenCommandDto) -> crate::services::transcribe::WordTokenDto {
    crate::services::transcribe::WordTokenDto {
        start: word.start,
        end: word.end,
        word: word.word,
    }
}

pub use super::transcription_cli::maybe_run_build_source_sentences_mode_from_args;
