//! Shared time-conversion helpers.
//!
//! `seconds_to_millis` was previously duplicated in three places with
//! subtly different NaN/overflow handling (see domain::task::adapters,
//! services::subtitle_step5::time_utils, services::transcription::
//! sentence_boundary::timing). Centralizing here ensures the same
//! timestamp produces the same millisecond value across pipeline steps.

/// Convert seconds to milliseconds, rounding to the nearest ms.
///
/// Negative values and NaN/Infinity collapse to 0, which is the correct
/// behavior for subtitle timestamps that may arrive as NaN from upstream
/// ASR/alignment when no word boundary was detected.
pub fn seconds_to_millis(value: f64) -> u64 {
    if !value.is_finite() || value <= 0.0 {
        return 0;
    }
    (value * 1000.0).round() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_and_negative_collapse_to_zero() {
        assert_eq!(seconds_to_millis(0.0), 0);
        assert_eq!(seconds_to_millis(-1.5), 0);
    }

    #[test]
    fn nan_and_infinity_collapse_to_zero() {
        assert_eq!(seconds_to_millis(f64::NAN), 0);
        assert_eq!(seconds_to_millis(f64::INFINITY), 0);
        assert_eq!(seconds_to_millis(f64::NEG_INFINITY), 0);
    }

    #[test]
    fn rounds_to_nearest_millisecond() {
        assert_eq!(seconds_to_millis(1.2345), 1235);
        assert_eq!(seconds_to_millis(1.2344), 1234);
    }
}
