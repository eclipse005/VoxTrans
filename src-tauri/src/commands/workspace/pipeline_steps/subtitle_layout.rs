use std::sync::Arc;

use async_trait::async_trait;
use tauri::AppHandle;
use tokio::runtime::Handle;

use crate::commands::translate_step5_commands::{
    build_step_5_split_align_with_progress_and_unit_store,
};
use crate::commands::translate_types::{
    BuildStep5SplitAlignCommandRequest, BuildStep5SplitAlignCommandResponse,
    BuildTranslationSegmentCommand, TranslateTerminologyEntryCommand,
};
use crate::services::pipeline::{CheckpointPolicy, PipelineStep, StepContext};

use super::super::progress::report_task_stage;
use super::super::TaskStage;

type ProgressCallback = Arc<dyn Fn(usize, usize) + Send + Sync>;

fn subtitle_layout_progress(
    app: &AppHandle,
    task_id: &str,
    label: &'static str,
) -> ProgressCallback {
    let task_id = task_id.to_string();
    let app_for_progress = app.clone();
    Arc::new(move |current, total| {
        let detail = if total > 0 {
            format!("{current}/{total}")
        } else {
            String::new()
        };
        let report = report_task_stage(
            &app_for_progress,
            &task_id,
            TaskStage::SubtitleLayout,
            format!("{label} {detail}"),
            current as u32,
            total as u32,
        );
        // The progress callback is invoked from the LLM HTTP client thread,
        // which may already be a tokio runtime worker; plain block_on would
        // panic. Use block_in_place on multi-thread runtimes.
        match Handle::try_current() {
            Ok(handle)
                if handle.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread =>
            {
                let _ = tokio::task::block_in_place(|| handle.block_on(report));
            }
            _ => {
                let _ = tauri::async_runtime::block_on(report);
            }
        }
    })
}

fn validate_step5_artifact(
    task_id: &str,
    media_path: &str,
    has_payload: bool,
    step_name: &str,
) -> Result<(), String> {
    if task_id.trim().is_empty() || media_path.trim().is_empty() || !has_payload {
        return Err(format!("invalid {step_name} artifact"));
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub(in crate::commands::workspace) struct Step5SplitAlignPipelineStep {
    pub(in crate::commands::workspace) task_id: String,
    pub(in crate::commands::workspace) media_path: String,
    pub(in crate::commands::workspace) source_lang: String,
    pub(in crate::commands::workspace) target_lang: String,
    pub(in crate::commands::workspace) theme_summary: String,
    pub(in crate::commands::workspace) segments: Vec<BuildTranslationSegmentCommand>,
    pub(in crate::commands::workspace) terminology_entries: Vec<TranslateTerminologyEntryCommand>,
    pub(in crate::commands::workspace) subtitle_length_preset: String,
    pub(in crate::commands::workspace) translate_api_key: String,
    pub(in crate::commands::workspace) translate_base_url: String,
    pub(in crate::commands::workspace) translate_model: String,
    pub(in crate::commands::workspace) llm_concurrency: u32,
    pub(in crate::commands::workspace) app: AppHandle,
}

#[async_trait]
impl PipelineStep for Step5SplitAlignPipelineStep {
    type Output = BuildStep5SplitAlignCommandResponse;

    fn name(&self) -> &'static str {
        "step_5_split_align"
    }

    fn policy(&self) -> CheckpointPolicy {
        CheckpointPolicy::SkipIfExists
    }

    fn validate(&self, output: &Self::Output) -> Result<(), String> {
        validate_step5_artifact(
            &output.task_id,
            &output.media_path,
            !output.parents.is_empty(),
            "step5",
        )
    }

    async fn run(&self, ctx: &StepContext<'_>) -> Result<Self::Output, String> {
        let unit_store = ctx.unit_store();
        build_step_5_split_align_with_progress_and_unit_store(
            self.app.clone(),
            BuildStep5SplitAlignCommandRequest {
                task_id: self.task_id.clone(),
                media_path: self.media_path.clone(),
                source_lang: self.source_lang.clone(),
                target_lang: self.target_lang.clone(),
                theme_summary: self.theme_summary.clone(),
                segments: self.segments.clone(),
                terminology_entries: self.terminology_entries.clone(),
                subtitle_length_preset: self.subtitle_length_preset.clone(),
                translate_api_key: self.translate_api_key.clone(),
                translate_base_url: self.translate_base_url.clone(),
                translate_model: self.translate_model.clone(),
                llm_concurrency: self.llm_concurrency,
            },
            Some(subtitle_layout_progress(
                &self.app,
                &self.task_id,
                "断句对齐",
            )),
            Some(unit_store),
        )
        .await
    }
}
