use super::super::srt::{SegmentWord, SubtitleSegment};
use super::WordToken;

pub(super) fn segment_from_words(words: &[WordToken]) -> SubtitleSegment {
    let start = words.first().map(|w| w.start).unwrap_or(0.0);
    let end = words.last().map(|w| w.end).unwrap_or(start);
    let text = words_to_sentence_text(words);
    SubtitleSegment {
        start_sec: start,
        end_sec: end,
        text,
        words: words
            .iter()
            .map(|w| SegmentWord {
                start: w.start,
                end: w.end,
                word: w.word.clone(),
            })
            .collect(),
    }
}

pub(super) fn plain_text_from_segments(segments: &[SubtitleSegment]) -> String {
    segments
        .iter()
        .map(|s| s.text.trim())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn words_to_sentence_text(words: &[WordToken]) -> String {
    let mut text = String::new();
    for token in words {
        let part = token.word.trim();
        if part.is_empty() {
            continue;
        }
        if text.is_empty() {
            text.push_str(part);
            continue;
        }

        if is_no_space_before(part) {
            text.push_str(part);
        } else {
            text.push(' ');
            text.push_str(part);
        }
    }
    text
}

fn is_no_space_before(word: &str) -> bool {
    matches!(
        word,
        "." | ","
            | "!"
            | "?"
            | ":"
            | ";"
            | ")"
            | "]"
            | "}"
            | "。"
            | "，"
            | "！"
            | "？"
            | "："
            | "；"
            | "、"
    )
}
