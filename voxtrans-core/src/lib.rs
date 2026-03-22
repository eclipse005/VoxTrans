use std::path::PathBuf;
use std::time::Instant;

use parakeet_rs::TimedToken;

mod audio;
mod binary;
mod provider;
mod transcribe_engine;
mod vad;

pub mod subtitle;

pub use provider::Provider;
pub use subtitle::srt::to_srt_from_sentence_tokens as to_srt;

use audio::prepare_audio_for_transcription;
use provider::to_execution_provider;
use transcribe_engine::{merge_punctuation_tokens, to_timestamp_mode, transcribe_in_segments};
use vad::build_segments_from_vad;

const DEFAULT_CHUNK_TARGET_SECONDS: f64 = 300.0;
pub(crate) const TARGET_SAMPLE_RATE: u32 = 16_000;
pub const DEFAULT_PROVIDER: Provider = Provider::Directml;

#[derive(Debug, Clone)]
pub struct TranscribeOptions {
    pub model_dir: PathBuf,
    pub audio_path: PathBuf,
    pub provider: Provider,
    pub timestamp_mode: TimestampKind,
    pub intra_threads: usize,
    pub inter_threads: usize,
    pub chunk_target_seconds: f64,
}

impl Default for TranscribeOptions {
    fn default() -> Self {
        let intra_threads = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);

        Self {
            model_dir: default_model_dir(),
            audio_path: PathBuf::new(),
            provider: DEFAULT_PROVIDER,
            timestamp_mode: TimestampKind::Sentences,
            intra_threads,
            inter_threads: 1,
            chunk_target_seconds: DEFAULT_CHUNK_TARGET_SECONDS,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum TimestampKind {
    Words,
    Sentences,
    Tokens,
}

#[derive(Debug, Clone)]
pub struct SegmentSummary {
    pub index: usize,
    pub duration_sec: f64,
}

#[derive(Debug, Clone)]
pub struct TranscribeOutput {
    pub text: String,
    pub tokens: Vec<TimedToken>,
    pub audio_duration_sec: f64,
    pub vad_elapsed_sec: f64,
    pub transcribe_elapsed_sec: f64,
    pub execution_provider: &'static str,
    pub segment_summaries: Vec<SegmentSummary>,
}

pub fn transcribe_with_parakeet_v2(
    options: &TranscribeOptions,
) -> Result<TranscribeOutput, Box<dyn std::error::Error>> {
    transcribe_with_parakeet_v2_with_progress(options, |_current, _total| {})
}

pub fn transcribe_with_parakeet_v2_with_progress<F>(
    options: &TranscribeOptions,
    mut on_segment_progress: F,
) -> Result<TranscribeOutput, Box<dyn std::error::Error>>
where
    F: FnMut(usize, usize),
{
    if options.audio_path.as_os_str().is_empty() {
        return Err("audio_path is required".into());
    }

    let execution_provider = to_execution_provider(options.provider);
    let timestamp_mode = to_timestamp_mode(options.timestamp_mode);
    let prepared_audio = prepare_audio_for_transcription(&options.audio_path)?;
    let audio_duration_sec = prepared_audio.duration_sec;
    let (segments, vad_elapsed_sec) = build_segments_from_vad(
        &prepared_audio.vad_wav.path,
        audio_duration_sec,
        options.chunk_target_seconds,
    )?;

    let started_at = Instant::now();
    let result = transcribe_in_segments(
        &options.model_dir,
        &prepared_audio.mono_samples,
        execution_provider,
        timestamp_mode,
        options.intra_threads,
        options.inter_threads,
        &segments,
        &mut on_segment_progress,
    )?;
    let result = match options.timestamp_mode {
        TimestampKind::Words => result,
        _ => merge_punctuation_tokens(result),
    };

    let elapsed_sec = started_at.elapsed().as_secs_f64();
    let execution_provider = options.provider.id();

    let segment_summaries = segments
        .iter()
        .map(|s| SegmentSummary {
            index: s.index + 1,
            duration_sec: s.duration_sec(),
        })
        .collect();

    Ok(TranscribeOutput {
        text: result.text,
        tokens: result.tokens,
        audio_duration_sec,
        vad_elapsed_sec,
        transcribe_elapsed_sec: elapsed_sec,
        execution_provider,
        segment_summaries,
    })
}

fn default_model_dir() -> PathBuf {
    if let Ok(custom) = std::env::var("VOXTRANS_MODEL_DIR") {
        let path = PathBuf::from(custom);
        if path.exists() {
            return path;
        }
    }

    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            let candidate = exe_dir.join("model").join("parakeet-tdt-0.6b-v2");
            if candidate.exists() {
                return candidate;
            }
        }
    }

    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("model")
        .join("parakeet-tdt-0.6b-v2")
}
