use serde_json::Value;

use crate::commands::translate_types::BuildStep6FinalCheckCommandResponse;
use crate::services::pipeline::StepSource;

use super::STEP_06_FINAL_CHECK_FILE;

pub(super) fn task_failure_log_payload(error: &str) -> Value {
    serde_json::json!({
        "status": "error",
        "error": error,
    })
}

pub(super) fn step6_final_check_log_payload(
    response: &BuildStep6FinalCheckCommandResponse,
    source: StepSource,
) -> Value {
    let issues = response
        .issues
        .iter()
        .take(12)
        .map(|issue| {
            serde_json::json!({
                "severity": issue.severity,
                "ruleId": issue.rule_id,
                "segmentId": issue.segment_id,
                "partId": issue.part_id,
                "message": issue.message,
            })
        })
        .collect::<Vec<_>>();
    serde_json::json!({
        "status": if response.quality_summary.passed { "passed" } else { "issues_found" },
        "source": match source {
            StepSource::Cache => "cache",
            StepSource::Computed => "computed",
        },
        "artifact": STEP_06_FINAL_CHECK_FILE,
        "summary": {
            "passed": response.quality_summary.passed,
            "hardFailCount": response.quality_summary.hard_fail_count,
            "issueCount": response.quality_summary.issue_count,
            "softScore": response.quality_summary.soft_score,
        },
        "metrics": {
            "segmentTotal": response.metrics.segment_total,
            "emptyCount": response.metrics.empty_count,
            "ellipsisTailCount": response.metrics.ellipsis_tail_count,
            "numericDriftCount": response.metrics.numeric_drift_count,
            "crossLineLeakCount": response.metrics.cross_line_leak_count,
            "gt25Count": response.metrics.gt25_count,
            "gt32Count": response.metrics.gt32_count,
        },
        "issuesPreview": issues,
        "issuesPreviewLimit": 12,
    })
}

#[cfg(test)]
mod tests {
    use super::{step6_final_check_log_payload, task_failure_log_payload};
    use crate::commands::translate_types::{
        BuildStep6FinalCheckCommandResponse, Step5ArtifactMetaCommand, Step5QualityIssueCommand,
        Step5QualitySummaryCommand, Step6FinalCheckMetricsCommand,
    };

    #[test]
    fn final_check_log_payload_records_issues_without_blocking() {
        let payload = step6_final_check_log_payload(
            &BuildStep6FinalCheckCommandResponse {
                task_id: "task".to_string(),
                media_path: "media.mp4".to_string(),
                source_lang: "en".to_string(),
                target_lang: "zh-CN".to_string(),
                schema_version: 1,
                pipeline_version: "test".to_string(),
                artifact_meta: Step5ArtifactMetaCommand::default(),
                metrics: Step6FinalCheckMetricsCommand {
                    segment_total: 3,
                    numeric_drift_count: 1,
                    ..Default::default()
                },
                issues: vec![Step5QualityIssueCommand {
                    rule_id: "numeric_drift".to_string(),
                    severity: "hard".to_string(),
                    segment_id: 7,
                    part_id: 2,
                    message: "数字不一致".to_string(),
                }],
                quality_summary: Step5QualitySummaryCommand {
                    passed: false,
                    hard_fail_count: 1,
                    issue_count: 1,
                    soft_score: 75.0,
                },
                segments: Vec::new(),
            },
            crate::services::pipeline::StepSource::Computed,
        );

        assert_eq!(payload["status"], "issues_found");
        assert_eq!(payload["artifact"], "step_06_final_check.json");
        assert_eq!(payload["summary"]["hardFailCount"], 1);
        assert_eq!(payload["metrics"]["numericDriftCount"], 1);
        assert_eq!(payload["issuesPreview"][0]["ruleId"], "numeric_drift");
    }

    #[test]
    fn task_failure_log_payload_preserves_error_reason() {
        let payload =
            task_failure_log_payload("step_04_translation failed: missing translation id 20");

        assert_eq!(payload["status"], "error");
        assert_eq!(
            payload["error"],
            "step_04_translation failed: missing translation id 20"
        );
    }
}
