use async_trait::async_trait;
use tauri::AppHandle;

use crate::commands::translate_final_check::build_step_6_final_check;
use crate::commands::translate_types::{
    BuildStep6FinalCheckCommandRequest, BuildStep6FinalCheckCommandResponse,
    BuildTranslationSegmentCommand,
};
use crate::services::pipeline::{CheckpointPolicy, PipelineStep, StepContext};

use super::super::progress::report_task_stage;
use super::super::{STEP_06_FINAL_CHECK_FILE, TaskStage};

#[derive(Debug, Clone)]
pub(in crate::commands::workspace) struct Step6FinalCheckPipelineStep {
    pub(in crate::commands::workspace) task_id: String,
    pub(in crate::commands::workspace) media_path: String,
    pub(in crate::commands::workspace) source_lang: String,
    pub(in crate::commands::workspace) target_lang: String,
    pub(in crate::commands::workspace) segments: Vec<BuildTranslationSegmentCommand>,
    pub(in crate::commands::workspace) app: AppHandle,
}

#[async_trait]
impl PipelineStep for Step6FinalCheckPipelineStep {
    type Output = BuildStep6FinalCheckCommandResponse;

    fn name(&self) -> &'static str {
        "step_6_final_check"
    }

    fn artifact_file(&self) -> &'static str {
        STEP_06_FINAL_CHECK_FILE
    }

    fn policy(&self) -> CheckpointPolicy {
        CheckpointPolicy::SkipIfExists
    }

    fn validate(&self, output: &Self::Output) -> Result<(), String> {
        if output.task_id.trim().is_empty()
            || output.media_path.trim().is_empty()
            || output.segments.is_empty()
        {
            return Err("invalid step6 artifact".to_string());
        }
        Ok(())
    }

    async fn run(&self, _ctx: &StepContext<'_>) -> Result<Self::Output, String> {
        let task_id = self.task_id.clone();
        let app_for_progress = self.app.clone();
        let _ = report_task_stage(&app_for_progress, &task_id, TaskStage::FinalCheck, "", 0, 1);
        let result = build_step_6_final_check(BuildStep6FinalCheckCommandRequest {
            task_id: self.task_id.clone(),
            media_path: self.media_path.clone(),
            source_lang: self.source_lang.clone(),
            target_lang: self.target_lang.clone(),
            segments: self.segments.clone(),
        })
        .await?;
        let detail = if result.quality_summary.issue_count == 0 {
            "无问题".to_string()
        } else {
            format!("{}项提示", result.quality_summary.issue_count)
        };
        let _ = report_task_stage(
            &app_for_progress,
            &task_id,
            TaskStage::FinalCheck,
            detail,
            1,
            1,
        );
        Ok(result)
    }
}
