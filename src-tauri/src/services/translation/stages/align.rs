use crate::services::translation::domain::AlignedCue;

pub fn run(aligned: Vec<AlignedCue>) -> Vec<AlignedCue> {
    // Minimal implementation: no re-segmentation yet, keep one-to-one cue alignment.
    aligned
}
