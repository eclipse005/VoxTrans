//! Post-ASR sentence-boundary processing for step2.
mod sentence_boundary;

pub use sentence_boundary::{
    BoundaryDecisionKind, SentenceBoundaryRequest,
    build_source_sentences_from_words_with_progress, source_sentences_to_srt,
};
/// Language-aware word joining (spacing rules), used by the translation
/// service to rebuild sentence-level unit source text from cue fragments.
pub(crate) use sentence_boundary::join_words;
