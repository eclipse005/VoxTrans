use voxtrans_core::subtitle::srt::{SegmentWord, SubtitleSegment};

use crate::services::transcribe::{SegmentWithWordsDto, WordTokenDto};

use super::domain::TimedHotwordSegment;

pub fn to_timed_segments(segments: &[SegmentWithWordsDto]) -> Vec<TimedHotwordSegment> {
    segments
        .iter()
        .map(|segment| TimedHotwordSegment {
            start_ms: (segment.start * 1000.0).round() as i64,
            end_ms: (segment.end * 1000.0).round() as i64,
            source_text: segment.text.clone(),
            words: segment.words.clone(),
        })
        .collect()
}

pub fn flatten_words(segments: &[TimedHotwordSegment]) -> Vec<WordTokenDto> {
    segments.iter().flat_map(|s| s.words.clone()).collect()
}

pub fn to_core_segments(segments: &[TimedHotwordSegment]) -> Vec<SubtitleSegment> {
    segments
        .iter()
        .map(|segment| SubtitleSegment {
            start_sec: (segment.start_ms as f64 / 1000.0).max(0.0),
            end_sec: (segment.end_ms as f64 / 1000.0).max(segment.start_ms as f64 / 1000.0),
            text: segment.source_text.clone(),
            words: segment
                .words
                .iter()
                .map(|w| SegmentWord {
                    start: w.start,
                    end: w.end,
                    word: w.word.clone(),
                })
                .collect(),
        })
        .collect()
}

pub fn to_segment_words_dto(segments: &[SubtitleSegment]) -> Vec<SegmentWithWordsDto> {
    segments
        .iter()
        .map(|segment| SegmentWithWordsDto {
            start: segment.start_sec,
            end: segment.end_sec,
            text: segment.text.clone(),
            words: segment
                .words
                .iter()
                .map(|w| WordTokenDto {
                    start: w.start,
                    end: w.end,
                    word: w.word.clone(),
                })
                .collect(),
        })
        .collect()
}
