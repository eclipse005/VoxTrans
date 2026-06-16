//! Language-aware word-boundary advice for the DP cost function.
//!
//! The segmentation pipeline operates on ASR-produced tokens. For Latin and
//! Japanese, token boundaries already correspond to word boundaries (ASR
//! emits word-level tokens). For Chinese (`zh`/`yue`), ASR emits **character-
//! level** tokens, so every character gap looks identical to the DP cost
//! function — it cannot tell `电影|制片厂` (word boundary, good cut) from
//! `电|影制片厂` (inside a word, bad cut).
//!
//! This module provides a [`WordBoundaryAdvisor`] trait that the cost function
//! queries at each candidate cut point. The Chinese implementation uses jieba
//! to determine whether a character gap falls inside a word, so the DP can
//! penalize intra-word cuts and prefer word-boundary cuts.
//!
//! **Token data is never modified** — jieba only informs the cost, it does
//! not merge or split tokens. Token ↔ timestamp alignment stays sacred.

use std::collections::HashSet;
use std::sync::OnceLock;

use jieba_rs::{Jieba, TokenizeMode};

/// Default advisor for languages where ASR token boundaries align with word
/// boundaries (English, Japanese, Korean, etc.). Returns `None` everywhere —
/// no extra cost penalty.
struct DefaultAdvisor;

impl DefaultAdvisor {
    #[inline]
    fn is_word_boundary(&self, _: usize) -> Option<bool> {
        None
    }
}

/// Chinese advisor using jieba word segmentation.
///
/// Constructed from the concatenated text of a semantic span. jieba tokenizes
/// it once; we record the byte offsets where words end (= word boundaries).
/// A gap at a word-boundary offset returns `Some(true)`; a gap inside a word
/// returns `Some(false)`.
struct JiebaAdvisor {
    /// Byte offsets (into the span text) that are word boundaries.
    boundaries: HashSet<usize>,
}

impl JiebaAdvisor {
    fn new(span_text: &str) -> Self {
        static JIEBA: OnceLock<Jieba> = OnceLock::new();
        let jieba = JIEBA.get_or_init(Jieba::new);
        let tokens = jieba.tokenize(span_text, TokenizeMode::Default, true);
        let boundaries: HashSet<usize> = tokens.iter().map(|t| t.end).collect();
        Self { boundaries }
    }

    fn is_word_boundary(&self, byte_offset: usize) -> Option<bool> {
        if self.boundaries.contains(&byte_offset) {
            Some(true)
        } else {
            // Not a jieba word boundary → it's inside a word.
            Some(false)
        }
    }
}

/// Boxed advisor enum. Constructed once per span, queried O(1) per candidate
/// cut. Enum dispatch avoids vtable overhead in the DP hot loop.
pub(super) enum Advisor {
    #[allow(dead_code)]
    Default(DefaultAdvisor),
    Jieba(JiebaAdvisor),
}

impl Advisor {
    #[inline]
    pub(super) fn is_word_boundary(&self, byte_offset: usize) -> Option<bool> {
        match self {
            Advisor::Default(a) => a.is_word_boundary(byte_offset),
            Advisor::Jieba(a) => a.is_word_boundary(byte_offset),
        }
    }
}

/// Construct the appropriate advisor for a language.
///
/// `span_text` is the concatenated text of the span being DP-split (needed
/// for jieba tokenization). For non-CJK languages the text is unused.
pub(super) fn advisor_for_lang(source_lang: &str, span_text: &str) -> Advisor {
    let lang = source_lang.trim().to_ascii_lowercase();
    if lang.starts_with("zh") || lang.starts_with("yue") {
        Advisor::Jieba(JiebaAdvisor::new(span_text))
    } else {
        Advisor::Default(DefaultAdvisor)
    }
}
