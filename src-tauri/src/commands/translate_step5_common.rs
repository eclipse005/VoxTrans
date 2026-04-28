use super::translate_defaults::{step5_pipeline_version, step5_schema_version};
use super::translate_types::{Step5ArtifactMetaCommand, Step5QualitySummaryCommand};

pub(super) fn validate_step5_command_base(
    task_id: &str,
    media_path: &str,
    source_lang: &str,
    target_lang: &str,
) -> Result<(), String> {
    if task_id.trim().is_empty() {
        return Err("taskId is required".to_string());
    }
    if media_path.trim().is_empty() {
        return Err("mediaPath is required".to_string());
    }
    if source_lang.trim().is_empty() {
        return Err("sourceLang is required".to_string());
    }
    if target_lang.trim().is_empty() {
        return Err("targetLang is required".to_string());
    }
    Ok(())
}

pub(super) fn step5_schema_version_value() -> u32 {
    step5_schema_version()
}

pub(super) fn step5_pipeline_version_value() -> String {
    step5_pipeline_version().to_string()
}

pub(super) fn step5_artifact_meta() -> Step5ArtifactMetaCommand {
    Step5ArtifactMetaCommand {
        schema_version: step5_schema_version(),
        pipeline_version: step5_pipeline_version().to_string(),
    }
}

pub(super) fn step51_quality_summary(
    parent_total: usize,
    part_total: usize,
) -> Step5QualitySummaryCommand {
    Step5QualitySummaryCommand {
        passed: parent_total > 0 && part_total > 0,
        hard_fail_count: 0,
        issue_count: 0,
        soft_score: 100.0,
    }
}
