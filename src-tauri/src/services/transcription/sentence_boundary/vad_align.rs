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

/// How far past a silence gap's edge a word may end/start and still count as
/// attached to the speech segment on its side. Aligner word edges extend into
/// silence by tens of ms (up to ~200ms observed); a word ending well beyond
/// that is floating INSIDE the gap — a VAD dead zone where the aligner found
/// quiet speech the VAD missed — and must not inherit the gap's full width.
pub(super) const WORD_ATTACH_TOL_SEC: f64 = 0.35;

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
    /// `right_start_sec` falls inside a VAD silence gap AND some of that
    /// silence actually lies between the two words. Uses the midpoint of the
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
        self.silence_duration_sec(left_end_sec, right_start_sec) > 0.0
    }

    /// Width (seconds) of the VAD silence that separates the words around the
    /// cut point. Returns 0.0 when the cut does not cross a silence gap (same
    /// tolerance window as [`Self::crosses_silence`]). Used to weigh the DP
    /// cost and to gate the ≥2s hard-pause split, so the number must be the
    /// pause BETWEEN THE TWO WORDS, not the width of the containing gap:
    ///
    /// - Words attached to opposite speech segments (each within
    ///   [`WORD_ATTACH_TOL_SEC`] of its side of the gap): the whole gap
    ///   separates them — aligner edges may stretch into the silence.
    /// - Otherwise (e.g. both words floating deep inside a long VAD dead
    ///   zone where the aligner found quiet speech the VAD missed): only the
    ///   part of the gap lying between the two words counts. Inheriting the
    ///   dead zone's full width would hard-split slow speech at every word.
    pub(super) fn silence_duration_sec(
        &self,
        left_end_sec: f64,
        right_start_sec: f64,
    ) -> f64 {
        if self.segments.len() < 2 {
            return 0.0;
        }
        let cut = (left_end_sec + right_start_sec) / 2.0;
        for window in self.segments.windows(2) {
            let silence_start = window[0].1;
            let silence_end = window[1].0;
            if cut >= silence_start - CUT_POINT_TOLERANCE_SEC
                && cut <= silence_end + CUT_POINT_TOLERANCE_SEC
            {
                if left_end_sec <= silence_start + WORD_ATTACH_TOL_SEC
                    && right_start_sec >= silence_end - WORD_ATTACH_TOL_SEC
                {
                    return (silence_end - silence_start).max(0.0);
                }
                let overlap =
                    right_start_sec.min(silence_end) - left_end_sec.max(silence_start);
                return overlap.max(0.0);
            }
        }
        0.0
    }
}

/// Map a VAD silence duration to a normalized strength in `[0.0, 1.0]`.
///
/// The curve is piecewise-linear: breath pauses (<0.3s) contribute nothing;
/// the signal ramps up through 0.5s (0.25), 0.8s (0.55), 1.2s (0.85), and
/// saturates at 1.0 beyond 2.0s. Longer silences are progressively stronger
/// sentence-end signals — a speaker who pauses for 1.2s almost certainly
/// finished a thought. Language-independent (purely acoustic).
///
/// Used by the DP cost function: `cost = 2.0 - vad_strength(silence)`, so a
/// 1.2s pause (strength 0.85) costs 1.15 — nearly as good as soft clause
/// punctuation (1.0) — while a 0.4s breath (strength ~0.07) costs ~1.93,
/// barely better than a plain word boundary.
pub(super) fn vad_strength(silence_sec: f64) -> f64 {
    // Anchor points: (silence_seconds, strength)
    const ANCHORS: [(f64, f64); 6] = [
        (0.0, 0.0),
        (0.3, 0.0),   // below this: noise/breath, no signal
        (0.5, 0.25),
        (0.8, 0.55),
        (1.2, 0.85),
        (2.0, 1.0),   // saturated
    ];
    if silence_sec <= 0.3 {
        return 0.0;
    }
    if silence_sec >= 2.0 {
        return 1.0;
    }
    // Linear interpolation between the two surrounding anchors.
    for window in ANCHORS.windows(2) {
        let (lo_s, lo_v) = window[0];
        let (hi_s, hi_v) = window[1];
        if silence_sec >= lo_s && silence_sec <= hi_s {
            if (hi_s - lo_s).abs() < f64::EPSILON {
                return hi_v;
            }
            let t = (silence_sec - lo_s) / (hi_s - lo_s);
            return lo_v + t * (hi_v - lo_v);
        }
    }
    1.0
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
        // The cut interval [1.98, 2.02] straddles the gap start (2.0); a
        // sliver of silence lies between the words, inside the tolerance.
        assert!(i.crosses_silence(1.98, 2.02));
    }

    #[test]
    fn dead_zone_words_do_not_inherit_the_silence_width() {
        // Both words float deep inside the 9s dead zone between the speech
        // segments (quiet speech the aligner found but VAD did not). The
        // counted silence is only the part between the words (0.5s), never
        // the dead zone's full width — otherwise slow speech would hard-split
        // at every word boundary.
        let i = SpeechSegmentIndex::new(vec![(0.0, 1.0), (10.0, 11.0)]);
        assert_eq!(i.silence_duration_sec(4.0, 4.5), 0.5);
        // A real inter-word pause inside the dead zone still counts at its
        // own width — wide enough gaps keep their hard-split signal.
        assert_eq!(i.silence_duration_sec(4.0, 7.5), 3.5);
    }

    #[test]
    fn stretched_word_edges_still_attach_to_speech_segments() {
        // Aligner edges extend up to ~200ms into the silence: the left word
        // ends past the gap start, the right starts before the gap end, yet
        // the words attach to opposite segments and the FULL gap counts.
        let i = SpeechSegmentIndex::new(vec![(0.0, 2.0), (3.0, 5.0), (6.0, 8.0)]);
        assert_eq!(i.silence_duration_sec(2.2, 2.8), 1.0);
        assert_eq!(i.silence_duration_sec(5.2, 5.8), 1.0);
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

    #[test]
    fn silence_duration_returns_gap_width() {
        // Custom index: a 1.5s gap between [0,1] and [2.5,4].
        let i = SpeechSegmentIndex::new(vec![(0.0, 1.0), (2.5, 4.0)]);
        // Words spanning the gap: midpoint falls inside [1.0, 2.5].
        assert_eq!(i.silence_duration_sec(1.2, 2.3), 1.5);
    }

    #[test]
    fn silence_duration_zero_when_not_crossing() {
        let i = idx();
        // Words both inside [3.0, 5.0] — no crossing.
        assert_eq!(i.silence_duration_sec(3.5, 4.0), 0.0);
    }

    #[test]
    fn vad_strength_curve_anchors() {
        // Breath / noise: no signal.
        assert_eq!(vad_strength(0.0), 0.0);
        assert_eq!(vad_strength(0.2), 0.0);
        assert_eq!(vad_strength(0.3), 0.0);
        // Ramp-up anchors.
        assert!((vad_strength(0.5) - 0.25).abs() < 1e-9);
        assert!((vad_strength(0.8) - 0.55).abs() < 1e-9);
        assert!((vad_strength(1.2) - 0.85).abs() < 1e-9);
        // Saturation.
        assert_eq!(vad_strength(2.0), 1.0);
        assert_eq!(vad_strength(5.0), 1.0);
    }

    #[test]
    fn vad_strength_monotonic_non_decreasing() {
        let mut prev = 0.0;
        let mut t = 0.0;
        while t <= 3.0 {
            let s = vad_strength(t);
            assert!(s >= prev - 1e-9, "strength decreased at t={t}: {s} < {prev}");
            prev = s;
            t += 0.05;
        }
    }
}
