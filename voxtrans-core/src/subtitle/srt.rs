use parakeet_rs::TimedToken;
use std::fmt::{Display, Formatter};

#[derive(Debug, Clone)]
pub struct SegmentWord {
    pub start: f64,
    pub end: f64,
    pub word: String,
}

#[derive(Debug, Clone)]
pub struct SubtitleSegment {
    pub start_sec: f64,
    pub end_sec: f64,
    pub text: String,
    pub words: Vec<SegmentWord>,
}

#[derive(Debug, Clone)]
pub struct SrtCue {
    pub index: usize,
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

#[derive(Debug, Clone)]
pub enum SrtError {
    MissingTimestampLine { block: usize },
    InvalidTimestampLine { block: usize, line: String },
    InvalidTimeValue { block: usize, value: String },
}

impl Display for SrtError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            SrtError::MissingTimestampLine { block } => {
                write!(f, "missing timestamp line at block {}", block)
            }
            SrtError::InvalidTimestampLine { block, line } => {
                write!(f, "invalid timestamp line at block {}: {}", block, line)
            }
            SrtError::InvalidTimeValue { block, value } => {
                write!(f, "invalid time value at block {}: {}", block, value)
            }
        }
    }
}

impl std::error::Error for SrtError {}

pub fn parse_srt(input: &str) -> Result<Vec<SrtCue>, SrtError> {
    let normalized = input.replace("\r\n", "\n");
    let mut cues = Vec::new();

    for (block_idx, block) in normalized
        .split("\n\n")
        .map(str::trim)
        .filter(|b| !b.is_empty())
        .enumerate()
    {
        let block_number = block_idx + 1;
        let mut lines: Vec<&str> = block.lines().map(str::trim_end).collect();
        if lines.is_empty() {
            continue;
        }

        if lines[0].chars().all(|ch| ch.is_ascii_digit()) {
            lines.remove(0);
        }

        let timestamp_line = lines
            .first()
            .ok_or(SrtError::MissingTimestampLine {
                block: block_number,
            })?
            .trim();

        let (start_str, end_str) =
            parse_timestamp_line(timestamp_line).ok_or_else(|| SrtError::InvalidTimestampLine {
                block: block_number,
                line: timestamp_line.to_string(),
            })?;

        let start_ms =
            parse_srt_time_to_ms(start_str).ok_or_else(|| SrtError::InvalidTimeValue {
                block: block_number,
                value: start_str.to_string(),
            })?;

        let end_ms = parse_srt_time_to_ms(end_str).ok_or_else(|| SrtError::InvalidTimeValue {
            block: block_number,
            value: end_str.to_string(),
        })?;

        let text = if lines.len() >= 2 {
            lines[1..].join("\n").trim().to_string()
        } else {
            String::new()
        };

        cues.push(SrtCue {
            index: block_number,
            start_ms,
            end_ms,
            text,
        });
    }

    Ok(cues)
}

pub fn normalize_cues(cues: &[SrtCue]) -> Vec<SrtCue> {
    let mut normalized: Vec<SrtCue> = cues
        .iter()
        .map(|cue| {
            let mut text = cue.text.replace("\r\n", "\n");
            text = text
                .lines()
                .map(str::trim_end)
                .collect::<Vec<_>>()
                .join("\n");
            let start_ms = cue.start_ms;
            let end_ms = cue.end_ms.max(start_ms);
            SrtCue {
                index: cue.index,
                start_ms,
                end_ms,
                text: text.trim().to_string(),
            }
        })
        .collect();

    normalized.sort_by_key(|cue| cue.start_ms);
    for (idx, cue) in normalized.iter_mut().enumerate() {
        cue.index = idx + 1;
    }

    normalized
}

pub fn validate_cues(cues: &[SrtCue]) -> Vec<String> {
    let mut warnings = Vec::new();

    for (idx, cue) in cues.iter().enumerate() {
        if cue.text.trim().is_empty() {
            warnings.push(format!("cue {} has empty text", idx + 1));
        }
        if cue.end_ms < cue.start_ms {
            warnings.push(format!("cue {} has end before start", idx + 1));
        }
        if cue.end_ms.saturating_sub(cue.start_ms) > 60_000 {
            warnings.push(format!("cue {} is longer than 60 seconds", idx + 1));
        }
        if idx > 0 && cue.start_ms < cues[idx - 1].end_ms {
            warnings.push(format!("cue {} overlaps with cue {}", idx + 1, idx));
        }
    }

    warnings
}

pub fn to_srt_from_cues(cues: &[SrtCue]) -> String {
    let normalized = normalize_cues(cues);
    if normalized.is_empty() {
        return String::new();
    }

    let mut out = String::new();
    for cue in normalized {
        out.push_str(&cue.index.to_string());
        out.push('\n');
        out.push_str(&format!(
            "{} --> {}\n",
            format_ms_to_srt_time(cue.start_ms),
            format_ms_to_srt_time(cue.end_ms)
        ));
        out.push_str(cue.text.trim());
        out.push_str("\n\n");
    }

    out
}

pub fn shift_all(cues: &[SrtCue], delta_ms: i64) -> Vec<SrtCue> {
    cues.iter()
        .map(|cue| {
            let start = shift_ms(cue.start_ms, delta_ms);
            let end = shift_ms(cue.end_ms, delta_ms).max(start);
            SrtCue {
                index: cue.index,
                start_ms: start,
                end_ms: end,
                text: cue.text.clone(),
            }
        })
        .collect()
}

pub fn to_srt_from_sentence_tokens(tokens: &[TimedToken]) -> String {
    if tokens.is_empty() {
        return String::new();
    }

    let cues: Vec<SrtCue> = tokens
        .iter()
        .filter_map(|token| {
            let text = token.text.trim();
            if text.is_empty() {
                return None;
            }
            let start_ms = seconds_to_ms(token.start.max(0.0));
            let end_ms = seconds_to_ms(token.end.max(token.start));
            Some(SrtCue {
                index: 0,
                start_ms,
                end_ms,
                text: text.to_string(),
            })
        })
        .collect();

    to_srt_from_cues(&cues)
}

pub fn to_srt_from_segments(segments: &[SubtitleSegment]) -> String {
    let cues: Vec<SrtCue> = segments
        .iter()
        .filter_map(|segment| {
            let text = segment.text.trim();
            if text.is_empty() {
                return None;
            }
            let start_ms = seconds_f64_to_ms(segment.start_sec.max(0.0));
            let end_ms = seconds_f64_to_ms(segment.end_sec.max(segment.start_sec));
            Some(SrtCue {
                index: 0,
                start_ms,
                end_ms,
                text: text.to_string(),
            })
        })
        .collect();

    to_srt_from_cues(&cues)
}

fn parse_timestamp_line(line: &str) -> Option<(&str, &str)> {
    let mut parts = line.split("-->");
    let start = parts.next()?.trim();
    let end = parts.next()?.trim();
    if parts.next().is_some() || start.is_empty() || end.is_empty() {
        return None;
    }
    Some((start, end))
}

fn parse_srt_time_to_ms(value: &str) -> Option<u64> {
    let mut split = value.split(',');
    let hms = split.next()?;
    let millis = split.next()?;
    if split.next().is_some() {
        return None;
    }

    let mut hms_parts = hms.split(':');
    let hours = hms_parts.next()?.trim().parse::<u64>().ok()?;
    let minutes = hms_parts.next()?.trim().parse::<u64>().ok()?;
    let seconds = hms_parts.next()?.trim().parse::<u64>().ok()?;
    if hms_parts.next().is_some() || minutes >= 60 || seconds >= 60 {
        return None;
    }

    let millis = millis.trim().parse::<u64>().ok()?;
    if millis >= 1000 {
        return None;
    }

    Some(hours * 3_600_000 + minutes * 60_000 + seconds * 1_000 + millis)
}

fn format_ms_to_srt_time(total_ms: u64) -> String {
    let hours = total_ms / 3_600_000;
    let minutes = (total_ms % 3_600_000) / 60_000;
    let seconds = (total_ms % 60_000) / 1_000;
    let millis = total_ms % 1_000;
    format!("{hours:02}:{minutes:02}:{seconds:02},{millis:03}")
}

fn seconds_to_ms(seconds: f32) -> u64 {
    (seconds as f64 * 1000.0).round().max(0.0) as u64
}

fn seconds_f64_to_ms(seconds: f64) -> u64 {
    (seconds * 1000.0).round().max(0.0) as u64
}

fn shift_ms(value: u64, delta: i64) -> u64 {
    if delta >= 0 {
        value.saturating_add(delta as u64)
    } else {
        value.saturating_sub(delta.unsigned_abs())
    }
}
