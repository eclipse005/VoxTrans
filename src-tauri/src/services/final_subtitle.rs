use serde::{Deserialize, Serialize};
use voxtrans_core::subtitle::srt::{SrtCue, to_srt_from_cues};

use crate::services::transcribe::{SegmentWithWordsDto, WordTokenDto};
use crate::services::translate::types::TranslateSegment;

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FinalSubtitleWord {
    pub start_ms: i64,
    pub end_ms: i64,
    #[serde(default)]
    pub word: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FinalSubtitleSegment {
    pub start_ms: i64,
    pub end_ms: i64,
    #[serde(default)]
    pub source_text: String,
    #[serde(default)]
    pub translated_text: String,
    #[serde(default)]
    pub source_words: Vec<FinalSubtitleWord>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FinalSubtitleTrack {
    Source,
    Target,
    BilingualSourceFirst,
    BilingualTargetFirst,
}

pub fn parse_final_subtitle_segments(raw: &str) -> Vec<FinalSubtitleSegment> {
    serde_json::from_str::<Vec<serde_json::Value>>(raw)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(normalize_segment_value)
        .collect()
}

pub fn normalize_final_subtitle_segments_json(raw: &str) -> Option<String> {
    serde_json::to_string(&parse_final_subtitle_segments(raw)).ok()
}

pub fn final_subtitle_segments_to_srt(
    segments: &[FinalSubtitleSegment],
    track: FinalSubtitleTrack,
) -> String {
    let cues = segments
        .iter()
        .enumerate()
        .map(|(index, segment)| {
            let source = segment.source_text.trim();
            let target = segment.translated_text.trim();
            let text = match track {
                FinalSubtitleTrack::Source => source.to_string(),
                FinalSubtitleTrack::Target => target.to_string(),
                FinalSubtitleTrack::BilingualSourceFirst => format!("{source}\n{target}"),
                FinalSubtitleTrack::BilingualTargetFirst => format!("{target}\n{source}"),
            };
            SrtCue {
                index: index + 1,
                start_ms: segment.start_ms.max(0) as u64,
                end_ms: segment.end_ms.max(segment.start_ms).max(0) as u64,
                text,
            }
        })
        .collect::<Vec<_>>();
    to_srt_from_cues(&cues)
}

pub fn final_subtitle_segments_from_source_segments(
    segments: &[SegmentWithWordsDto],
) -> Vec<FinalSubtitleSegment> {
    segments
        .iter()
        .map(|segment| FinalSubtitleSegment {
            start_ms: ms_from_sec(segment.start),
            end_ms: ms_from_sec(segment.end),
            source_text: segment.text.clone(),
            translated_text: String::new(),
            source_words: segment
                .words
                .iter()
                .map(final_subtitle_word_from_dto)
                .collect(),
        })
        .collect()
}

pub fn final_subtitle_segments_from_translate_segments(
    segments: &[TranslateSegment],
    source_words: &[FinalSubtitleWord],
) -> Vec<FinalSubtitleSegment> {
    let mut result = segments
        .iter()
        .map(|segment| FinalSubtitleSegment {
            start_ms: segment.start_ms as i64,
            end_ms: segment.end_ms as i64,
            source_text: segment.source_text.clone(),
            translated_text: segment.translated_text.clone(),
            source_words: Vec::new(),
        })
        .collect::<Vec<_>>();
    if result.is_empty() {
        return result;
    }

    let mut seg_idx = 0usize;
    for word in source_words.iter().cloned() {
        while seg_idx + 1 < result.len()
            && word_midpoint_ms(&word) >= result[seg_idx].end_ms
            && word_midpoint_ms(&word) >= result[seg_idx + 1].start_ms
        {
            seg_idx += 1;
        }
        let assigned_idx = locate_word_segment(&result, seg_idx, &word).unwrap_or(seg_idx);
        result[assigned_idx].source_words.push(word);
    }
    result
}

pub fn final_subtitle_words_from_word_dtos(words: &[WordTokenDto]) -> Vec<FinalSubtitleWord> {
    words.iter().map(final_subtitle_word_from_dto).collect()
}

pub fn cues_to_final_subtitle_segments(
    cues: &[SrtCue],
    existing_segments: &[FinalSubtitleSegment],
) -> Vec<FinalSubtitleSegment> {
    let _ = existing_segments;
    cues.iter()
        .map(|cue| FinalSubtitleSegment {
            start_ms: cue.start_ms as i64,
            end_ms: cue.end_ms as i64,
            source_text: cue.text.clone(),
            translated_text: String::new(),
            source_words: Vec::new(),
        })
        .collect()
}

fn normalize_segment_value(value: serde_json::Value) -> Option<FinalSubtitleSegment> {
    let start_ms = value.get("startMs").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let end_ms = value
        .get("endMs")
        .and_then(|v| v.as_f64())
        .unwrap_or(start_ms)
        .round() as i64;
    if !start_ms.is_finite() {
        return None;
    }
    let start_ms = start_ms.round() as i64;
    let source_words = value
        .get("sourceWords")
        .and_then(|v| v.as_array())
        .map(|words| {
            words
                .iter()
                .filter_map(|word| normalize_word_value(word.clone()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Some(FinalSubtitleSegment {
        start_ms: start_ms.max(0),
        end_ms: end_ms.max(start_ms),
        source_text: value
            .get("sourceText")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        translated_text: value
            .get("translatedText")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        source_words,
    })
}

fn normalize_word_value(value: serde_json::Value) -> Option<FinalSubtitleWord> {
    let start_ms = value.get("startMs").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let end_ms = value
        .get("endMs")
        .and_then(|v| v.as_f64())
        .unwrap_or(start_ms)
        .round() as i64;
    if !start_ms.is_finite() {
        return None;
    }
    let start_ms = start_ms.round() as i64;
    Some(FinalSubtitleWord {
        start_ms: start_ms.max(0),
        end_ms: end_ms.max(start_ms),
        word: value
            .get("word")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
    })
}

fn final_subtitle_word_from_dto(word: &WordTokenDto) -> FinalSubtitleWord {
    FinalSubtitleWord {
        start_ms: ms_from_sec(word.start),
        end_ms: ms_from_sec(word.end),
        word: word.word.clone(),
    }
}

fn ms_from_sec(value: f64) -> i64 {
    (value.max(0.0) * 1000.0).round() as i64
}

fn word_midpoint_ms(word: &FinalSubtitleWord) -> i64 {
    (word.start_ms + word.end_ms) / 2
}

fn locate_word_segment(
    segments: &[FinalSubtitleSegment],
    start_index: usize,
    word: &FinalSubtitleWord,
) -> Option<usize> {
    let midpoint = word_midpoint_ms(word);
    for (offset, segment) in segments.iter().enumerate().skip(start_index) {
        let is_last = offset + 1 == segments.len();
        if midpoint < segment.start_ms {
            return Some(offset.saturating_sub(1));
        }
        if midpoint < segment.end_ms || (is_last && midpoint <= segment.end_ms) {
            return Some(offset);
        }
    }
    segments.len().checked_sub(1)
}

