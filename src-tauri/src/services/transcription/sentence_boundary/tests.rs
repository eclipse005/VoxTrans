use super::{
    BoundaryDecisionKind, build_deterministic_sentence_spans, build_micro_chunks,
    build_source_sentences_from_words_with_progress,
};
use crate::services::transcribe::WordTokenDto;
use voxtrans_core::subtitle::text_rules::ends_with_terminal_punctuation;

fn w(index: usize, text: &str) -> WordTokenDto {
    let start = index as f64 * 0.5;
    WordTokenDto {
        start,
        end: start + 0.3,
        word: text.to_string(),
    }
}

fn request(words: Vec<WordTokenDto>) -> super::SentenceBoundaryRequest {
    request_with_lang_and_preset(words, "en", "standard")
}

fn request_with_lang_and_preset(
    words: Vec<WordTokenDto>,
    source_lang: &str,
    subtitle_length_preset: &str,
) -> super::SentenceBoundaryRequest {
    request_with_lang_preset_and_layout(words, source_lang, subtitle_length_preset, true)
}

fn request_with_lang_preset_and_layout(
    words: Vec<WordTokenDto>,
    source_lang: &str,
    subtitle_length_preset: &str,
    use_subtitle_layout_split: bool,
) -> super::SentenceBoundaryRequest {
    request_with_vad(
        words,
        source_lang,
        subtitle_length_preset,
        use_subtitle_layout_split,
        Vec::new(),
    )
}

fn request_with_vad(
    words: Vec<WordTokenDto>,
    source_lang: &str,
    subtitle_length_preset: &str,
    use_subtitle_layout_split: bool,
    vad_speech_segments: Vec<(f64, f64)>,
) -> super::SentenceBoundaryRequest {
    super::SentenceBoundaryRequest {
        task_id: "task-1".to_string(),
        media_path: "demo.mp4".to_string(),
        source_lang: source_lang.to_string(),
        subtitle_length_preset: subtitle_length_preset.to_string(),
        use_subtitle_layout_split,
        words,
        vad_speech_segments,
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
fn overlong_terminal_sentence_stays_intact_until_terminal_punctuation() {
    let words = "All right, in this video, we're going to be talking about daily review habits and how they affect your focus and your planning mindset."
        .split_whitespace()
        .enumerate()
        .map(|(index, token)| w(index, token))
        .collect::<Vec<_>>();

    let spans = build_deterministic_sentence_spans(&words);

    assert_eq!(spans, vec![(0, words.len() - 1)]);
}

#[test]
fn soft_punctuation_does_not_create_extra_step2_split() {
    let words = (0..45)
        .map(|index| {
            let token = if index == 29 { "checkpoint," } else { "word" };
            w(index, token)
        })
        .collect::<Vec<_>>();

    let spans = build_deterministic_sentence_spans(&words);

    assert_eq!(spans, vec![(0, 44)]);
}

#[test]
fn duration_fallback_does_not_split_without_hard_pause() {
    let words = (0..30)
        .map(|index| WordTokenDto {
            start: index as f64,
            end: index as f64 + 0.2,
            word: format!("w{index}"),
        })
        .collect::<Vec<_>>();

    let spans = build_deterministic_sentence_spans(&words);

    assert_eq!(spans, vec![(0, 29)]);
}

#[test]
fn long_unpunctuated_runs_stay_intact_without_hard_pause() {
    let words = (0..45)
        .map(|index| w(index, &format!("w{index}")))
        .collect::<Vec<_>>();

    let spans = build_deterministic_sentence_spans(&words);

    assert_eq!(spans, vec![(0, 44)]);
}

#[test]
fn long_missing_punctuation_span_stays_intact_for_later_llm_layout() {
    let text = "It's something I've been trying to do every week just to get a good idea of how I'm performing against the reference list of literally reviewing every high quality example that I see because sometimes your execution slips, you might skip examples due to hesitation or maybe you choose weaker examples because you're not thinking straight.";
    let words = text
        .split_whitespace()
        .enumerate()
        .map(|(index, token)| w(index, token))
        .collect::<Vec<_>>();

    let spans = build_deterministic_sentence_spans(&words);
    let texts = spans
        .iter()
        .map(|(start, end)| {
            super::join_words(words[*start..=*end].iter().map(|word| word.word.as_str()))
        })
        .collect::<Vec<_>>();

    assert_eq!(texts, vec![text.to_string()]);
}

#[test]
fn terminal_punctuation_still_splits_long_runs() {
    let text = "This long sentence has no useful internal punctuation it keeps running through several separate ideas the recognizer only produced a final period";
    let mut words = text
        .split_whitespace()
        .enumerate()
        .map(|(index, token)| w(index, token))
        .collect::<Vec<_>>();
    words[6].word = "punctuation.".to_string();

    let spans = build_deterministic_sentence_spans(&words);

    assert_eq!(spans, vec![(0, 6), (7, words.len() - 1)]);
}

#[test]
fn broad_terminal_punctuation_splits_step2_sentences() {
    let words = vec![w(0, "你好．"), w(1, "Next⁉"), w(2, "Again")];

    let spans = build_deterministic_sentence_spans(&words);

    assert_eq!(spans, vec![(0, 0), (1, 1), (2, 2)]);
}

#[test]
fn abbreviation_terminal_punctuation_does_not_split_step2_sentence() {
    let words = vec![w(0, "Mr."), w(1, "Smith"), w(2, "arrived.")];

    let spans = build_deterministic_sentence_spans(&words);

    assert_eq!(spans, vec![(0, 2)]);
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

    // VAD detects two speech segments separated by silence [0.2, 2.4]; the
    // cut midpoint (1.3) falls inside that gap, so the hard split fires.
    let vad_segments = vec![(0.0, 0.2), (2.4, 2.7)];
    let vad_index = super::vad_align::SpeechSegmentIndex::new(vad_segments);
    let split_points = super::build_deterministic_split_points(&words, &vad_index);
    let spans = super::split_points_to_spans(words.len(), &split_points);

    assert_eq!(spans, vec![(0, 0), (1, 1)]);
}

#[test]
fn step2_builds_same_response_shape_without_llm_settings() {
    let words = vec![w(0, "Hello"), w(1, "world."), w(2, "Again.")];

    let response = tauri::async_runtime::block_on(build_source_sentences_from_words_with_progress(
        request(words),
        None,
    ))
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
    // gap_after_ms is the raw wall-clock gap between word.end (0.2s) and the
    // next word.start (2.4s) = 2200ms, independent of any threshold constant.
    assert_eq!(chunks[0].gap_after_ms, 2_200);
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

    let response = tauri::async_runtime::block_on(build_source_sentences_from_words_with_progress(
        request(words),
        None,
    ))
    .expect("step2 should build sentence");

    assert_eq!(response.translation_sentences[0].text, "Alright, welcome.");
}

#[test]
fn local_subtitle_layout_splits_long_semantic_sentence_near_punctuation() {
    let text = "Today the local transcription pipeline keeps complete semantic sentences for accurate review, but it should split long subtitle lines near punctuation for comfortable offline viewing.";
    let words = text
        .split_whitespace()
        .enumerate()
        .map(|(index, token)| w(index, token))
        .collect::<Vec<_>>();

    let response = tauri::async_runtime::block_on(build_source_sentences_from_words_with_progress(
        request_with_lang_and_preset(words, "en", "short"),
        None,
    ))
    .expect("step2 should build local subtitle layout");

    assert_eq!(response.sentence_total, 2);
    assert_eq!(
        response.translation_sentences[0].text,
        "Today the local transcription pipeline keeps complete semantic sentences for accurate review,"
    );
    assert_eq!(
        response.translation_sentences[1].text,
        "but it should split long subtitle lines near punctuation for comfortable offline viewing."
    );
    assert!(
        response
            .boundaries
            .iter()
            .any(|boundary| boundary.reason_tag == "subtitle_layout")
    );
}

#[test]
fn translation_mode_keeps_long_semantic_sentence_without_layout_split() {
    let text = "Today the translation pipeline keeps complete semantic sentences for accurate review, but it should leave subtitle readability splitting to the later LLM alignment stage.";
    let words = text
        .split_whitespace()
        .enumerate()
        .map(|(index, token)| w(index, token))
        .collect::<Vec<_>>();

    let response = tauri::async_runtime::block_on(build_source_sentences_from_words_with_progress(
        request_with_lang_preset_and_layout(words, "en", "short", false),
        None,
    ))
    .expect("step2 should build translation source sentences");

    assert_eq!(response.sentence_total, 1);
    assert_eq!(
        response.translation_sentences[0].text,
        "Today the translation pipeline keeps complete semantic sentences for accurate review, but it should leave subtitle readability splitting to the later LLM alignment stage."
    );
    assert!(
        response
            .boundaries
            .iter()
            .all(|boundary| boundary.reason_tag != "subtitle_layout")
    );
}

#[test]
fn local_subtitle_layout_never_crosses_hard_pause() {
    let words = vec![
        WordTokenDto {
            start: 0.0,
            end: 0.2,
            word: "Before".to_string(),
        },
        WordTokenDto {
            start: 0.3,
            end: 0.5,
            word: "pause".to_string(),
        },
        WordTokenDto {
            start: 2.8,
            end: 3.0,
            word: "after".to_string(),
        },
        WordTokenDto {
            start: 3.1,
            end: 3.3,
            word: "pause".to_string(),
        },
    ];

    let response = tauri::async_runtime::block_on(build_source_sentences_from_words_with_progress(
        request_with_vad(
            words,
            "en",
            "short",
            true,
            // VAD detects a silence gap [0.5, 2.8] between "pause" and "after".
            vec![(0.0, 0.5), (2.8, 3.3)],
        ),
        None,
    ))
    .expect("step2 should preserve hard pause boundary");

    assert_eq!(response.sentence_total, 2);
    assert_eq!(response.translation_sentences[0].text, "Before pause");
    assert_eq!(response.translation_sentences[1].text, "after pause");
    assert_eq!(response.boundaries[1].reason_tag, "hard_pause".to_string());
}
