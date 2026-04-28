use super::translate_step5_common::{
    step5_artifact_meta, step5_pipeline_version_value, step5_schema_version_value,
    validate_step5_command_base,
};
use super::translate_step5_mapping::command_segments_to_step5_final;
use super::translate_types::{
    BuildStep6FinalCheckCommandRequest, BuildStep6FinalCheckCommandResponse,
    BuildTranslationSegmentCommand, Step5QualityIssueCommand, Step5QualitySummaryCommand,
    Step6FinalCheckMetricsCommand,
};

#[tauri::command]
pub async fn build_step_6_final_check(
    request: BuildStep6FinalCheckCommandRequest,
) -> Result<BuildStep6FinalCheckCommandResponse, String> {
    validate_step5_command_base(
        &request.task_id,
        &request.media_path,
        &request.source_lang,
        &request.target_lang,
    )?;
    if request.segments.is_empty() {
        return Err("segments is required".to_string());
    }
    let service_response = run_step_6_final_check_request(&request.target_lang, &request.segments)?;

    let output_segments = request.segments;

    Ok(BuildStep6FinalCheckCommandResponse {
        task_id: request.task_id,
        media_path: request.media_path,
        source_lang: request.source_lang,
        target_lang: request.target_lang,
        schema_version: step5_schema_version_value(),
        pipeline_version: step5_pipeline_version_value(),
        artifact_meta: step5_artifact_meta(),
        quality_summary: Step5QualitySummaryCommand {
            passed: service_response.passed,
            hard_fail_count: service_response.hard_fail_count,
            issue_count: service_response.issue_count,
            soft_score: service_response.soft_score,
        },
        metrics: Step6FinalCheckMetricsCommand {
            segment_total: service_response.metrics.segment_total,
            empty_count: service_response.metrics.empty_count,
            ellipsis_tail_count: service_response.metrics.ellipsis_tail_count,
            numeric_drift_count: service_response.metrics.numeric_drift_count,
            cross_line_leak_count: service_response.metrics.cross_line_leak_count,
            gt25_count: service_response.metrics.gt25_count,
            gt32_count: service_response.metrics.gt32_count,
        },
        issues: service_response
            .issues
            .into_iter()
            .map(|issue| Step5QualityIssueCommand {
                rule_id: issue.rule_id,
                severity: issue.severity,
                segment_id: issue.segment_id,
                part_id: issue.part_id,
                message: issue.message,
            })
            .collect(),
        segments: output_segments,
    })
}

fn run_step_6_final_check_request(
    target_lang: &str,
    segments: &[BuildTranslationSegmentCommand],
) -> Result<crate::services::subtitle_step5::BuildStep6FinalCheckResponse, String> {
    crate::services::subtitle_step5::build_step_6_final_check(
        crate::services::subtitle_step5::BuildStep6FinalCheckRequest {
            target_lang: target_lang.to_string(),
            segments: command_segments_to_step5_final(segments),
        },
    )
}
