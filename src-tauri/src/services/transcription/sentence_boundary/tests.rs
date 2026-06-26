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
    request_with_lang_preset_and_layout(words, source_lang, subtitle_length_preset)
}

fn request_with_lang_preset_and_layout(
    words: Vec<WordTokenDto>,
    source_lang: &str,
    subtitle_length_preset: &str,
) -> super::SentenceBoundaryRequest {
    request_with_vad(words, source_lang, subtitle_length_preset, Vec::new())
}

fn request_with_vad(
    words: Vec<WordTokenDto>,
    source_lang: &str,
    subtitle_length_preset: &str,
    vad_speech_segments: Vec<(f64, f64)>,
) -> super::SentenceBoundaryRequest {
    super::SentenceBoundaryRequest {
        task_id: "task-1".to_string(),
        media_path: "demo.mp4".to_string(),
        source_lang: source_lang.to_string(),
        subtitle_length_preset: subtitle_length_preset.to_string(),
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
fn single_letter_enumeration_token_forces_step2_split() {
    // Reproduces the "step one B. So ..." regression: a single-letter dotted
    // token is NOT a name initial — it's a spoken enumeration end. The `.`
    // must force a split, and the following capitalized sentence becomes its
    // own span.
    let words = vec![
        w(0, "step"),
        w(1, "one"),
        w(2, "B."),
        w(3, "So"),
        w(4, "let's"),
        w(5, "go."),
    ];

    let spans = build_deterministic_sentence_spans(&words);

    assert_eq!(spans, vec![(0, 2), (3, 5)]);
}

#[test]
fn consecutive_single_letter_initials_chain_only_protects_internal_pairs() {
    // "J. K. Rowling" — the chain J.→K. is protected (both single-letter), so
    // no split between them. But K.→Rowling is NOT protected (Rowling is a
    // normal word), so K. is treated as an isolated terminal and splits.
    // This is the intended design: single-letter protection is pairwise, so
    // the *internal* bond of an initial chain holds, but a trailing isolated
    // initial still acts as a sentence end. Real ASR rarely emits initial
    // chains, so the common case ("step one B.") splits correctly.
    let words = vec![w(0, "J."), w(1, "K."), w(2, "Rowling")];

    let spans = build_deterministic_sentence_spans(&words);

    assert_eq!(spans, vec![(0, 1), (2, 2)]);
}

#[test]
fn short_unpunctuated_fragment_merges_into_next_punctuated_sentence() {
    let words = vec![w(0, "well"), w(1, "let's"), w(2, "start.")];

    let spans = build_deterministic_sentence_spans(&words);

    assert_eq!(spans, vec![(0, 2)]);
}

#[test]
fn hard_pause_does_not_split_short_sentence_without_punctuation() {
    // VAD detects a silence gap, but the sentence is short (2 words, well under
    // the length budget). After the DP rewrite, a sentence under budget is
    // NEVER split even with a VAD pause — preventing the Orderblock-style
    // mid-sentence fragmentation.
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

    // semantic.rs no longer hard-splits on VAD (only terminal punctuation).
    let split_points = super::build_deterministic_split_points(&words);
    assert!(
        split_points.is_empty(),
        "short sentence with VAD pause must not be hard-split"
    );
    let spans = super::split_points_to_spans(words.len(), &split_points);
    assert_eq!(spans, vec![(0, 1)]);
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

    // VAD detects two speech segments separated by silence [0.2, 2.4]; the
    // cut midpoint (1.3) falls inside that gap, so hard_split_after fires.
    let vad_index = super::vad_align::SpeechSegmentIndex::new(vec![(0.0, 0.2), (2.4, 2.7)]);
    let chunks = build_micro_chunks(&words, &vad_index);
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
fn short_sentence_with_vad_pause_stays_intact() {
    // 4 words with a VAD silence gap in the middle, but well under the length
    // budget (short preset = 12 words). After the DP rewrite, this is NOT
    // split — the VAD pause only matters for overlong spans.
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
            // VAD detects a silence gap [0.5, 2.8] between "pause" and "after".
            vec![(0.0, 0.5), (2.8, 3.3)],
        ),
        None,
    ))
    .expect("step2 should build one sentence");

    // Under budget → one sentence, not fragmented by the VAD pause.
    assert_eq!(response.sentence_total, 1);
    assert_eq!(
        response.translation_sentences[0].text,
        "Before pause after pause"
    );
}

/// Ablation: VAD sustains segmentation when terminal punctuation is absent,
/// but ONLY for overlong spans that exceed the length budget. This proves VAD
/// adds value (overlong unpunctuated speech gets split at silence) without
/// fragmenting short sentences.
#[test]
fn vad_sustains_segmentation_when_punctuation_stripped() {
    fn word(start: f64, end: f64, text: &str) -> WordTokenDto {
        WordTokenDto {
            start,
            end,
            word: text.to_string(),
        }
    }

    // An overlong unpunctuated span (20 words, English "short" preset limit=12).
    // A silence gap sits in the middle (between word 9 and word 10).
    let words_stripped: Vec<WordTokenDto> = (0..20)
        .map(|i| {
            let t = i as f64;
            // gap after word 9: word 9 ends at 9.0, word 10 starts at 10.5
            let (start, end) = if i < 10 {
                (t, t + 0.4)
            } else {
                (t + 1.5, t + 1.9)
            };
            word(start, end, &format!("w{i}"))
        })
        .collect();

    let vad_segments = vec![
        (0.0, 9.4),  // words 0-9
        (10.5, 21.9), // words 10-19
    ];

    // semantic.rs: no terminal punctuation → no hard split (correct).
    let en_profile = super::language::profile_for_lang("en");
    let splits_semantic =
        super::semantic::build_split_points_from_hard_boundaries(&words_stripped, &*en_profile);
    assert!(
        splits_semantic.is_empty(),
        "no punctuation → no semantic hard split"
    );

    // Build semantic spans (one span: 0..19, since no hard split).
    let semantic_spans = super::split_points_to_spans(words_stripped.len(), &splits_semantic);

    let en_profile = super::language::profile_for_lang("en");
    let short_preset =
        crate::services::subtitle_length::subtitle_length_preset_from_id("short");

    // DP with VAD: overlong span must be split, and the VAD silence gap
    // (cost 2.0) should be chosen over plain word boundaries (cost 6.0).
    let idx_vad = super::vad_align::SpeechSegmentIndex::new(vad_segments);
    let splits_vad = super::subtitle_layout::build_subtitle_layout_split_points(
        &words_stripped,
        &semantic_spans,
        &*en_profile,
        short_preset,
        &idx_vad,
    );
    assert!(
        splits_vad.iter().any(|(i, _)| *i == 9),
        "DP should split at the VAD silence (after word 9), got cuts: {:?}",
        splits_vad
    );

    // DP WITHOUT VAD: still splits (overlong), but at a plain word boundary
    // (cost 6.0) — proving VAD gives a *better* cut, not the only cut.
    let idx_empty = super::vad_align::SpeechSegmentIndex::new(Vec::new());
    let splits_no_vad = super::subtitle_layout::build_subtitle_layout_split_points(
        &words_stripped,
        &semantic_spans,
        &*en_profile,
        short_preset,
        &idx_empty,
    );
    assert!(
        !splits_no_vad.is_empty(),
        "overlong span must be split even without VAD"
    );
}
