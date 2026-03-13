use crate::services::translation::domain::{AlignedCue, StageResult};

pub fn run_stage(enabled: bool, aligned: Vec<AlignedCue>) -> StageResult<Vec<AlignedCue>> {
    if !enabled {
        let mut result = StageResult::skipped_with(aligned);
        result
            .warnings
            .push("stage disabled in current milestone".to_string());
        return result;
    }
    // Minimal implementation: no re-segmentation yet, keep one-to-one cue alignment.
    StageResult::executed(aligned)
}
