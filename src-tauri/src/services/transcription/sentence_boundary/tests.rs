use super::text::ends_with_terminal_punctuation;
use super::{
    BoundaryDecisionKind, DEFAULT_SUBTITLE_MAX_WORDS_PER_SEGMENT, HARD_SPLIT_GAP_MS,
    build_deterministic_sentence_spans, build_micro_chunks,
    build_source_sentences_from_words_with_progress,
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
fn length_fallback_still_splits_overlong_terminal_sentence() {
    let words = "All right, in this video, we're going to be talking about daily review habits and how they affect your focus and your planning mindset."
        .split_whitespace()
        .enumerate()
        .map(|(index, token)| w(index, token))
        .collect::<Vec<_>>();

    let spans = build_deterministic_sentence_spans(&words);

    assert!(
        spans.len() > 1,
        "overlong terminal sentence should still be shortened before translation"
    );
    assert!(spans.iter().all(|(start, end)| end - start < 20));
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

    assert_eq!(spans, vec![(0, 19), (20, 29), (30, 44)]);
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

    assert_eq!(spans, vec![(0, 14), (15, 29)]);
}

#[test]
fn deterministic_spans_split_long_unpunctuated_runs_without_llm() {
    let words = (0..45)
        .map(|index| w(index, &format!("w{index}")))
        .collect::<Vec<_>>();

    let spans = build_deterministic_sentence_spans(&words);

    assert!(spans.len() > 1, "long unpunctuated ASR run should be split");
    assert_eq!(spans.first(), Some(&(0, 19)));
    assert_eq!(spans.last(), Some(&(30, 44)));
}

#[test]
fn long_missing_punctuation_span_is_split_before_step4_translation() {
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

    assert!(
        texts.len() >= 3,
        "long ASR span should be refined before translation"
    );
    assert!(
        texts
            .iter()
            .all(|text| text.split_whitespace().count() <= 25),
        "step4 should not receive very long translation units: {texts:?}"
    );
    assert!(
        !texts
            .first()
            .map(|text| text.contains("hesitation"))
            .unwrap_or(false),
        "first translation unit should not absorb the next idea: {texts:?}"
    );
    assert!(
        !texts
            .first()
            .map(|text| text.ends_with(','))
            .unwrap_or(false),
        "first translation unit should not be a comma-hanging half sentence: {texts:?}"
    );
}

#[test]
fn long_punctuation_sparse_span_uses_llm_refinement_when_available() {
    let text = "This long sentence has no useful internal punctuation it keeps running through several separate ideas the recognizer only produced a final period";
    let words = text
        .split_whitespace()
        .enumerate()
        .map(|(index, token)| w(index, token))
        .collect::<Vec<_>>();
    let word_limit = super::translation_unit_word_limit(DEFAULT_SUBTITLE_MAX_WORDS_PER_SEGMENT);

    assert!(super::should_split_semantic_span(
        &words,
        0,
        words.len() - 1,
        word_limit
    ));
    assert!(super::should_refine_semantic_span(
        &words,
        0,
        words.len() - 1,
        word_limit
    ));
}

#[test]
fn long_punctuation_rich_span_stays_on_local_boundaries() {
    let text = "First we check the outline, then we confirm the references, because timing still matters, and finally we wait for a clean draft before sending feedback.";
    let words = text
        .split_whitespace()
        .enumerate()
        .map(|(index, token)| w(index, token))
        .collect::<Vec<_>>();
    let word_limit = super::translation_unit_word_limit(DEFAULT_SUBTITLE_MAX_WORDS_PER_SEGMENT);

    assert!(super::should_split_semantic_span(
        &words,
        0,
        words.len() - 1,
        word_limit
    ));
    assert!(!super::should_refine_semantic_span(
        &words,
        0,
        words.len() - 1,
        word_limit
    ));
}

#[test]
fn short_span_skips_semantic_splitting_and_refinement() {
    let text = "This short sentence is already fine.";
    let words = text
        .split_whitespace()
        .enumerate()
        .map(|(index, token)| w(index, token))
        .collect::<Vec<_>>();
    let word_limit = super::translation_unit_word_limit(DEFAULT_SUBTITLE_MAX_WORDS_PER_SEGMENT);

    assert!(!super::should_split_semantic_span(
        &words,
        0,
        words.len() - 1,
        word_limit
    ));
    assert!(!super::should_refine_semantic_span(
        &words,
        0,
        words.len() - 1,
        word_limit
    ));
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

    let response = tauri::async_runtime::block_on(build_source_sentences_from_words_with_progress(
        request(words),
        None,
    ))
    .expect("step2 should build sentence");

    assert_eq!(response.translation_sentences[0].text, "Alright, welcome.");
}
