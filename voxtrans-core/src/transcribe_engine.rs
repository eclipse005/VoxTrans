use std::path::Path;

use parakeet_rs::{
    ExecutionConfig, ExecutionProvider, ParakeetTDT, TimedToken, TimestampMode, Transcriber,
    TranscriptionResult,
};

use crate::{TimestampKind, TARGET_SAMPLE_RATE};

#[derive(Debug, Clone)]
pub(crate) struct AudioSegment {
    pub index: usize,
    pub start_sec: f64,
    pub end_sec: f64,
}

impl AudioSegment {
    pub fn duration_sec(&self) -> f64 {
        self.end_sec - self.start_sec
    }
}

pub(crate) fn transcribe_in_segments(
    model_dir: &Path,
    full_audio_samples: &[f32],
    provider: ExecutionProvider,
    timestamp_mode: TimestampMode,
    intra_threads: usize,
    inter_threads: usize,
    segments: &[AudioSegment],
    on_segment_progress: &mut dyn FnMut(usize, usize),
) -> Result<TranscriptionResult, Box<dyn std::error::Error>> {
    let mut model = ParakeetTDT::from_pretrained(
        model_dir,
        Some(
            ExecutionConfig::new()
                .with_execution_provider(provider)
                .with_intra_threads(intra_threads)
                .with_inter_threads(inter_threads),
        ),
    )?;

    let mut all_tokens: Vec<TimedToken> = Vec::new();
    let mut text_parts: Vec<String> = Vec::new();

    let total_segments = segments.len();
    let sample_len = full_audio_samples.len();
    for segment in segments {
        on_segment_progress(segment.index + 1, total_segments);
        let start_index =
            ((segment.start_sec * TARGET_SAMPLE_RATE as f64).floor() as usize).min(sample_len);
        let end_index =
            ((segment.end_sec * TARGET_SAMPLE_RATE as f64).ceil() as usize).min(sample_len);
        if end_index <= start_index {
            continue;
        }
        let mut segment_result = model.transcribe_samples(
            full_audio_samples[start_index..end_index].to_vec(),
            TARGET_SAMPLE_RATE,
            1,
            Some(timestamp_mode),
        )?;

        if !segment_result.text.trim().is_empty() {
            text_parts.push(segment_result.text.trim().to_string());
        }

        for token in &mut segment_result.tokens {
            token.start += segment.start_sec as f32;
            token.end += segment.start_sec as f32;
        }

        all_tokens.extend(segment_result.tokens);
    }

    Ok(TranscriptionResult {
        text: text_parts.join(" "),
        tokens: all_tokens,
    })
}

pub(crate) fn to_timestamp_mode(mode: TimestampKind) -> TimestampMode {
    match mode {
        TimestampKind::Words => TimestampMode::Words,
        TimestampKind::Sentences => TimestampMode::Sentences,
        TimestampKind::Tokens => TimestampMode::Tokens,
    }
}

pub(crate) fn merge_punctuation_tokens(mut result: TranscriptionResult) -> TranscriptionResult {
    let mut merged: Vec<TimedToken> = Vec::with_capacity(result.tokens.len());

    for token in result.tokens {
        if is_standalone_punctuation(&token.text) {
            if let Some(prev) = merged.last_mut() {
                prev.text.push_str(&token.text);
                prev.end = token.end;
            } else {
                merged.push(token);
            }
        } else {
            merged.push(token);
        }
    }

    result.tokens = merged;
    result
}

fn is_standalone_punctuation(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }
    trimmed.chars().all(|c| {
        matches!(
            c,
            ',' | '.' | '!' | '?' | ';' | ':' | '，' | '。' | '！' | '？' | '；' | '：'
        )
    })
}
