use std::sync::Arc;

pub use super::transcription_types::{
    BoundaryDecisionCommandDto, BuildSourceSentencesCommandRequest,
    BuildSourceSentencesCommandResponse, GroupedSentenceSegmentCommandDto,
    GroupedSentenceTokenCommandDto, MicroChunkCommandDto, SourceSentenceCommandDto,
    WordTokenCommandDto,
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
            subtitle_length_preset: request.subtitle_length_preset,
            use_subtitle_layout_split: request.use_subtitle_layout_split,
            words: request.words.into_iter().map(to_service_word).collect(),
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

fn build_grouped_sentence_segments(
    words: &[WordTokenCommandDto],
    sentences: &[SourceSentenceCommandDto],
) -> Vec<GroupedSentenceSegmentCommandDto> {
    let mut out = Vec::<GroupedSentenceSegmentCommandDto>::new();
    if words.is_empty() {
        return out;
    }

    for sentence in sentences {
        if sentence.word_start >= words.len() {
            continue;
        }
        let end = sentence.word_end.min(words.len() - 1);
        if end < sentence.word_start {
            continue;
        }
        let sentence_words = &words[sentence.word_start..=end];
        let start = sentence_words
            .first()
            .map(|token| token.start)
            .unwrap_or(0.0);
        let end = sentence_words
            .last()
            .map(|token| token.end)
            .unwrap_or(start);
        let tokens = sentence_words
            .iter()
            .map(|token| GroupedSentenceTokenCommandDto {
                text: token.word.clone(),
                start: token.start,
                end: token.end,
            })
            .collect::<Vec<_>>();
        let segment = if sentence.text.trim().is_empty() {
            fallback_segment_text_from_tokens(&tokens)
        } else {
            sentence.text.clone()
        };
        out.push(GroupedSentenceSegmentCommandDto {
            segment,
            start,
            end,
            tokens,
        });
    }

    out
}

fn fallback_segment_text_from_tokens(tokens: &[GroupedSentenceTokenCommandDto]) -> String {
    let mut out = String::new();
    let mut prev_word_like = false;

    for token in tokens {
        let piece = token.text.trim();
        if piece.is_empty() {
            continue;
        }

        let next_word_like = token_has_spacing_word(piece);
        let next_starts_with_joiner = starts_with_joiner(piece);
        let prev_ends_with_spacing_punctuation = out
            .chars()
            .rev()
            .find(|ch| !ch.is_whitespace())
            .map(is_spacing_punctuation)
            .unwrap_or(false);

        if !out.is_empty()
            && ((prev_word_like && next_word_like && !next_starts_with_joiner)
                || (prev_ends_with_spacing_punctuation && next_word_like))
        {
            out.push(' ');
        }

        out.push_str(piece);
        prev_word_like = next_word_like;
    }

    out
}

fn token_has_spacing_word(token: &str) -> bool {
    token
        .chars()
        .any(|ch| ch.is_ascii_alphanumeric() || is_hangul(ch))
}

fn starts_with_joiner(token: &str) -> bool {
    token
        .chars()
        .next()
        .map(|ch| matches!(ch, '\'' | '’'))
        .unwrap_or(false)
}

fn is_spacing_punctuation(ch: char) -> bool {
    matches!(
        ch,
        ',' | '.' | '!' | '?' | ':' | ';' | '，' | '。' | '！' | '？' | '：' | '；'
    )
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

#[cfg(test)]
mod tests {
    use super::{
        SourceSentenceCommandDto, WordTokenCommandDto, build_grouped_sentence_segments,
    };

    #[test]
    fn build_grouped_sentence_segments_maps_tokens_by_sentence_span() {
        let words = vec![
            WordTokenCommandDto {
                start: 0.0,
                end: 0.2,
                word: "Hello".to_string(),
            },
            WordTokenCommandDto {
                start: 0.2,
                end: 0.4,
                word: "world".to_string(),
            },
            WordTokenCommandDto {
                start: 0.5,
                end: 0.8,
                word: "Again".to_string(),
            },
        ];
        let sentences = vec![
            SourceSentenceCommandDto {
                sentence_id: 1,
                start_ms: 0,
                end_ms: 400,
                text: "Hello world".to_string(),
                word_start: 0,
                word_end: 1,
                chunk_start: 0,
                chunk_end: 0,
            },
            SourceSentenceCommandDto {
                sentence_id: 2,
                start_ms: 500,
                end_ms: 800,
                text: "Again".to_string(),
                word_start: 2,
                word_end: 2,
                chunk_start: 1,
                chunk_end: 1,
            },
        ];

        let segments = build_grouped_sentence_segments(&words, &sentences);

        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].segment, "Hello world");
        assert_eq!(segments[0].start, 0.0);
        assert_eq!(segments[0].end, 0.4);
        assert_eq!(segments[0].tokens.len(), 2);
        assert_eq!(segments[0].tokens[0].text, "Hello");
        assert_eq!(segments[0].tokens[1].text, "world");

        assert_eq!(segments[1].segment, "Again");
        assert_eq!(segments[1].start, 0.5);
        assert_eq!(segments[1].end, 0.8);
        assert_eq!(segments[1].tokens.len(), 1);
        assert_eq!(segments[1].tokens[0].text, "Again");
    }

    #[test]
    fn build_grouped_sentence_segments_fallback_preserves_spacing_for_english_and_punctuation() {
        let words = vec![
            WordTokenCommandDto {
                start: 0.0,
                end: 0.2,
                word: "Hello".to_string(),
            },
            WordTokenCommandDto {
                start: 0.2,
                end: 0.3,
                word: ",".to_string(),
            },
            WordTokenCommandDto {
                start: 0.3,
                end: 0.5,
                word: "world".to_string(),
            },
        ];
        let sentences = vec![SourceSentenceCommandDto {
            sentence_id: 1,
            start_ms: 0,
            end_ms: 500,
            text: "   ".to_string(),
            word_start: 0,
            word_end: 2,
            chunk_start: 0,
            chunk_end: 0,
        }];

        let segments = build_grouped_sentence_segments(&words, &sentences);

        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].segment, "Hello, world");
    }

    #[test]
    fn build_grouped_sentence_segments_fallback_preserves_spacing_for_korean() {
        let words = vec![
            WordTokenCommandDto {
                start: 0.0,
                end: 0.2,
                word: "안녕하세요".to_string(),
            },
            WordTokenCommandDto {
                start: 0.2,
                end: 0.4,
                word: "여러분".to_string(),
            },
        ];
        let sentences = vec![SourceSentenceCommandDto {
            sentence_id: 1,
            start_ms: 0,
            end_ms: 400,
            text: "".to_string(),
            word_start: 0,
            word_end: 1,
            chunk_start: 0,
            chunk_end: 0,
        }];

        let segments = build_grouped_sentence_segments(&words, &sentences);

        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].segment, "안녕하세요 여러분");
    }
}
