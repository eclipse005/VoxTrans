use std::sync::Arc;

use async_trait::async_trait;
use tauri::AppHandle;

use crate::services::pipeline::{CheckpointPolicy, PipelineStep, StepContext};

use super::super::progress::report_task_stage;
use super::super::{
    STEP_05_01_SOURCE_SPLIT_FILE, STEP_05_02_TRANSLATION_ALIGN_FILE,
    STEP_05_03_TRANSLATION_POLISH_FILE, TaskStage,
};

#[derive(Debug, Clone)]
pub(in crate::commands::workspace) struct Step51SourceSplitPipelineStep {
    pub(in crate::commands::workspace) task_id: String,
    pub(in crate::commands::workspace) media_path: String,
    pub(in crate::commands::workspace) source_lang: String,
    pub(in crate::commands::workspace) target_lang: String,
    pub(in crate::commands::workspace) segments:
        Vec<crate::commands::translate::BuildTranslationSegmentCommand>,
    pub(in crate::commands::workspace) translate_api_key: String,
    pub(in crate::commands::workspace) translate_base_url: String,
    pub(in crate::commands::workspace) translate_model: String,
    pub(in crate::commands::workspace) llm_concurrency: u32,
    pub(in crate::commands::workspace) subtitle_max_words_per_segment: u32,
    pub(in crate::commands::workspace) subtitle_length_reference: u32,
    pub(in crate::commands::workspace) app: AppHandle,
}

#[async_trait]
impl PipelineStep for Step51SourceSplitPipelineStep {
    type Output = crate::commands::translate::BuildStep51SourceSplitCommandResponse;

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
        if output.task_id.trim().is_empty()
            || output.media_path.trim().is_empty()
            || output.parents.is_empty()
        {
            return Err("invalid step5_1 artifact".to_string());
        }
        Ok(())
    }

    async fn run(&self, _ctx: &StepContext<'_>) -> Result<Self::Output, String> {
        let task_id = self.task_id.clone();
        let app_for_progress = self.app.clone();
        let on_progress: Arc<dyn Fn(usize, usize) + Send + Sync> =
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
                    format!("原文切分 {detail}"),
                    current as u32,
                    total as u32,
                );
            });
        crate::commands::translate::build_step_5_1_source_split_with_progress(
            crate::commands::translate::BuildStep51SourceSplitCommandRequest {
                task_id: self.task_id.clone(),
                media_path: self.media_path.clone(),
                source_lang: self.source_lang.clone(),
                target_lang: self.target_lang.clone(),
                segments: self.segments.clone(),
                translate_api_key: self.translate_api_key.clone(),
                translate_base_url: self.translate_base_url.clone(),
                translate_model: self.translate_model.clone(),
                llm_concurrency: self.llm_concurrency,
                subtitle_max_words_per_segment: self.subtitle_max_words_per_segment,
                subtitle_length_reference: self.subtitle_length_reference,
            },
            Some(on_progress),
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
    pub(in crate::commands::workspace) parents:
        Vec<crate::commands::translate::Step5SplitParentCommand>,
    pub(in crate::commands::workspace) terminology_entries:
        Vec<crate::commands::translate::TranslateTerminologyEntryCommand>,
    pub(in crate::commands::workspace) translate_api_key: String,
    pub(in crate::commands::workspace) translate_base_url: String,
    pub(in crate::commands::workspace) translate_model: String,
    pub(in crate::commands::workspace) llm_concurrency: u32,
    pub(in crate::commands::workspace) app: AppHandle,
}

#[async_trait]
impl PipelineStep for Step52TranslationAlignPipelineStep {
    type Output = crate::commands::translate::BuildStep52TranslationAlignCommandResponse;

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
        if output.task_id.trim().is_empty()
            || output.media_path.trim().is_empty()
            || output.parents.is_empty()
        {
            return Err("invalid step5_2 artifact".to_string());
        }
        Ok(())
    }

    async fn run(&self, _ctx: &StepContext<'_>) -> Result<Self::Output, String> {
        let task_id = self.task_id.clone();
        let app_for_progress = self.app.clone();
        let on_progress: Arc<dyn Fn(usize, usize) + Send + Sync> =
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
                    format!("译文对齐 {detail}"),
                    current as u32,
                    total as u32,
                );
            });
        crate::commands::translate::build_step_5_2_translation_align_with_progress(
            crate::commands::translate::BuildStep52TranslationAlignCommandRequest {
                task_id: self.task_id.clone(),
                media_path: self.media_path.clone(),
                source_lang: self.source_lang.clone(),
                target_lang: self.target_lang.clone(),
                theme_summary: self.theme_summary.clone(),
                parents: self.parents.clone(),
                terminology_entries: self.terminology_entries.clone(),
                translate_api_key: self.translate_api_key.clone(),
                translate_base_url: self.translate_base_url.clone(),
                translate_model: self.translate_model.clone(),
                llm_concurrency: self.llm_concurrency,
            },
            Some(on_progress),
        )
        .await
    }
}

#[derive(Debug, Clone)]
pub(in crate::commands::workspace) struct Step53TranslationPolishPipelineStep {
    pub(in crate::commands::workspace) task_id: String,
    pub(in crate::commands::workspace) media_path: String,
    pub(in crate::commands::workspace) source_lang: String,
    pub(in crate::commands::workspace) target_lang: String,
    pub(in crate::commands::workspace) theme_summary: String,
    pub(in crate::commands::workspace) parents:
        Vec<crate::commands::translate::Step5AlignedParentCommand>,
    pub(in crate::commands::workspace) terminology_entries:
        Vec<crate::commands::translate::TranslateTerminologyEntryCommand>,
    pub(in crate::commands::workspace) translate_api_key: String,
    pub(in crate::commands::workspace) translate_base_url: String,
    pub(in crate::commands::workspace) translate_model: String,
    pub(in crate::commands::workspace) llm_concurrency: u32,
    pub(in crate::commands::workspace) subtitle_length_reference: u32,
    pub(in crate::commands::workspace) app: AppHandle,
}

#[async_trait]
impl PipelineStep for Step53TranslationPolishPipelineStep {
    type Output = crate::commands::translate::BuildStep53TranslationPolishCommandResponse;

    fn name(&self) -> &'static str {
        "step_5_3_translation_polish"
    }

    fn artifact_file(&self) -> &'static str {
        STEP_05_03_TRANSLATION_POLISH_FILE
    }

    fn policy(&self) -> CheckpointPolicy {
        CheckpointPolicy::SkipIfExists
    }

    fn validate(&self, output: &Self::Output) -> Result<(), String> {
        if output.task_id.trim().is_empty()
            || output.media_path.trim().is_empty()
            || output.segments.is_empty()
        {
            return Err("invalid step5_3 artifact".to_string());
        }
        Ok(())
    }

    async fn run(&self, _ctx: &StepContext<'_>) -> Result<Self::Output, String> {
        let task_id = self.task_id.clone();
        let app_for_progress = self.app.clone();
        let on_progress: Arc<dyn Fn(usize, usize) + Send + Sync> =
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
                    format!("译文润色 {detail}"),
                    current as u32,
                    total as u32,
                );
            });
        crate::commands::translate::build_step_5_3_translation_polish_with_progress(
            crate::commands::translate::BuildStep53TranslationPolishCommandRequest {
                task_id: self.task_id.clone(),
                media_path: self.media_path.clone(),
                source_lang: self.source_lang.clone(),
                target_lang: self.target_lang.clone(),
                theme_summary: self.theme_summary.clone(),
                terminology_entries: self.terminology_entries.clone(),
                parents: self.parents.clone(),
                translate_api_key: self.translate_api_key.clone(),
                translate_base_url: self.translate_base_url.clone(),
                translate_model: self.translate_model.clone(),
                llm_concurrency: self.llm_concurrency,
                subtitle_length_reference: self.subtitle_length_reference,
                batch_size: 20,
            },
            Some(on_progress),
        )
        .await
    }
}
