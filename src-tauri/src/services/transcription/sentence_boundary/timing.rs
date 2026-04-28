use crate::services::transcribe::WordTokenDto;

pub(super) fn span_duration_ms(words: &[WordTokenDto], start: usize, end: usize) -> u64 {
    if start >= words.len() || end >= words.len() || start > end {
        return 0;
    }
    ((words[end].end - words[start].start).max(0.0) * 1000.0).round() as u64
}

pub(super) fn gap_ms(left_end_sec: f64, right_start_sec: f64) -> u64 {
    ((right_start_sec - left_end_sec).max(0.0) * 1000.0).round() as u64
}

pub(super) fn seconds_to_ms(value: f64) -> u64 {
    (value.max(0.0) * 1000.0).round() as u64
}
