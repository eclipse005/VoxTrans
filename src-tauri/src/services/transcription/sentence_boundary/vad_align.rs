//! Tolerance query: does a cut point between two words fall inside a VAD
//! silence gap (i.e. the two words are in different speech segments)?
//!
//! VAD segment boundaries and forced-aligner word timestamps never align
//! exactly — VAD lags up to ~200ms (frame accumulation), aligner extends
//! word edges into silence by tens of ms. We absorb both with a tolerance
//! window applied to each silence gap before testing the cut point.

/// Tolerance (seconds) added to each end of a VAD silence gap when testing
/// whether a cut point falls inside it. 100ms covers VAD frame lag (10ms
/// steps × min_silence_frame accumulation) plus aligner jitter, without
/// being so large that adjacent words get misjudged as crossing a gap.
pub(super) const CUT_POINT_TOLERANCE_SEC: f64 = 0.100;

/// Sorted speech segments `[(start_sec, end_sec)]`. Built once from
/// fireredvad output (pre-normalized by the caller) and reused for every
/// word-pair query.
#[derive(Debug, Clone)]
pub(super) struct SpeechSegmentIndex {
    segments: Vec<(f64, f64)>,
}

impl SpeechSegmentIndex {
    pub(super) fn new(mut segments: Vec<(f64, f64)>) -> Self {
        segments.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        Self { segments }
    }

    /// Returns true when the cut point between `left_end_sec` and
    /// `right_start_sec` falls inside a VAD silence gap — i.e. the two
    /// words belong to different speech segments. Uses the midpoint of the
    /// word-pair gap as the test point (symmetric against aligner drift in
    /// either direction), with `CUT_POINT_TOLERANCE_SEC` of slack on each
    /// edge of the silence.
    ///
    /// Fewer than two segments => always false (no silence gap exists, or
    /// no VAD data — caller degrades to punctuation + length budget).
    pub(super) fn crosses_silence(
        &self,
        left_end_sec: f64,
        right_start_sec: f64,
    ) -> bool {
        if self.segments.len() < 2 {
            return false;
        }
        let cut = (left_end_sec + right_start_sec) / 2.0;
        for window in self.segments.windows(2) {
            let silence_start = window[0].1;
            let silence_end = window[1].0;
            if cut >= silence_start - CUT_POINT_TOLERANCE_SEC
                && cut <= silence_end + CUT_POINT_TOLERANCE_SEC
            {
                return true;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn idx() -> SpeechSegmentIndex {
        // Three speech segments with two silence gaps: [2.0,2.0]-[3.0,3.0]
        // gap is 1.0s wide; [5.0,5.0]-[6.0,6.0] gap is 1.0s wide.
        SpeechSegmentIndex::new(vec![(0.0, 2.0), (3.0, 5.0), (6.0, 8.0)])
    }

    #[test]
    fn cut_at_silence_midpoint_crosses() {
        let i = idx();
        // Words span the [2.0, 3.0] gap; midpoint = 2.5.
        assert!(i.crosses_silence(2.2, 2.8));
    }

    #[test]
    fn cut_inside_a_speech_segment_does_not_cross() {
        let i = idx();
        // Both words inside [3.0, 5.0].
        assert!(!i.crosses_silence(3.5, 4.0));
    }

    #[test]
    fn cut_within_tolerance_of_silence_edge_crosses() {
        let i = idx();
        // Midpoint 2.05 sits 50ms inside the [2.0,3.0] gap start — within
        // the 100ms tolerance.
        assert!(i.crosses_silence(2.18, 1.92));
    }

    #[test]
    fn cut_beyond_tolerance_does_not_cross() {
        let i = idx();
        // Midpoint 1.85 is 150ms before the gap start (2.0); outside the
        // 100ms tolerance window on that side.
        assert!(!i.crosses_silence(1.9, 1.8));
    }

    #[test]
    fn empty_segments_never_cross() {
        let i = SpeechSegmentIndex::new(vec![]);
        assert!(!i.crosses_silence(1.0, 5.0));
    }

    #[test]
    fn single_segment_never_cross() {
        let i = SpeechSegmentIndex::new(vec![(0.0, 10.0)]);
        assert!(!i.crosses_silence(3.0, 4.0));
    }

    #[test]
    fn second_gap_also_detected() {
        let i = idx();
        // Words span the [5.0, 6.0] gap.
        assert!(i.crosses_silence(5.1, 5.9));
    }
}
