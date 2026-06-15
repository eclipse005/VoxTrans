use std::path::Path;

mod audio;
mod binary;
mod vad;

pub mod subtitle;

use audio::prepare_audio_for_transcription;
use vad::build_segments_from_vad;

pub const TARGET_SAMPLE_RATE: u32 = 16_000;

#[derive(Debug, Clone)]
pub struct SegmentSummary {
    pub index: usize,
    pub start_sec: f64,
    pub end_sec: f64,
    pub duration_sec: f64,
}

#[derive(Debug, Clone)]
pub struct PreparedAudioSegments {
    pub mono_samples: Vec<f32>,
    pub audio_duration_sec: f64,
    pub vad_elapsed_sec: f64,
    pub vad_speech_segments: Vec<(f64, f64)>,
    pub segment_summaries: Vec<SegmentSummary>,
}

pub fn prepare_audio_segments_for_asr(
    audio_path: &Path,
    chunk_target_seconds: f64,
) -> Result<PreparedAudioSegments, Box<dyn std::error::Error>> {
    let prepared_audio = prepare_audio_for_transcription(audio_path)?;
    let audio_duration_sec = prepared_audio.duration_sec;
    let (segments, vad_elapsed_sec, vad_speech_segments) = build_segments_from_vad(
        &prepared_audio.vad_wav.path,
        audio_duration_sec,
        chunk_target_seconds,
    )?;
    let segment_summaries = segments
        .iter()
        .map(|s| SegmentSummary {
            index: s.index + 1,
            start_sec: s.start_sec,
            end_sec: s.end_sec,
            duration_sec: s.duration_sec(),
        })
        .collect();

    Ok(PreparedAudioSegments {
        mono_samples: prepared_audio.mono_samples,
        audio_duration_sec,
        vad_elapsed_sec,
        vad_speech_segments,
        segment_summaries,
    })
}
