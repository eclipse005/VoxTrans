use crate::services::translation::domain::{AlignedCue, QaStageMetrics, QaSummary, StageResult};

pub fn run_stage(enabled: bool, cues: Vec<AlignedCue>) -> StageResult<QaStageMetrics> {
    // Minimal implementation: QA/fix loop will be added in next iteration.
    let qa = QaSummary {
        issue_total: 0,
        fixed_total: 0,
        unresolved_total: 0,
        issues: Vec::new(),
    };
    let metrics = QaStageMetrics { cues, qa };
    if !enabled {
        let mut result = StageResult::skipped_with(metrics);
        result
            .warnings
            .push("stage disabled in current milestone".to_string());
        return result;
    }
    StageResult::executed(metrics)
}
