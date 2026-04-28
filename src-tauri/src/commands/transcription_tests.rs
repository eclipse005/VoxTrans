use super::transcription::{SourceSentenceCommandDto, WordTokenCommandDto};
use super::transcription_grouping::build_grouped_sentence_segments;

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
