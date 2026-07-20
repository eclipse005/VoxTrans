use std::path::Path;
use std::time::Instant;

use fireredvad::{Vad, VadConfig};

/// Tail shorter than this is absorbed into the previous segment so we avoid
/// tiny ASR/align windows (e.g. 1–5s remainder after ~chunk_target splits).
/// Applies to all engines (Qwen/Cohere chunk 30–180, MOSS fixed 180): max
/// merged length is chunk_target + just under 15s (e.g. MOSS ≈ 195s).
const MIN_TAIL_SEGMENT_SEC: f64 = 15.0;

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

pub(crate) fn build_segments_from_vad(
    audio_path: &Path,
    total_duration_sec: f64,
    chunk_target_seconds: f64,
) -> Result<(Vec<AudioSegment>, f64, Vec<(f64, f64)>), Box<dyn std::error::Error>> {
    let vad_started_at = Instant::now();
    let chunk_target_seconds = chunk_target_seconds.max(30.0);
    let engine = Vad::new()?;
    let cfg = VadConfig {
        speech_threshold: 0.4,
        min_speech_frame: 20,
        min_silence_frame: 15,
        max_speech_frame: 60000,
        smooth_window_size: 5,
        merge_silence_frame: 0,
        extend_speech_frame: 0,
        silence_schedule: Vec::new(),
    };
    let vad = engine.detect_wav(audio_path, &cfg)?;
    let vad_elapsed_sec = vad_started_at.elapsed().as_secs_f64();
    let effective_total_duration = if total_duration_sec > 0.0 {
        total_duration_sec
    } else {
        vad.dur as f64
    };
    if effective_total_duration <= chunk_target_seconds {
        return Ok((
            vec![AudioSegment {
                index: 0,
                start_sec: 0.0,
                end_sec: effective_total_duration,
            }],
            vad_elapsed_sec,
            vec![(0.0, effective_total_duration)],
        ));
    }

    let speech_ranges = normalize_ranges(&vad.timestamps, effective_total_duration);
    let silence_midpoints = silence_midpoints_from_vad(&speech_ranges, effective_total_duration);

    let mut split_points = Vec::new();
    let mut last = 0.0_f64;
    while last + chunk_target_seconds < effective_total_duration {
        let boundary = last + chunk_target_seconds;
        let candidate = silence_midpoints
            .iter()
            .copied()
            .filter(|mid| *mid > last + 0.2 && *mid < boundary)
            .fold(None, |acc: Option<f64>, cur| match acc {
                Some(prev) if prev > cur => Some(prev),
                _ => Some(cur),
            });
        let mut split = candidate.unwrap_or(boundary);
        if split <= last + 0.2 {
            split = boundary;
        }
        split_points.push(split);
        last = split;
    }

    let mut segments = Vec::new();
    let mut start = 0.0_f64;
    for (idx, end) in split_points.iter().enumerate() {
        // Skip zero-length segments that arise when a split point equals
        // the previous one (can happen if silence_midpoints produces a
        // boundary exactly at `last`). Empty segments break downstream
        // `windows(2)` logic and produce negative durations.
        if *end > start {
            segments.push(AudioSegment {
                index: idx,
                start_sec: start,
                end_sec: *end,
            });
            start = *end;
        }
    }
    // Final tail segment — only add if it has positive duration. When the
    // last split point equals `effective_total_duration`, the tail would
    // be zero-length; skip it instead of emitting a degenerate segment.
    if effective_total_duration > start {
        segments.push(AudioSegment {
            index: segments.len(),
            start_sec: start,
            end_sec: effective_total_duration,
        });
    }
    merge_short_tail_segment(&mut segments);
    Ok((segments, vad_elapsed_sec, speech_ranges))
}

/// If the last segment is shorter than [`MIN_TAIL_SEGMENT_SEC`], fold it into
/// the previous one. Same rule for every ASR engine / chunk_target.
fn merge_short_tail_segment(segments: &mut Vec<AudioSegment>) {
    if segments.len() < 2 {
        return;
    }
    let last_duration = segments[segments.len() - 1].duration_sec();
    if last_duration >= MIN_TAIL_SEGMENT_SEC {
        return;
    }
    let last = segments.pop().expect("len >= 2");
    if let Some(prev) = segments.last_mut() {
        prev.end_sec = last.end_sec;
    }
    for (idx, seg) in segments.iter_mut().enumerate() {
        seg.index = idx;
    }
}

fn normalize_ranges(ranges: &[(f32, f32)], total_duration_sec: f64) -> Vec<(f64, f64)> {
    if total_duration_sec <= 0.0 {
        return Vec::new();
    }

    let mut normalized: Vec<(f64, f64)> = ranges
        .iter()
        .map(|(start, end)| ((*start as f64).max(0.0), (*end as f64).min(total_duration_sec)))
        .filter(|(start, end)| *end > *start)
        .collect();
    normalized.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    let mut merged: Vec<(f64, f64)> = Vec::with_capacity(normalized.len());
    for (start, end) in normalized {
        if let Some(last) = merged.last_mut()
            && start <= last.1
        {
            last.1 = last.1.max(end);
            continue;
        }
        merged.push((start, end));
    }
    merged
}

fn silence_midpoints_from_vad(speech_ranges: &[(f64, f64)], total_duration_sec: f64) -> Vec<f64> {
    if total_duration_sec <= 0.0 {
        return Vec::new();
    }

    if speech_ranges.is_empty() {
        return Vec::new();
    }

    let mut midpoints = Vec::new();
    let mut cursor = 0.0_f64;

    for &(speech_start, speech_end) in speech_ranges {
        if speech_start > cursor {
            midpoints.push((cursor + speech_start) / 2.0);
        }
        cursor = cursor.max(speech_end);
    }

    if cursor < total_duration_sec {
        midpoints.push((cursor + total_duration_sec) / 2.0);
    }

    midpoints
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(index: usize, start: f64, end: f64) -> AudioSegment {
        AudioSegment {
            index,
            start_sec: start,
            end_sec: end,
        }
    }

    #[test]
    fn short_tail_under_15s_merges_into_previous() {
        // ~60 + ~60 + 5 → merge last into prev → ~60 + ~65
        let mut segments = vec![
            seg(0, 0.0, 60.0),
            seg(1, 60.0, 120.0),
            seg(2, 120.0, 125.0),
        ];
        merge_short_tail_segment(&mut segments);
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].start_sec, 0.0);
        assert_eq!(segments[0].end_sec, 60.0);
        assert_eq!(segments[1].start_sec, 60.0);
        assert_eq!(segments[1].end_sec, 125.0);
        assert!((segments[1].duration_sec() - 65.0).abs() < 1e-9);
        assert_eq!(segments[1].index, 1);
    }

    #[test]
    fn tail_at_least_15s_stays_separate() {
        let mut segments = vec![seg(0, 0.0, 60.0), seg(1, 60.0, 75.0)];
        merge_short_tail_segment(&mut segments);
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[1].end_sec, 75.0);
        assert!((segments[1].duration_sec() - 15.0).abs() < 1e-9);
    }

    #[test]
    fn moss_style_180s_chunk_short_tail_merges_to_under_195s() {
        // chunk_target=180, remainder 14.9s → merged max just under 195s
        let mut segments = vec![seg(0, 0.0, 180.0), seg(1, 180.0, 194.9)];
        merge_short_tail_segment(&mut segments);
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].start_sec, 0.0);
        assert_eq!(segments[0].end_sec, 194.9);
        assert!(segments[0].duration_sec() < 195.0);
        assert!(segments[0].duration_sec() > 194.0);
    }

    #[test]
    fn single_segment_unchanged() {
        let mut segments = vec![seg(0, 0.0, 8.0)];
        merge_short_tail_segment(&mut segments);
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].end_sec, 8.0);
    }
}
