use std::sync::Arc;

use async_trait::async_trait;
use tauri::AppHandle;

use crate::commands::translate_step5_commands::{
    build_step_5_1_source_split_with_progress, build_step_5_2_translation_align_with_progress,
};
use crate::commands::translate_types::{
    BuildStep51SourceSplitCommandRequest, BuildStep51SourceSplitCommandResponse,
    BuildStep52TranslationAlignCommandRequest, BuildStep52TranslationAlignCommandResponse,
    BuildTranslationSegmentCommand, Step5SplitParentCommand, TranslateTerminologyEntryCommand,
};
use crate::services::pipeline::{CheckpointPolicy, PipelineStep, StepContext};

use super::super::progress::report_task_stage;
use super::super::{STEP_05_01_SOURCE_SPLIT_FILE, STEP_05_02_TRANSLATION_ALIGN_FILE, TaskStage};

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
        let _ = report_task_stage(
            &app_for_progress,
            &task_id,
            TaskStage::SubtitleLayout,
            format!("{label} {detail}"),
            current as u32,
            total as u32,
        );
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
pub(in crate::commands::workspace) struct Step51SourceSplitPipelineStep {
    pub(in crate::commands::workspace) task_id: String,
    pub(in crate::commands::workspace) media_path: String,
    pub(in crate::commands::workspace) source_lang: String,
    pub(in crate::commands::workspace) target_lang: String,
    pub(in crate::commands::workspace) segments: Vec<BuildTranslationSegmentCommand>,
    pub(in crate::commands::workspace) translate_api_key: String,
    pub(in crate::commands::workspace) translate_base_url: String,
    pub(in crate::commands::workspace) translate_model: String,
    pub(in crate::commands::workspace) llm_concurrency: u32,
    pub(in crate::commands::workspace) subtitle_length_preset: String,
    pub(in crate::commands::workspace) app: AppHandle,
}

#[async_trait]
impl PipelineStep for Step51SourceSplitPipelineStep {
    type Output = BuildStep51SourceSplitCommandResponse;

    fn name(&self) -> &'static str {
        "step_5_1_source_split"
    }

    fn artifact_file(&self) -> &'static str {
        STEP_05_01_SOURCE_SPLIT_FILE
    }

    fn policy(&self) -> CheckpointPolicy {
        CheckpointPolicy::SkipIfExists
    }

    fn validate(&self, output: &Self::Output) -> Result<(), String> {
        validate_step5_artifact(
            &output.task_id,
            &output.media_path,
            !output.parents.is_empty(),
            "step5_1",
        )
    }

    async fn run(&self, _ctx: &StepContext<'_>) -> Result<Self::Output, String> {
        build_step_5_1_source_split_with_progress(
            BuildStep51SourceSplitCommandRequest {
                task_id: self.task_id.clone(),
                media_path: self.media_path.clone(),
                source_lang: self.source_lang.clone(),
                target_lang: self.target_lang.clone(),
                segments: self.segments.clone(),
                translate_api_key: self.translate_api_key.clone(),
                translate_base_url: self.translate_base_url.clone(),
                translate_model: self.translate_model.clone(),
                llm_concurrency: self.llm_concurrency,
                subtitle_length_preset: self.subtitle_length_preset.clone(),
            },
            Some(subtitle_layout_progress(
                &self.app,
                &self.task_id,
                "原文切分",
            )),
        )
        .await
    }
}

#[derive(Debug, Clone)]
pub(in crate::commands::workspace) struct Step52TranslationAlignPipelineStep {
    pub(in crate::commands::workspace) task_id: String,
    pub(in crate::commands::workspace) media_path: String,
    pub(in crate::commands::workspace) source_lang: String,
    pub(in crate::commands::workspace) target_lang: String,
    pub(in crate::commands::workspace) theme_summary: String,
    pub(in crate::commands::workspace) parents: Vec<Step5SplitParentCommand>,
    pub(in crate::commands::workspace) terminology_entries: Vec<TranslateTerminologyEntryCommand>,
    pub(in crate::commands::workspace) subtitle_length_preset: String,
    pub(in crate::commands::workspace) translate_api_key: String,
    pub(in crate::commands::workspace) translate_base_url: String,
    pub(in crate::commands::workspace) translate_model: String,
    pub(in crate::commands::workspace) llm_concurrency: u32,
    pub(in crate::commands::workspace) app: AppHandle,
}

#[async_trait]
impl PipelineStep for Step52TranslationAlignPipelineStep {
    type Output = BuildStep52TranslationAlignCommandResponse;

    fn name(&self) -> &'static str {
        "step_5_2_translation_align"
    }

    fn artifact_file(&self) -> &'static str {
        STEP_05_02_TRANSLATION_ALIGN_FILE
    }

    fn policy(&self) -> CheckpointPolicy {
        CheckpointPolicy::SkipIfExists
    }

    fn validate(&self, output: &Self::Output) -> Result<(), String> {
        validate_step5_artifact(
            &output.task_id,
            &output.media_path,
            !output.parents.is_empty(),
            "step5_2",
        )
    }

    async fn run(&self, _ctx: &StepContext<'_>) -> Result<Self::Output, String> {
        build_step_5_2_translation_align_with_progress(
            BuildStep52TranslationAlignCommandRequest {
                task_id: self.task_id.clone(),
                media_path: self.media_path.clone(),
                source_lang: self.source_lang.clone(),
                target_lang: self.target_lang.clone(),
                theme_summary: self.theme_summary.clone(),
                parents: self.parents.clone(),
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
                "译文对齐",
            )),
        )
        .await
    }
}
