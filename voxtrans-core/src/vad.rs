use std::path::Path;
use std::time::Instant;

use fireredvad::VadConfig;

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
) -> Result<(Vec<AudioSegment>, f64), Box<dyn std::error::Error>> {
    let vad_started_at = Instant::now();
    let chunk_target_seconds = chunk_target_seconds.max(30.0);
    let vad = fireredvad::detect(audio_path, &VadConfig::default())?;
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
        segments.push(AudioSegment {
            index: idx,
            start_sec: start,
            end_sec: *end,
        });
        start = *end;
    }
    segments.push(AudioSegment {
        index: segments.len(),
        start_sec: start,
        end_sec: effective_total_duration,
    });
    Ok((segments, vad_elapsed_sec))
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
