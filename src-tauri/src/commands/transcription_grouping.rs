use super::transcription::{
    GroupedSentenceSegmentCommandDto, GroupedSentenceTokenCommandDto, SourceSentenceCommandDto,
    WordTokenCommandDto,
};

pub(super) fn build_grouped_sentence_segments(
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
