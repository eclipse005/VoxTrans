use super::WordToken;
use crate::subtitle::text_rules::should_split_after_terminal_token;

pub(super) fn split_by_semantic_boundary(words: &[WordToken], max_pause_ms: f64) -> Vec<Vec<WordToken>> {
    let mut segments: Vec<Vec<WordToken>> = Vec::new();
    let mut current: Vec<WordToken> = Vec::new();

    for (idx, word) in words.iter().enumerate() {
        current.push(word.clone());

        let punctuation_break = should_split_after_word(words, idx);
        let pause_break = if idx + 1 < words.len() {
            let pause_ms = (words[idx + 1].start - word.end).max(0.0) * 1000.0;
            pause_ms >= max_pause_ms
        } else {
            false
        };

        if punctuation_break || pause_break {
            segments.push(current);
            current = Vec::new();
        }
    }

    if !current.is_empty() {
        segments.push(current);
    }

    segments
}

fn should_split_after_word(words: &[WordToken], idx: usize) -> bool {
    let current_word = match words.get(idx) {
        Some(w) => w.word.as_str(),
        None => return false,
    };
    let next_word = words.get(idx + 1).map(|w| w.word.as_str());
    should_split_after_terminal_token(current_word, next_word)
}
