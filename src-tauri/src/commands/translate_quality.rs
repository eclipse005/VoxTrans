use super::translate::{Step5QualitySummaryCommand, is_tail_ellipsis};

pub fn summarize_step52_quality(
    parents: &[crate::services::subtitle_step5::Step5AlignedParent],
) -> Step5QualitySummaryCommand {
    let mut issue_count = 0usize;
    let mut hard_fail_count = 0usize;
    for parent in parents {
        for part in &parent.parts {
            let text = part.translation.trim();
            if text.is_empty() {
                issue_count += 1;
                hard_fail_count += 1;
                continue;
            }
            if is_tail_ellipsis(text) {
                issue_count += 1;
                hard_fail_count += 1;
            }
        }
    }
    Step5QualitySummaryCommand {
        passed: hard_fail_count == 0,
        hard_fail_count,
        issue_count,
        soft_score: if hard_fail_count == 0 { 100.0 } else { 75.0 },
    }
}
