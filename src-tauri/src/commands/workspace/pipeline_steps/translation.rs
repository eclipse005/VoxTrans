use std::sync::Arc;

use async_trait::async_trait;
use tauri::AppHandle;
use tokio::runtime::Handle;

use crate::commands::translate_terminology::build_terminology_layer;
use crate::commands::translate_translation::build_translation_layer_with_progress_and_unit_store;
use crate::commands::translate_types::{
    BuildTerminologyLayerCommandRequest, BuildTerminologyLayerCommandResponse,
    BuildTranslationLayerCommandRequest, BuildTranslationLayerCommandResponse,
    SourceSegmentForTerminologyCommand, TranslateTerminologyEntryCommand,
};
use crate::domain::task::adapters::workspace_subtitle_segments_from_translation_outputs;
use crate::services::pipeline::{CheckpointPolicy, PipelineStep, StepContext};
use crate::services::translation::TranslationProgress;

use super::super::preview::update_subtitle_preview;
use super::super::progress::report_task_stage;
use super::super::TaskStage;

#[derive(Debug, Clone)]
pub(in crate::commands::workspace) struct Step3TerminologyPipelineStep {
    pub(in crate::commands::workspace) task_id: String,
    pub(in crate::commands::workspace) media_path: String,
    pub(in crate::commands::workspace) source_lang: String,
    pub(in crate::commands::workspace) target_lang: String,
    pub(in crate::commands::workspace) segments: Vec<SourceSegmentForTerminologyCommand>,
    pub(in crate::commands::workspace) translate_api_key: String,
    pub(in crate::commands::workspace) translate_base_url: String,
    pub(in crate::commands::workspace) translate_model: String,
    pub(in crate::commands::workspace) llm_concurrency: u32,
    pub(in crate::commands::workspace) terminology_entries: Vec<TranslateTerminologyEntryCommand>,
    pub(in crate::commands::workspace) app: AppHandle,
}

#[async_trait]
impl PipelineStep for Step3TerminologyPipelineStep {
    type Output = BuildTerminologyLayerCommandResponse;

    fn name(&self) -> &'static str {
        "step_03_terminology"
    }

    fn policy(&self) -> CheckpointPolicy {
        CheckpointPolicy::SkipIfExists
    }

    fn validate(&self, output: &Self::Output) -> Result<(), String> {
        if output.task_id.trim().is_empty() || output.media_path.trim().is_empty() {
            return Err("invalid step3 artifact".to_string());
        }
        Ok(())
    }

    async fn run(&self, _ctx: &StepContext<'_>) -> Result<Self::Output, String> {
        build_terminology_layer(
            self.app.clone(),
            BuildTerminologyLayerCommandRequest {
                task_id: self.task_id.clone(),
                media_path: self.media_path.clone(),
                source_lang: self.source_lang.clone(),
                target_lang: self.target_lang.clone(),
                segments: self.segments.clone(),
                translate_api_key: self.translate_api_key.clone(),
                translate_base_url: self.translate_base_url.clone(),
                translate_model: self.translate_model.clone(),
                llm_concurrency: self.llm_concurrency,
                terminology_entries: self.terminology_entries.clone(),
            },
        )
        .await
    }
}

/// Run a future from the progress callback, which fires on the LLM HTTP
/// client thread. That thread may itself be a tokio multi-thread runtime
/// worker, where a plain `block_on` would panic — `block_in_place` is the
/// safe path there. Falls back to `async_runtime::block_on` otherwise.
fn block_on_runtime_worker<F>(fut: F)
where
    F: std::future::Future,
{
    match Handle::try_current() {
        Ok(handle)
            if handle.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread =>
        {
            let _ = tokio::task::block_in_place(|| handle.block_on(fut));
        }
        _ => {
            let _ = tauri::async_runtime::block_on(fut);
        }
    }
}

#[derive(Debug, Clone)]
pub(in crate::commands::workspace) struct Step4TranslationPipelineStep {
    pub(in crate::commands::workspace) task_id: String,
    pub(in crate::commands::workspace) media_path: String,
    pub(in crate::commands::workspace) source_lang: String,
    pub(in crate::commands::workspace) target_lang: String,
    pub(in crate::commands::workspace) segments: Vec<SourceSegmentForTerminologyCommand>,
    pub(in crate::commands::workspace) theme_summary: String,
    pub(in crate::commands::workspace) terminology_entries: Vec<TranslateTerminologyEntryCommand>,
    pub(in crate::commands::workspace) translate_api_key: String,
    pub(in crate::commands::workspace) translate_base_url: String,
    pub(in crate::commands::workspace) translate_model: String,
    pub(in crate::commands::workspace) llm_concurrency: u32,
    pub(in crate::commands::workspace) enable_vision_assist: bool,
    pub(in crate::commands::workspace) app: AppHandle,
}

#[async_trait]
impl PipelineStep for Step4TranslationPipelineStep {
    type Output = BuildTranslationLayerCommandResponse;

    fn name(&self) -> &'static str {
        "step_04_translation"
    }

    fn policy(&self) -> CheckpointPolicy {
        CheckpointPolicy::SkipIfExists
    }

    fn validate(&self, output: &Self::Output) -> Result<(), String> {
        if output.task_id.trim().is_empty() || output.media_path.trim().is_empty() {
            return Err("invalid step4 artifact".to_string());
        }
        Ok(())
    }

    async fn run(&self, ctx: &StepContext<'_>) -> Result<Self::Output, String> {
        let task_id = self.task_id.clone();
        let app_for_progress = self.app.clone();
        // Precompute the source text once so each incremental preview emit
        // can pass it to update_subtitle_preview without rebuilding it.
        let source_text_for_progress = self
            .segments
            .iter()
            .map(|segment| segment.segment.trim())
            .filter(|segment| !segment.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        let on_progress: Arc<dyn Fn(TranslationProgress) + Send + Sync> =
            Arc::new(move |progress: TranslationProgress| {
                let current = progress.done;
                let total = progress.total;
                let detail = if total > 0 {
                    format!("{current}/{total}")
                } else {
                    String::new()
                };
                let report = report_task_stage(
                    &app_for_progress,
                    &task_id,
                    TaskStage::Translating,
                    detail,
                    current as u32,
                    total as u32,
                );
                block_on_runtime_worker(report);

                // Stream the incremental translation snapshot to the subtitle
                // editor as a read-only preview. Untranslated rows keep an
                // empty translation; the final full result overwrites this
                // once translation completes.
                let segments =
                    workspace_subtitle_segments_from_translation_outputs(&progress.partial_outputs);
                let preview = update_subtitle_preview(
                    &app_for_progress,
                    &task_id,
                    &source_text_for_progress,
                    segments,
                );
                block_on_runtime_worker(preview);
            });

        let unit_store = ctx.unit_store();

        build_translation_layer_with_progress_and_unit_store(
            self.app.clone(),
            BuildTranslationLayerCommandRequest {
                task_id: self.task_id.clone(),
                media_path: self.media_path.clone(),
                source_lang: self.source_lang.clone(),
                target_lang: self.target_lang.clone(),
                segments: self.segments.clone(),
                theme_summary: self.theme_summary.clone(),
                terminology_entries: self.terminology_entries.clone(),
                translate_api_key: self.translate_api_key.clone(),
                translate_base_url: self.translate_base_url.clone(),
                translate_model: self.translate_model.clone(),
                llm_concurrency: self.llm_concurrency,
                batch_size: 20,
            },
            Some(on_progress),
            Some(unit_store),
            self.enable_vision_assist,
        )
        .await
    }
}
