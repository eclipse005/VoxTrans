use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Emitter};

use crate::services::pipeline::{
    CheckpointPolicy, PipelineStep, StepContext, StepSource, execute_step,
};
use crate::services::task_log::{TaskLogger, event as task_log_event};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceQueueItem {
    pub id: String,
    pub path: String,
    pub name: String,
    pub media_kind: String,
    pub size_bytes: u64,
    pub transcribe_status: String,
    pub task_progress: WorkspaceTaskProgressState,
    pub transcribe_error: String,
    pub result_text: String,
    pub result_srt: String,
    pub subtitle_segments_json: String,
    #[serde(default)]
    pub llm_total_tokens: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceTaskStageState {
    #[serde(default)]
    pub code: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub order: u32,
    #[serde(default)]
    pub detail: String,
    #[serde(default)]
    pub current: u32,
    #[serde(default)]
    pub total: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceTaskProgressState {
    #[serde(default)]
    pub stage: WorkspaceTaskStageState,
}

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
enum TaskStage {
    Downloading,
    Preparing,
    Recognizing,
    Segmenting,
    Summarizing,
    Terminology,
    Translating,
    SubtitleLayout,
    FinalCheck,
}

impl TaskStage {
    fn code(self) -> &'static str {
        match self {
            TaskStage::Downloading => "downloading",
            TaskStage::Preparing => "preparing",
            TaskStage::Recognizing => "recognizing",
            TaskStage::Segmenting => "segmenting",
            TaskStage::Summarizing => "summarizing",
            TaskStage::Terminology => "terminology",
            TaskStage::Translating => "translating",
            TaskStage::SubtitleLayout => "subtitleLayout",
            TaskStage::FinalCheck => "finalCheck",
        }
    }

    fn label(self) -> &'static str {
        match self {
            TaskStage::Downloading => "下载中",
            TaskStage::Preparing => "准备中",
            TaskStage::Recognizing => "语音识别中",
            TaskStage::Segmenting => "AI断句中",
            TaskStage::Summarizing => "总结中",
            TaskStage::Terminology => "术语提取中",
            TaskStage::Translating => "翻译中",
            TaskStage::SubtitleLayout => "",
            TaskStage::FinalCheck => "本地最终检查中",
        }
    }

    fn order(self) -> u32 {
        match self {
            TaskStage::Downloading => 10,
            TaskStage::Preparing => 20,
            TaskStage::Recognizing => 30,
            TaskStage::Segmenting => 40,
            TaskStage::Summarizing => 50,
            TaskStage::Terminology => 60,
            TaskStage::Translating => 70,
            TaskStage::SubtitleLayout => 80,
            TaskStage::FinalCheck => 90,
        }
    }
}

#[derive(Debug, Clone)]
struct WorkspaceTaskRecord {
    item: WorkspaceQueueItem,
    intent: String,
    source_lang: String,
    target_lang: String,
    max_retries: u32,
    settings_snapshot: Value,
}

#[derive(Debug, Default)]
struct WorkspaceStore {
    tasks: Vec<WorkspaceTaskRecord>,
}

static WORKSPACE_STORE: OnceLock<Mutex<WorkspaceStore>> = OnceLock::new();
static WORKSPACE_HYDRATED: OnceLock<Mutex<bool>> = OnceLock::new();
const TASK_META_FILE_NAME: &str = "task_meta.json";
const STEP_01_ASR_FILE: &str = "step_01_asr.json";
const STEP_01_5_HOTWORDS_FILE: &str = "step_01_5_hotwords.json";
const STEP_02_SEGMENTS_FILE: &str = "step_02_segments.json";
const STEP_03_TERMINOLOGY_FILE: &str = "step_03_terminology.json";
const STEP_04_TRANSLATION_FILE: &str = "step_04_translation.json";
const STEP_05_01_SOURCE_SPLIT_FILE: &str = "step_05_01_source_split.json";
const STEP_05_02_TRANSLATION_ALIGN_FILE: &str = "step_05_02_translation_align.json";
const STEP_05_03_TRANSLATION_POLISH_FILE: &str = "step_05_03_translation_polish.json";
const STEP_06_FINAL_CHECK_FILE: &str = "step_06_final_check.json";

fn workspace_store() -> &'static Mutex<WorkspaceStore> {
    WORKSPACE_STORE.get_or_init(|| Mutex::new(WorkspaceStore::default()))
}

fn lock_workspace_store() -> Result<std::sync::MutexGuard<'static, WorkspaceStore>, String> {
    workspace_store()
        .lock()
        .map_err(|_| "workspace store lock poisoned".to_string())
}

fn lock_workspace_hydrated() -> Result<std::sync::MutexGuard<'static, bool>, String> {
    WORKSPACE_HYDRATED
        .get_or_init(|| Mutex::new(false))
        .lock()
        .map_err(|_| "workspace hydrated lock poisoned".to_string())
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteTasksCommandRequest {
    #[serde(default)]
    pub task_id: Option<String>,
    #[serde(default)]
    pub media_path: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterTaskUploadCommandRequest {
    pub id: String,
    pub media_path: String,
    pub name: String,
    pub media_kind: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadWorkspaceTaskCommandRequest {
    pub task_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnqueueTaskRunCommandRequest {
    pub id: String,
    pub media_path: String,
    pub name: String,
    pub media_kind: String,
    pub size_bytes: u64,
    pub intent: String,
    #[serde(default)]
    pub source_lang: Option<String>,
    #[serde(default)]
    pub target_lang: Option<String>,
    #[serde(default)]
    pub max_retries: Option<u32>,
    #[serde(default)]
    pub settings_snapshot: Option<Value>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecuteTaskRunCommandRequest {
    pub task_id: String,
    #[serde(default)]
    pub intent: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecuteTaskBatchCommandRequest {
    pub items: Vec<ExecuteTaskRunCommandRequest>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnqueueAndExecuteTaskBatchCommandRequest {
    pub items: Vec<EnqueueTaskRunCommandRequest>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceStateResponse {
    pub queue: Vec<WorkspaceQueueItem>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceTaskResponse {
    pub item: WorkspaceQueueItem,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecuteTaskBatchFailedItem {
    pub task_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecuteTaskBatchCommandResponse {
    pub succeeded_task_ids: Vec<String>,
    pub failed: Vec<ExecuteTaskBatchFailedItem>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct SettingsSnapshotInput {
    #[serde(default)]
    provider: Option<String>,
    #[serde(default)]
    chunk_target_seconds: Option<u32>,
    #[serde(default)]
    translate_api_key: Option<String>,
    #[serde(default)]
    translate_base_url: Option<String>,
    #[serde(default)]
    translate_model: Option<String>,
    #[serde(default)]
    llm_concurrency: Option<u32>,
    #[serde(default)]
    subtitle_max_words_per_segment: Option<u32>,
    #[serde(default)]
    subtitle_length_reference: Option<u32>,
    #[serde(default)]
    terminology_groups: Option<Vec<SettingsSnapshotTerminologyGroup>>,
    #[serde(default)]
    enable_terminology: Option<bool>,
    #[serde(default)]
    hotword_groups: Option<Vec<SettingsSnapshotHotwordGroup>>,
    #[serde(default)]
    enable_hotwords: Option<bool>,
    #[serde(default)]
    enable_subtitle_beautify: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct SettingsSnapshotTerminologyGroup {
    #[serde(default)]
    terms: Vec<SettingsSnapshotTerminologyTerm>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct SettingsSnapshotTerminologyTerm {
    #[serde(default)]
    origin: String,
    #[serde(default)]
    target: String,
    #[serde(default)]
    note: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct SettingsSnapshotHotwordGroup {
    #[serde(default)]
    terms: Vec<SettingsSnapshotHotwordTerm>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct SettingsSnapshotHotwordTerm {
    #[serde(default)]
    word: String,
    #[serde(default)]
    aliases: Vec<String>,
    #[serde(default)]
    lang: String,
    #[serde(default)]
    note: String,
}

#[derive(Debug, Clone)]
struct PipelineRuntimeSettings {
    provider: String,
    chunk_target_seconds: u32,
    translate_api_key: String,
    translate_base_url: String,
    translate_model: String,
    llm_concurrency: u32,
    subtitle_max_words_per_segment: u32,
    subtitle_length_reference: u32,
    terminology_entries: Vec<crate::commands::translate::TranslateTerminologyEntryCommand>,
    hotword_entries: Vec<crate::services::hotwords::HotwordEntry>,
    enable_hotwords: bool,
    enable_subtitle_beautify: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Step1AsrArtifact {
    task_id: String,
    media_path: String,
    source_lang: String,
    words: Vec<crate::commands::transcription::WordTokenCommandDto>,
}

#[derive(Debug, Clone)]
struct Step1AsrPipelineStep {
    task_id: String,
    media_path: String,
    source_lang: String,
    provider: String,
    chunk_target_seconds: u32,
    app: AppHandle,
}

#[async_trait]
impl PipelineStep for Step1AsrPipelineStep {
    type Output = Step1AsrArtifact;

    fn name(&self) -> &'static str {
        "step_01_asr"
    }

    fn artifact_file(&self) -> &'static str {
        STEP_01_ASR_FILE
    }

    fn policy(&self) -> CheckpointPolicy {
        CheckpointPolicy::SkipIfExists
    }

    fn validate(&self, output: &Self::Output) -> Result<(), String> {
        if output.task_id.trim().is_empty()
            || output.media_path.trim().is_empty()
            || output.words.is_empty()
        {
            return Err("invalid step1 artifact".to_string());
        }
        Ok(())
    }

    async fn run(&self, _ctx: &StepContext<'_>) -> Result<Self::Output, String> {
        let task_id_owned = self.task_id.clone();
        let app_for_progress = self.app.clone();
        let media_path = self.media_path.clone();
        let provider = self.provider.clone();
        let chunk_target_seconds = self.chunk_target_seconds;
        let transcribe_request = crate::services::transcribe::TranscribeRequest {
            task_id: task_id_owned.clone(),
            audio_path: media_path.clone(),
            provider,
            chunk_target_seconds,
            model_dir: None,
        };
        let transcribe_response = tauri::async_runtime::spawn_blocking(move || {
            crate::services::transcribe::transcribe_blocking(
                transcribe_request,
                move |current, total| {
                    let _ = report_task_stage(
                        &app_for_progress,
                        &task_id_owned,
                        TaskStage::Recognizing,
                        format!("{current}/{total}"),
                        current as u32,
                        total as u32,
                    );
                },
            )
        })
        .await
        .map_err(|err| err.to_string())??;

        let words = transcribe_response
            .words
            .iter()
            .map(|word| crate::commands::transcription::WordTokenCommandDto {
                start: word.start,
                end: word.end,
                word: word.word.clone(),
            })
            .collect::<Vec<_>>();
        Ok(Step1AsrArtifact {
            task_id: self.task_id.clone(),
            media_path: self.media_path.clone(),
            source_lang: self.source_lang.clone(),
            words,
        })
    }
}

#[derive(Debug, Clone)]
struct Step15HotwordsPipelineStep {
    task_id: String,
    media_path: String,
    source_lang: String,
    words: Vec<crate::commands::transcription::WordTokenCommandDto>,
    hotwords: Vec<crate::services::hotwords::HotwordEntry>,
    enabled: bool,
    translate_api_key: String,
    translate_base_url: String,
    translate_model: String,
}

#[async_trait]
impl PipelineStep for Step15HotwordsPipelineStep {
    type Output = crate::services::hotwords::BuildHotwordCorrectionResponse;

    fn name(&self) -> &'static str {
        "step_01_5_hotwords"
    }

    fn artifact_file(&self) -> &'static str {
        STEP_01_5_HOTWORDS_FILE
    }

    fn policy(&self) -> CheckpointPolicy {
        CheckpointPolicy::SkipIfExists
    }

    fn validate(&self, output: &Self::Output) -> Result<(), String> {
        if output.task_id.trim().is_empty()
            || output.media_path.trim().is_empty()
            || output.words.is_empty()
        {
            return Err("invalid step1.5 artifact".to_string());
        }
        Ok(())
    }

    async fn run(&self, _ctx: &StepContext<'_>) -> Result<Self::Output, String> {
        let words = self
            .words
            .iter()
            .map(|word| crate::services::transcribe::WordTokenDto {
                start: word.start,
                end: word.end,
                word: word.word.clone(),
            })
            .collect::<Vec<_>>();

        Ok(crate::services::hotwords::build_hotword_correction_async(
            crate::services::hotwords::BuildHotwordCorrectionRequest {
                task_id: self.task_id.clone(),
                media_path: self.media_path.clone(),
                source_lang: self.source_lang.clone(),
                words,
                hotwords: self.hotwords.clone(),
                enabled: self.enabled,
                translate_api_key: self.translate_api_key.clone(),
                translate_base_url: self.translate_base_url.clone(),
                translate_model: self.translate_model.clone(),
            },
        )
        .await)
    }
}

#[derive(Debug, Clone)]
struct Step2SegmentsPipelineStep {
    task_id: String,
    media_path: String,
    source_lang: String,
    words: Vec<crate::commands::transcription::WordTokenCommandDto>,
    subtitle_max_words_per_segment: u32,
    translate_api_key: String,
    translate_base_url: String,
    translate_model: String,
    llm_concurrency: u32,
    app: AppHandle,
}

#[async_trait]
impl PipelineStep for Step2SegmentsPipelineStep {
    type Output = Vec<crate::commands::transcription::GroupedSentenceSegmentCommandDto>;

    fn name(&self) -> &'static str {
        "step_02_segments"
    }

    fn artifact_file(&self) -> &'static str {
        STEP_02_SEGMENTS_FILE
    }

    fn policy(&self) -> CheckpointPolicy {
        CheckpointPolicy::SkipIfExists
    }

    fn validate(&self, output: &Self::Output) -> Result<(), String> {
        if output.is_empty() {
            return Err("invalid step2 artifact".to_string());
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
                    TaskStage::Segmenting,
                    detail,
                    current as u32,
                    total as u32,
                );
            });
        let step2_request = crate::commands::transcription::BuildSourceSentencesCommandRequest {
            task_id: self.task_id.clone(),
            audio_path: self.media_path.clone(),
            source_lang: self.source_lang.clone(),
            words: self.words.clone(),
            subtitle_max_words_per_segment: self.subtitle_max_words_per_segment,
            translate_api_key: self.translate_api_key.clone(),
            translate_base_url: self.translate_base_url.clone(),
            translate_model: self.translate_model.clone(),
            llm_concurrency: self.llm_concurrency,
        };
        let step2_response = crate::commands::transcription::build_source_sentences_with_progress(
            step2_request,
            Some(on_progress),
        )
        .await?;
        Ok(step2_response.segments)
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceSubtitleWord {
    start_ms: u64,
    end_ms: u64,
    word: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceSubtitleSegment {
    start_ms: u64,
    end_ms: u64,
    source_text: String,
    translated_text: String,
    source_words: Vec<WorkspaceSubtitleWord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceTaskMetaArtifact {
    #[serde(default = "task_meta_version")]
    version: u32,
    item: WorkspaceQueueItem,
    #[serde(default)]
    intent: String,
    #[serde(default)]
    source_lang: String,
    #[serde(default)]
    target_lang: String,
    #[serde(default)]
    max_retries: u32,
    #[serde(default)]
    settings_snapshot: Value,
    #[serde(default)]
    updated_at_ms: u64,
}

#[derive(Debug, Clone)]
struct Step3TerminologyPipelineStep {
    task_id: String,
    media_path: String,
    source_lang: String,
    target_lang: String,
    segments: Vec<crate::commands::translate::SourceSegmentForTerminologyCommand>,
    translate_api_key: String,
    translate_base_url: String,
    translate_model: String,
    llm_concurrency: u32,
    terminology_entries: Vec<crate::commands::translate::TranslateTerminologyEntryCommand>,
}

#[async_trait]
impl PipelineStep for Step3TerminologyPipelineStep {
    type Output = crate::commands::translate::BuildTerminologyLayerCommandResponse;

    fn name(&self) -> &'static str {
        "step_03_terminology"
    }

    fn artifact_file(&self) -> &'static str {
        STEP_03_TERMINOLOGY_FILE
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
        crate::commands::translate::build_terminology_layer(
            crate::commands::translate::BuildTerminologyLayerCommandRequest {
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

#[derive(Debug, Clone)]
struct Step4TranslationPipelineStep {
    task_id: String,
    media_path: String,
    source_lang: String,
    target_lang: String,
    segments: Vec<crate::commands::translate::SourceSegmentForTerminologyCommand>,
    theme_summary: String,
    terminology_entries: Vec<crate::commands::translate::TranslateTerminologyEntryCommand>,
    translate_api_key: String,
    translate_base_url: String,
    translate_model: String,
    llm_concurrency: u32,
    app: AppHandle,
}

#[async_trait]
impl PipelineStep for Step4TranslationPipelineStep {
    type Output = crate::commands::translate::BuildTranslationLayerCommandResponse;

    fn name(&self) -> &'static str {
        "step_04_translation"
    }

    fn artifact_file(&self) -> &'static str {
        STEP_04_TRANSLATION_FILE
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
                    TaskStage::Translating,
                    detail,
                    current as u32,
                    total as u32,
                );
            });
        crate::commands::translate::build_translation_layer_with_progress(
            crate::commands::translate::BuildTranslationLayerCommandRequest {
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
        )
        .await
    }
}

#[derive(Debug, Clone)]
struct Step51SourceSplitPipelineStep {
    task_id: String,
    media_path: String,
    source_lang: String,
    target_lang: String,
    segments: Vec<crate::commands::translate::BuildTranslationSegmentCommand>,
    translate_api_key: String,
    translate_base_url: String,
    translate_model: String,
    llm_concurrency: u32,
    subtitle_max_words_per_segment: u32,
    subtitle_length_reference: u32,
    app: AppHandle,
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
struct Step52TranslationAlignPipelineStep {
    task_id: String,
    media_path: String,
    source_lang: String,
    target_lang: String,
    theme_summary: String,
    parents: Vec<crate::commands::translate::Step5SplitParentCommand>,
    terminology_entries: Vec<crate::commands::translate::TranslateTerminologyEntryCommand>,
    translate_api_key: String,
    translate_base_url: String,
    translate_model: String,
    llm_concurrency: u32,
    app: AppHandle,
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
struct Step53TranslationPolishPipelineStep {
    task_id: String,
    media_path: String,
    source_lang: String,
    target_lang: String,
    theme_summary: String,
    parents: Vec<crate::commands::translate::Step5AlignedParentCommand>,
    terminology_entries: Vec<crate::commands::translate::TranslateTerminologyEntryCommand>,
    translate_api_key: String,
    translate_base_url: String,
    translate_model: String,
    llm_concurrency: u32,
    subtitle_length_reference: u32,
    app: AppHandle,
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

#[derive(Debug, Clone)]
struct Step6FinalCheckPipelineStep {
    task_id: String,
    media_path: String,
    source_lang: String,
    target_lang: String,
    segments: Vec<crate::commands::translate::BuildTranslationSegmentCommand>,
    translate_api_key: String,
    translate_base_url: String,
    translate_model: String,
    llm_concurrency: u32,
    app: AppHandle,
}

#[async_trait]
impl PipelineStep for Step6FinalCheckPipelineStep {
    type Output = crate::commands::translate::BuildStep6FinalCheckCommandResponse;

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
        let result = crate::commands::translate::build_step_6_final_check(
            crate::commands::translate::BuildStep6FinalCheckCommandRequest {
                task_id: self.task_id.clone(),
                media_path: self.media_path.clone(),
                source_lang: self.source_lang.clone(),
                target_lang: self.target_lang.clone(),
                segments: self.segments.clone(),
                translate_api_key: self.translate_api_key.clone(),
                translate_base_url: self.translate_base_url.clone(),
                translate_model: self.translate_model.clone(),
                llm_concurrency: self.llm_concurrency,
            },
        )
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

#[tauri::command]
pub async fn load_workspace_state() -> Result<WorkspaceStateResponse, String> {
    ensure_workspace_hydrated_from_disk()?;
    let store = lock_workspace_store()?;
    Ok(WorkspaceStateResponse {
        queue: store.tasks.iter().map(|task| task.item.clone()).collect(),
    })
}

#[tauri::command]
pub async fn load_workspace_task(
    request: LoadWorkspaceTaskCommandRequest,
) -> Result<WorkspaceTaskResponse, String> {
    ensure_workspace_hydrated_from_disk()?;
    let task_id = request.task_id.trim();
    if task_id.is_empty() {
        return Err("taskId is required".to_string());
    }
    let store = lock_workspace_store()?;
    let Some(task) = store.tasks.iter().find(|entry| entry.item.id == task_id) else {
        return Err(format!("task not found: {task_id}"));
    };
    Ok(WorkspaceTaskResponse {
        item: task.item.clone(),
    })
}

#[tauri::command]
pub async fn register_task_upload(request: RegisterTaskUploadCommandRequest) -> Result<(), String> {
    ensure_workspace_hydrated_from_disk()?;
    let id = request.id.trim();
    let media_path = request.media_path.trim();
    if id.is_empty() {
        return Err("id is required".to_string());
    }
    if media_path.is_empty() {
        return Err("mediaPath is required".to_string());
    }

    {
        let mut store = lock_workspace_store()?;
        if let Some(existing) = find_task_mut(&mut store, id) {
            existing.item.path = media_path.to_string();
            existing.item.name = request.name;
            existing.item.media_kind = normalize_media_kind(&request.media_kind).to_string();
            existing.item.size_bytes = request.size_bytes;
            persist_task_meta(existing)?;
        } else {
            let record = WorkspaceTaskRecord {
                item: WorkspaceQueueItem {
                    id: id.to_string(),
                    path: media_path.to_string(),
                    name: request.name,
                    media_kind: normalize_media_kind(&request.media_kind).to_string(),
                    size_bytes: request.size_bytes,
                    transcribe_status: "pending".to_string(),
                    task_progress: WorkspaceTaskProgressState::default(),
                    transcribe_error: String::new(),
                    result_text: String::new(),
                    result_srt: String::new(),
                    subtitle_segments_json: "[]".to_string(),
                    llm_total_tokens: 0,
                },
                intent: "TRANSCRIBE".to_string(),
                source_lang: "auto".to_string(),
                target_lang: "zh-CN".to_string(),
                max_retries: 0,
                settings_snapshot: Value::Null,
            };
            persist_task_meta(&record)?;
            store.tasks.push(record);
        }
    }

    Ok(())
}

#[tauri::command]
pub async fn delete_tasks(request: DeleteTasksCommandRequest) -> Result<(), String> {
    ensure_workspace_hydrated_from_disk()?;
    let task_id = request
        .task_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let media_path = request
        .media_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);

    let mut store = lock_workspace_store()?;
    if task_id.is_none() && media_path.is_none() {
        for task in &store.tasks {
            remove_task_meta(&task.item);
        }
        store.tasks.clear();
        return Ok(());
    }

    let removed = store
        .tasks
        .iter()
        .filter(|task| {
            let task_match = task_id
                .as_deref()
                .map(|id| task.item.id == id)
                .unwrap_or(false);
            let media_match = media_path
                .as_deref()
                .map(|path| task.item.path == path)
                .unwrap_or(false);
            task_match || media_match
        })
        .map(|task| task.item.clone())
        .collect::<Vec<_>>();

    store.tasks.retain(|task| {
        let task_match = task_id
            .as_deref()
            .map(|id| task.item.id == id)
            .unwrap_or(false);
        let media_match = media_path
            .as_deref()
            .map(|path| task.item.path == path)
            .unwrap_or(false);
        !(task_match || media_match)
    });
    drop(store);
    for item in removed {
        remove_task_meta(&item);
    }
    Ok(())
}

#[tauri::command]
pub async fn enqueue_task_run(
    app: AppHandle,
    request: EnqueueTaskRunCommandRequest,
) -> Result<(), String> {
    enqueue_task_run_internal(&app, request)
}

#[tauri::command]
pub async fn execute_task_run(
    app: AppHandle,
    request: ExecuteTaskRunCommandRequest,
) -> Result<(), String> {
    let response = execute_task_batch_internal(&app, vec![request]).await;
    if let Some(failed) = response.failed.first() {
        return Err(failed.error.clone());
    }
    Ok(())
}

#[tauri::command]
pub async fn execute_task_batch(
    app: AppHandle,
    request: ExecuteTaskBatchCommandRequest,
) -> Result<ExecuteTaskBatchCommandResponse, String> {
    Ok(execute_task_batch_internal(&app, request.items).await)
}

#[tauri::command]
pub async fn enqueue_and_execute_task_batch(
    app: AppHandle,
    request: EnqueueAndExecuteTaskBatchCommandRequest,
) -> Result<ExecuteTaskBatchCommandResponse, String> {
    let mut failed = Vec::<ExecuteTaskBatchFailedItem>::new();
    let mut execute_items = Vec::<ExecuteTaskRunCommandRequest>::new();

    for item in request.items {
        match enqueue_task_run_internal(&app, item.clone()) {
            Ok(()) => execute_items.push(ExecuteTaskRunCommandRequest {
                task_id: item.id,
                intent: Some(item.intent),
            }),
            Err(err) => failed.push(ExecuteTaskBatchFailedItem {
                task_id: item.id,
                error: err,
            }),
        }
    }

    let mut response = execute_task_batch_internal(&app, execute_items).await;
    response.failed.splice(0..0, failed);
    Ok(response)
}

fn enqueue_task_run_internal(
    app: &AppHandle,
    request: EnqueueTaskRunCommandRequest,
) -> Result<(), String> {
    ensure_workspace_hydrated_from_disk()?;
    let id = request.id.trim();
    let media_path = request.media_path.trim();
    if id.is_empty() {
        return Err("id is required".to_string());
    }
    if media_path.is_empty() {
        return Err("mediaPath is required".to_string());
    }

    let queued_item = {
        let mut store = lock_workspace_store()?;
        if let Some(existing) = find_task_mut(&mut store, id) {
            existing.item.path = media_path.to_string();
            existing.item.name = request.name;
            existing.item.media_kind = normalize_media_kind(&request.media_kind).to_string();
            existing.item.size_bytes = request.size_bytes;
            existing.item.transcribe_status = "queued".to_string();
            existing.item.task_progress = WorkspaceTaskProgressState::default();
            existing.item.transcribe_error = String::new();
            existing.item.result_text = String::new();
            existing.item.result_srt = String::new();
            existing.item.subtitle_segments_json = "[]".to_string();
            existing.item.llm_total_tokens = 0;

            existing.intent = normalize_intent(&request.intent).to_string();
            existing.source_lang = request
                .source_lang
                .unwrap_or_else(|| "auto".to_string())
                .trim()
                .to_string();
            existing.target_lang = request
                .target_lang
                .unwrap_or_else(|| "zh-CN".to_string())
                .trim()
                .to_string();
            existing.max_retries = request.max_retries.unwrap_or(0);
            existing.settings_snapshot = request.settings_snapshot.unwrap_or(Value::Null);
            persist_task_meta(existing)?;
            existing.item.clone()
        } else {
            let record = WorkspaceTaskRecord {
                item: WorkspaceQueueItem {
                    id: id.to_string(),
                    path: media_path.to_string(),
                    name: request.name,
                    media_kind: normalize_media_kind(&request.media_kind).to_string(),
                    size_bytes: request.size_bytes,
                    transcribe_status: "queued".to_string(),
                    task_progress: WorkspaceTaskProgressState::default(),
                    transcribe_error: String::new(),
                    result_text: String::new(),
                    result_srt: String::new(),
                    subtitle_segments_json: "[]".to_string(),
                    llm_total_tokens: 0,
                },
                intent: normalize_intent(&request.intent).to_string(),
                source_lang: request.source_lang.unwrap_or_else(|| "auto".to_string()),
                target_lang: request.target_lang.unwrap_or_else(|| "zh-CN".to_string()),
                max_retries: request.max_retries.unwrap_or(0),
                settings_snapshot: request.settings_snapshot.unwrap_or(Value::Null),
            };
            let emitted = record.item.clone();
            persist_task_meta(&record)?;
            store.tasks.push(record);
            emitted
        }
    };
    emit_task_state_changed(app, &queued_item);
    Ok(())
}

async fn execute_task_batch_internal(
    app: &AppHandle,
    items: Vec<ExecuteTaskRunCommandRequest>,
) -> ExecuteTaskBatchCommandResponse {
    let _ = ensure_workspace_hydrated_from_disk();
    let mut response = ExecuteTaskBatchCommandResponse {
        succeeded_task_ids: Vec::new(),
        failed: Vec::new(),
    };

    for request in items {
        let task_id = request.task_id.trim().to_string();
        if task_id.is_empty() {
            response.failed.push(ExecuteTaskBatchFailedItem {
                task_id,
                error: "taskId is required".to_string(),
            });
            continue;
        }

        if let Some(intent) = request.intent.as_deref() {
            let _ = patch_task_item(app, &task_id, |record| {
                record.intent = normalize_intent(intent).to_string();
            });
        }

        match execute_single_task(app, &task_id).await {
            Ok(()) => response.succeeded_task_ids.push(task_id),
            Err(err) => {
                log_task_failure_to_main(&task_id, &err);
                response.failed.push(ExecuteTaskBatchFailedItem {
                    task_id,
                    error: err,
                });
            }
        }
    }

    response
}

async fn execute_single_task(app: &AppHandle, task_id: &str) -> Result<(), String> {
    let record = get_task_record(task_id)?;
    let intent = normalize_intent(&record.intent).to_string();
    let runtime =
        resolve_runtime_settings(&record.settings_snapshot, intent == "TRANSCRIBE_TRANSLATE")?;
    let mut source_lang = if record.source_lang.trim().is_empty() {
        "auto".to_string()
    } else {
        record.source_lang.trim().to_string()
    };
    let target_lang = if record.target_lang.trim().is_empty() {
        "zh-CN".to_string()
    } else {
        record.target_lang.trim().to_string()
    };
    let task_output_dir =
        crate::services::task_path::task_output_dir(task_id, Path::new(&record.item.path));
    std::fs::create_dir_all(&task_output_dir).map_err(|err| err.to_string())?;
    let artifact_dir =
        crate::services::task_path::task_artifacts_dir(task_id, Path::new(&record.item.path));
    std::fs::create_dir_all(&artifact_dir).map_err(|err| err.to_string())?;
    migrate_legacy_artifacts(&task_output_dir, &artifact_dir)?;
    let step_context = StepContext {
        output_dir: &artifact_dir,
    };

    report_task_stage(app, task_id, TaskStage::Preparing, "", 1, 1)?;
    patch_task_item(app, task_id, |task| {
        task.item.transcribe_error = String::new();
        task.item.result_text = String::new();
        task.item.result_srt = String::new();
        task.item.subtitle_segments_json = "[]".to_string();
    })?;

    let step1_exec = match execute_step(
        &Step1AsrPipelineStep {
            task_id: task_id.to_string(),
            media_path: record.item.path.clone(),
            source_lang: source_lang.clone(),
            provider: runtime.provider.clone(),
            chunk_target_seconds: runtime.chunk_target_seconds,
            app: app.clone(),
        },
        &step_context,
    )
    .await
    {
        Ok(value) => value,
        Err(err) => {
            mark_task_failed(app, task_id, &err)?;
            return Err(err);
        }
    };

    if !step1_exec.output.source_lang.trim().is_empty() {
        source_lang = step1_exec.output.source_lang.clone();
    }

    let step15_exec = match execute_step(
        &Step15HotwordsPipelineStep {
            task_id: task_id.to_string(),
            media_path: record.item.path.clone(),
            source_lang: source_lang.clone(),
            words: step1_exec.output.words.clone(),
            hotwords: runtime.hotword_entries.clone(),
            enabled: runtime.enable_hotwords,
            translate_api_key: runtime.translate_api_key.clone(),
            translate_base_url: runtime.translate_base_url.clone(),
            translate_model: runtime.translate_model.clone(),
        },
        &step_context,
    )
    .await
    {
        Ok(value) => value,
        Err(err) => {
            mark_task_failed(app, task_id, &err)?;
            return Err(err);
        }
    };
    let step2_words = step15_exec
        .output
        .words
        .iter()
        .map(|word| crate::commands::transcription::WordTokenCommandDto {
            start: word.start,
            end: word.end,
            word: word.word.clone(),
        })
        .collect::<Vec<_>>();

    report_task_stage(app, task_id, TaskStage::Segmenting, "", 0, 1)?;

    let step2_exec = match execute_step(
        &Step2SegmentsPipelineStep {
            task_id: task_id.to_string(),
            media_path: record.item.path.clone(),
            source_lang: source_lang.clone(),
            words: step2_words,
            subtitle_max_words_per_segment: runtime.subtitle_max_words_per_segment,
            translate_api_key: runtime.translate_api_key.clone(),
            translate_base_url: runtime.translate_base_url.clone(),
            translate_model: runtime.translate_model.clone(),
            llm_concurrency: runtime.llm_concurrency,
            app: app.clone(),
        },
        &step_context,
    )
    .await
    {
        Ok(value) => value,
        Err(err) => {
            mark_task_failed(app, task_id, &err)?;
            return Err(err);
        }
    };
    let step2_segments = step2_exec.output;
    let source_text = source_text_from_step2_segments(&step2_segments);
    let step2_srt = step2_segments_to_srt(&step2_segments);
    update_processing_preview_from_step2(app, task_id, &step2_segments, &source_text)?;

    let run_result = if intent == "TRANSCRIBE_TRANSLATE" {
        execute_translate_steps(
            app,
            task_id,
            &record,
            runtime,
            source_lang,
            target_lang,
            &artifact_dir,
            &step2_segments,
            source_text,
        )
        .await
    } else {
        finish_transcribe_only(
            app,
            task_id,
            &record.item.path,
            &step2_segments,
            step2_srt,
            source_text,
            runtime.enable_subtitle_beautify,
        )
    };
    if let Err(err) = run_result {
        mark_task_failed(app, task_id, &err)?;
        return Err(err);
    }
    Ok(())
}

async fn execute_translate_steps(
    app: &AppHandle,
    task_id: &str,
    record: &WorkspaceTaskRecord,
    runtime: PipelineRuntimeSettings,
    source_lang: String,
    target_lang: String,
    output_dir: &Path,
    step2_segments: &[crate::commands::transcription::GroupedSentenceSegmentCommandDto],
    source_text: String,
) -> Result<(), String> {
    // Checkpoint contract:
    // Existing step53 layout is treated as user-selected input. Delete the file to rebuild it.
    if let Some(step5_existing) = read_json_file_if_exists::<
        crate::commands::translate::BuildStep53TranslationPolishCommandResponse,
    >(&output_dir.join(STEP_05_03_TRANSLATION_POLISH_FILE))?
    {
        let step_context = StepContext { output_dir };
        let step6_exec = execute_step(
            &Step6FinalCheckPipelineStep {
                task_id: task_id.to_string(),
                media_path: record.item.path.clone(),
                source_lang: source_lang.clone(),
                target_lang: target_lang.clone(),
                segments: step5_existing.segments.clone(),
                translate_api_key: runtime.translate_api_key.clone(),
                translate_base_url: runtime.translate_base_url.clone(),
                translate_model: runtime.translate_model.clone(),
                llm_concurrency: runtime.llm_concurrency,
                app: app.clone(),
            },
            &step_context,
        )
        .await
        .map_err(|err| {
            let _ = mark_task_failed(app, task_id, &err);
            err
        })?;
        ensure_step6_final_check_passed(&step6_exec.output.quality_summary).map_err(|err| {
            let _ = mark_task_failed(app, task_id, &err);
            err
        })?;
        return finish_translate_with_step5(
            app,
            task_id,
            &record.item.path,
            &step6_exec.output.segments,
            source_text,
            runtime.enable_subtitle_beautify,
        );
    }

    report_task_stage(app, task_id, TaskStage::Terminology, "", 0, 1)?;

    let terminology_segments = map_step2_segments_for_translate(step2_segments);
    let step_context = StepContext { output_dir };
    let step3_exec = match execute_step(
        &Step3TerminologyPipelineStep {
            task_id: task_id.to_string(),
            media_path: record.item.path.clone(),
            source_lang: source_lang.clone(),
            target_lang: target_lang.clone(),
            segments: terminology_segments.clone(),
            translate_api_key: runtime.translate_api_key.clone(),
            translate_base_url: runtime.translate_base_url.clone(),
            translate_model: runtime.translate_model.clone(),
            llm_concurrency: runtime.llm_concurrency,
            terminology_entries: runtime.terminology_entries.clone(),
        },
        &step_context,
    )
    .await
    {
        Ok(value) => value,
        Err(err) => {
            mark_task_failed(app, task_id, &err)?;
            return Err(err);
        }
    };
    let step3_response = step3_exec.output;
    report_task_stage(
        app,
        task_id,
        TaskStage::Terminology,
        if step3_exec.source == StepSource::Cache {
            "缓存命中"
        } else {
            ""
        },
        1,
        1,
    )?;

    let step4_exec = match execute_step(
        &Step4TranslationPipelineStep {
            task_id: task_id.to_string(),
            media_path: record.item.path.clone(),
            source_lang: source_lang.clone(),
            target_lang: target_lang.clone(),
            segments: terminology_segments,
            theme_summary: step3_response.theme_summary.clone(),
            terminology_entries: step3_response.terminology_entries.clone(),
            translate_api_key: runtime.translate_api_key.clone(),
            translate_base_url: runtime.translate_base_url.clone(),
            translate_model: runtime.translate_model.clone(),
            llm_concurrency: runtime.llm_concurrency,
            app: app.clone(),
        },
        &step_context,
    )
    .await
    {
        Ok(value) => value,
        Err(err) => {
            mark_task_failed(app, task_id, &err)?;
            return Err(err);
        }
    };

    if step4_exec.source == StepSource::Cache {
        report_task_stage(app, task_id, TaskStage::Translating, "缓存命中", 1, 1)?;
    }
    patch_task_item(app, task_id, |task| {
        task.item.result_text = source_text.clone();
        task.item.subtitle_segments_json = serialize_workspace_subtitle_segments(
            &workspace_subtitle_segments_from_translation_segments(&step4_exec.output.segments),
        );
    })?;

    let step51_exec = match execute_step(
        &Step51SourceSplitPipelineStep {
            task_id: task_id.to_string(),
            media_path: record.item.path.clone(),
            source_lang: source_lang.clone(),
            target_lang: target_lang.clone(),
            segments: step4_exec.output.segments.clone(),
            translate_api_key: runtime.translate_api_key.clone(),
            translate_base_url: runtime.translate_base_url.clone(),
            translate_model: runtime.translate_model.clone(),
            llm_concurrency: runtime.llm_concurrency,
            subtitle_max_words_per_segment: runtime.subtitle_max_words_per_segment,
            subtitle_length_reference: runtime.subtitle_length_reference,
            app: app.clone(),
        },
        &step_context,
    )
    .await
    {
        Ok(value) => value,
        Err(err) => {
            mark_task_failed(app, task_id, &err)?;
            return Err(err);
        }
    };

    if step51_exec.source == StepSource::Cache {
        report_task_stage(
            app,
            task_id,
            TaskStage::SubtitleLayout,
            "原文切分缓存命中",
            1,
            1,
        )?;
    }
    patch_task_item(app, task_id, |task| {
        task.item.result_text = source_text.clone();
        task.item.subtitle_segments_json = serialize_workspace_subtitle_segments(
            &workspace_subtitle_segments_from_step51_parents(&step51_exec.output.parents),
        );
    })?;

    let step52_exec = match execute_step(
        &Step52TranslationAlignPipelineStep {
            task_id: task_id.to_string(),
            media_path: record.item.path.clone(),
            source_lang: source_lang.clone(),
            target_lang: target_lang.clone(),
            theme_summary: step3_response.theme_summary.clone(),
            parents: step51_exec.output.parents.clone(),
            terminology_entries: step3_response.terminology_entries.clone(),
            translate_api_key: runtime.translate_api_key.clone(),
            translate_base_url: runtime.translate_base_url.clone(),
            translate_model: runtime.translate_model.clone(),
            llm_concurrency: runtime.llm_concurrency,
            app: app.clone(),
        },
        &step_context,
    )
    .await
    {
        Ok(value) => value,
        Err(err) => {
            mark_task_failed(app, task_id, &err)?;
            return Err(err);
        }
    };

    if step52_exec.source == StepSource::Cache {
        report_task_stage(
            app,
            task_id,
            TaskStage::SubtitleLayout,
            "译文对齐缓存命中",
            1,
            1,
        )?;
    }
    patch_task_item(app, task_id, |task| {
        task.item.result_text = source_text.clone();
        task.item.subtitle_segments_json = serialize_workspace_subtitle_segments(
            &workspace_subtitle_segments_from_step52_parents(&step52_exec.output.parents),
        );
    })?;

    let step53_exec = match execute_step(
        &Step53TranslationPolishPipelineStep {
            task_id: task_id.to_string(),
            media_path: record.item.path.clone(),
            source_lang: source_lang.clone(),
            target_lang: target_lang.clone(),
            theme_summary: step3_response.theme_summary,
            parents: step52_exec.output.parents,
            terminology_entries: step3_response.terminology_entries,
            translate_api_key: runtime.translate_api_key.clone(),
            translate_base_url: runtime.translate_base_url.clone(),
            translate_model: runtime.translate_model.clone(),
            llm_concurrency: runtime.llm_concurrency,
            subtitle_length_reference: runtime.subtitle_length_reference,
            app: app.clone(),
        },
        &step_context,
    )
    .await
    {
        Ok(value) => value,
        Err(err) => {
            mark_task_failed(app, task_id, &err)?;
            return Err(err);
        }
    };
    if step53_exec.source == StepSource::Cache {
        report_task_stage(
            app,
            task_id,
            TaskStage::SubtitleLayout,
            "译文润色缓存命中",
            1,
            1,
        )?;
    }
    let step53_output = step53_exec.output;

    let step6_exec = match execute_step(
        &Step6FinalCheckPipelineStep {
            task_id: task_id.to_string(),
            media_path: record.item.path.clone(),
            source_lang: source_lang.clone(),
            target_lang: target_lang.clone(),
            segments: step53_output.segments.clone(),
            translate_api_key: runtime.translate_api_key.clone(),
            translate_base_url: runtime.translate_base_url.clone(),
            translate_model: runtime.translate_model.clone(),
            llm_concurrency: runtime.llm_concurrency,
            app: app.clone(),
        },
        &step_context,
    )
    .await
    {
        Err(err) => {
            mark_task_failed(app, task_id, &err)?;
            return Err(err);
        }
        Ok(value) => value,
    };
    if let Err(err) = ensure_step6_final_check_passed(&step6_exec.output.quality_summary) {
        mark_task_failed(app, task_id, &err)?;
        return Err(err);
    }

    finish_translate_with_step5(
        app,
        task_id,
        &record.item.path,
        &step6_exec.output.segments,
        source_text,
        runtime.enable_subtitle_beautify,
    )
}

fn ensure_step6_final_check_passed(
    quality_summary: &crate::commands::translate::Step5QualitySummaryCommand,
) -> Result<(), String> {
    if quality_summary.passed {
        return Ok(());
    }
    Err(format!(
        "最终检查未通过: {} 项硬失败",
        quality_summary.hard_fail_count
    ))
}

fn finish_transcribe_only(
    app: &AppHandle,
    task_id: &str,
    media_path: &str,
    step2_segments: &[crate::commands::transcription::GroupedSentenceSegmentCommandDto],
    step2_srt: String,
    source_text: String,
    enable_subtitle_beautify: bool,
) -> Result<(), String> {
    let workspace_segments = workspace_subtitle_segments_from_step2_segments(step2_segments);
    let subtitle_segments_json = serialize_workspace_subtitle_segments(&workspace_segments);
    write_step7_srts(
        task_id,
        media_path,
        &workspace_segments,
        false,
        enable_subtitle_beautify,
    )?;

    patch_task_item(app, task_id, |task| {
        task.item.transcribe_status = "done".to_string();
        task.item.task_progress = done_task_progress_state();
        task.item.transcribe_error = String::new();
        task.item.result_text = source_text.clone();
        task.item.result_srt = step2_srt.clone();
        task.item.subtitle_segments_json = subtitle_segments_json.clone();
    })
}

fn finish_translate_with_step5(
    app: &AppHandle,
    task_id: &str,
    media_path: &str,
    segments: &[crate::commands::translate::BuildTranslationSegmentCommand],
    source_text: String,
    enable_subtitle_beautify: bool,
) -> Result<(), String> {
    let workspace_segments = workspace_subtitle_segments_from_translation_segments(segments);
    let subtitle_segments_json = serialize_workspace_subtitle_segments(&workspace_segments);
    write_step7_srts(
        task_id,
        media_path,
        &workspace_segments,
        true,
        enable_subtitle_beautify,
    )?;

    patch_task_item(app, task_id, |task| {
        task.item.transcribe_status = "done".to_string();
        task.item.task_progress = done_task_progress_state();
        task.item.transcribe_error = String::new();
        task.item.result_text = source_text.clone();
        task.item.result_srt = String::new();
        task.item.subtitle_segments_json = subtitle_segments_json.clone();
    })
}

fn write_step7_srts(
    task_id: &str,
    media_path: &str,
    segments: &[WorkspaceSubtitleSegment],
    include_translation_variants: bool,
    enable_subtitle_beautify: bool,
) -> Result<(), String> {
    let mut srt_segments = segments
        .iter()
        .map(
            |segment| crate::services::subtitle_srt::SubtitleSrtSegment {
                start_ms: segment.start_ms,
                end_ms: segment.end_ms,
                source_text: segment.source_text.clone(),
                translated_text: segment.translated_text.clone(),
            },
        )
        .collect::<Vec<_>>();
    if enable_subtitle_beautify {
        beautify_subtitle_srt_segments(&mut srt_segments);
    }
    let items = if include_translation_variants {
        vec![
            crate::services::subtitle_srt::ExportSrtItem::Source,
            crate::services::subtitle_srt::ExportSrtItem::Target,
            crate::services::subtitle_srt::ExportSrtItem::BilingualSourceFirst,
            crate::services::subtitle_srt::ExportSrtItem::BilingualTargetFirst,
        ]
    } else {
        vec![crate::services::subtitle_srt::ExportSrtItem::Source]
    };
    crate::services::subtitle_srt::write_task_output_variants(
        task_id,
        Path::new(media_path),
        &srt_segments,
        &items,
    )?;
    Ok(())
}

pub fn beautify_subtitle_srt_segments(
    segments: &mut [crate::services::subtitle_srt::SubtitleSrtSegment],
) {
    for segment in segments {
        segment.source_text = beautify_subtitle_text(&segment.source_text);
        segment.translated_text = beautify_subtitle_text(&segment.translated_text);
    }
}

pub fn task_enable_subtitle_beautify(task_id: &str) -> Result<bool, String> {
    let record = get_task_record(task_id)?;
    let saved = crate::services::preferences::load_saved_settings_from_default_path()
        .unwrap_or_else(|_| fallback_saved_settings());
    let _ = record;
    Ok(saved.enable_subtitle_beautify)
}

fn beautify_subtitle_text(raw: &str) -> String {
    let normalized = raw.replace('\r', "\n").replace('\n', " ");
    let normalized = normalized.trim();
    if normalized.is_empty() {
        return String::new();
    }

    let without_edges = trim_bounding_punctuation(normalized);
    if without_edges.is_empty() {
        return String::new();
    }
    let without_commas = remove_internal_commas_for_subtitle(&without_edges);
    let with_spacing = normalize_cjk_ascii_spacing(&without_commas);
    collapse_multiple_spaces(&with_spacing).trim().to_string()
}

fn trim_bounding_punctuation(text: &str) -> String {
    let mut chars = text.chars().collect::<Vec<char>>();
    while matches!(chars.first(), Some(ch) if is_subtitle_boundary_punctuation(*ch)) {
        let _ = chars.remove(0);
    }
    while matches!(chars.last(), Some(ch) if is_subtitle_boundary_punctuation(*ch)) {
        let _ = chars.pop();
    }
    chars.into_iter().collect()
}

fn is_subtitle_boundary_punctuation(ch: char) -> bool {
    ch.is_ascii_punctuation()
        || matches!(
            ch,
            '，' | '。'
                | '、'
                | '；'
                | '：'
                | '！'
                | '？'
                | '…'
                | '「'
                | '」'
                | '『'
                | '』'
                | '《'
                | '》'
                | '“'
                | '”'
                | '‘'
                | '’'
                | '（'
                | '）'
                | '［'
                | '］'
                | '【'
                | '】'
        )
}

fn remove_internal_commas_for_subtitle(text: &str) -> String {
    let chars = text.chars().collect::<Vec<_>>();
    let mut out = String::new();
    for idx in 0..chars.len() {
        let ch = chars[idx];
        if ch == ',' {
            let prev = chars.get(idx.wrapping_sub(1)).copied();
            let next = chars.get(idx + 1).copied();
            if prev.is_some_and(|value| value.is_ascii_digit())
                && next.is_some_and(|value| value.is_ascii_digit())
            {
                out.push(ch);
            } else {
                out.push(' ');
            }
            continue;
        }
        if ch == '，' {
            continue;
        }
        out.push(ch);
    }
    out
}

fn normalize_cjk_ascii_spacing(text: &str) -> String {
    let mut output = String::new();
    let mut previous = None;
    for ch in text.chars() {
        if let Some(prev) = previous {
            if need_cjk_ascii_space(prev, ch) && !output.ends_with(' ') {
                output.push(' ');
            }
        }
        output.push(ch);
        previous = Some(ch);
    }
    output
}

fn need_cjk_ascii_space(left: char, right: char) -> bool {
    (is_cjk_char(left) && is_ascii_word_char(right))
        || (is_ascii_word_char(left) && is_cjk_char(right))
}

fn is_ascii_word_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric()
}

fn is_cjk_char(ch: char) -> bool {
    let value = ch as u32;
    (0x3400..=0x4dbf).contains(&value)
        || (0x4e00..=0x9fff).contains(&value)
        || (0x20000..=0x2a6df).contains(&value)
        || (0xf900..=0xfaff).contains(&value)
        || (0x3040..=0x31ff).contains(&value)
        || (0xaf00..=0xafff).contains(&value)
}

fn collapse_multiple_spaces(text: &str) -> String {
    let mut out = String::new();
    let mut saw_space = false;
    for ch in text.chars() {
        if ch.is_whitespace() {
            if !saw_space {
                out.push(' ');
                saw_space = true;
            }
            continue;
        }
        out.push(ch);
        saw_space = false;
    }
    out
}

fn update_processing_preview_from_step2(
    app: &AppHandle,
    task_id: &str,
    step2_segments: &[crate::commands::transcription::GroupedSentenceSegmentCommandDto],
    source_text: &str,
) -> Result<(), String> {
    let subtitle_segments_json = serialize_workspace_subtitle_segments(
        &workspace_subtitle_segments_from_step2_segments(step2_segments),
    );
    patch_task_item(app, task_id, |task| {
        task.item.result_text = source_text.to_string();
        task.item.subtitle_segments_json = subtitle_segments_json.clone();
    })
}

fn workspace_subtitle_segments_from_step2_segments(
    segments: &[crate::commands::transcription::GroupedSentenceSegmentCommandDto],
) -> Vec<WorkspaceSubtitleSegment> {
    segments
        .iter()
        .map(|segment| WorkspaceSubtitleSegment {
            start_ms: seconds_to_millis(segment.start),
            end_ms: seconds_to_millis(segment.end),
            source_text: segment.segment.clone(),
            translated_text: String::new(),
            source_words: segment
                .tokens
                .iter()
                .map(|token| WorkspaceSubtitleWord {
                    start_ms: seconds_to_millis(token.start),
                    end_ms: seconds_to_millis(token.end),
                    word: token.text.clone(),
                })
                .collect(),
        })
        .collect()
}

fn workspace_subtitle_segments_from_translation_segments(
    segments: &[crate::commands::translate::BuildTranslationSegmentCommand],
) -> Vec<WorkspaceSubtitleSegment> {
    segments
        .iter()
        .map(|segment| WorkspaceSubtitleSegment {
            start_ms: seconds_to_millis(segment.start),
            end_ms: seconds_to_millis(segment.end),
            source_text: segment.source.clone(),
            translated_text: segment.translation.clone(),
            source_words: segment
                .tokens
                .iter()
                .map(|token| WorkspaceSubtitleWord {
                    start_ms: seconds_to_millis(token.start),
                    end_ms: seconds_to_millis(token.end),
                    word: token.text.clone(),
                })
                .collect(),
        })
        .collect()
}

fn workspace_subtitle_segments_from_step51_parents(
    parents: &[crate::commands::translate::Step5SplitParentCommand],
) -> Vec<WorkspaceSubtitleSegment> {
    let mut segments = Vec::new();
    for parent in parents {
        for part in &parent.parts {
            segments.push(WorkspaceSubtitleSegment {
                start_ms: seconds_to_millis(part.start),
                end_ms: seconds_to_millis(part.end),
                source_text: part.source.clone(),
                translated_text: String::new(),
                source_words: part
                    .tokens
                    .iter()
                    .map(|token| WorkspaceSubtitleWord {
                        start_ms: seconds_to_millis(token.start),
                        end_ms: seconds_to_millis(token.end),
                        word: token.text.clone(),
                    })
                    .collect(),
            });
        }
    }
    segments
}

fn workspace_subtitle_segments_from_step52_parents(
    parents: &[crate::commands::translate::Step5AlignedParentCommand],
) -> Vec<WorkspaceSubtitleSegment> {
    let mut segments = Vec::new();
    for parent in parents {
        for part in &parent.parts {
            segments.push(WorkspaceSubtitleSegment {
                start_ms: seconds_to_millis(part.start),
                end_ms: seconds_to_millis(part.end),
                source_text: part.source.clone(),
                translated_text: part.translation.clone(),
                source_words: part
                    .tokens
                    .iter()
                    .map(|token| WorkspaceSubtitleWord {
                        start_ms: seconds_to_millis(token.start),
                        end_ms: seconds_to_millis(token.end),
                        word: token.text.clone(),
                    })
                    .collect(),
            });
        }
    }
    segments
}

fn serialize_workspace_subtitle_segments(segments: &[WorkspaceSubtitleSegment]) -> String {
    serde_json::to_string(segments).unwrap_or_else(|_| "[]".to_string())
}

fn source_text_from_step2_segments(
    segments: &[crate::commands::transcription::GroupedSentenceSegmentCommandDto],
) -> String {
    segments
        .iter()
        .map(|segment| segment.segment.trim())
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn step2_segments_to_srt(
    segments: &[crate::commands::transcription::GroupedSentenceSegmentCommandDto],
) -> String {
    let mut out = String::new();
    for (index, segment) in segments.iter().enumerate() {
        let start_ms = seconds_to_millis(segment.start);
        let end_ms = seconds_to_millis(segment.end.max(segment.start));
        out.push_str(&(index + 1).to_string());
        out.push('\n');
        out.push_str(&format!(
            "{} --> {}\n",
            format_srt_ms(start_ms),
            format_srt_ms(end_ms)
        ));
        out.push_str(segment.segment.trim());
        out.push_str("\n\n");
    }
    out.trim_end().to_string()
}

fn format_srt_ms(total_ms: u64) -> String {
    let hours = total_ms / 3_600_000;
    let minutes = (total_ms % 3_600_000) / 60_000;
    let seconds = (total_ms % 60_000) / 1000;
    let millis = total_ms % 1000;
    format!("{hours:02}:{minutes:02}:{seconds:02},{millis:03}")
}

fn task_meta_version() -> u32 {
    2
}

fn ensure_workspace_hydrated_from_disk() -> Result<(), String> {
    {
        let hydrated = lock_workspace_hydrated()?;
        if *hydrated {
            return Ok(());
        }
    }

    hydrate_workspace_from_disk()?;
    let mut hydrated = lock_workspace_hydrated()?;
    *hydrated = true;
    Ok(())
}

fn hydrate_workspace_from_disk() -> Result<(), String> {
    let restored = load_task_meta_artifacts()?;
    if restored.is_empty() {
        return Ok(());
    }

    let mut store = lock_workspace_store()?;
    for artifact in restored {
        let record = workspace_record_from_meta(artifact);
        if store
            .tasks
            .iter()
            .any(|task| task.item.id == record.item.id)
        {
            continue;
        }
        store.tasks.push(record);
    }
    Ok(())
}

fn load_task_meta_artifacts() -> Result<Vec<WorkspaceTaskMetaArtifact>, String> {
    let output_dir = crate::services::output::resolve_output_dir();
    if !output_dir.exists() {
        return Ok(Vec::new());
    }

    let mut out = Vec::<WorkspaceTaskMetaArtifact>::new();
    let entries = std::fs::read_dir(&output_dir).map_err(|err| err.to_string())?;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let artifact_meta_path = path
            .join(crate::services::task_path::ARTIFACTS_DIR_NAME)
            .join(TASK_META_FILE_NAME);
        let legacy_meta_path = path.join(TASK_META_FILE_NAME);
        let Some(mut artifact) =
            read_json_file_if_exists::<WorkspaceTaskMetaArtifact>(&artifact_meta_path)?.or(
                read_json_file_if_exists::<WorkspaceTaskMetaArtifact>(&legacy_meta_path)?,
            )
        else {
            continue;
        };
        if artifact.item.transcribe_status == "processing" {
            artifact.item.transcribe_status = "error".to_string();
            artifact.item.task_progress = WorkspaceTaskProgressState::default();
            artifact.item.transcribe_error = "任务在运行中被中断，请重新开始".to_string();
        }
        out.push(artifact);
    }
    out.sort_by(|a, b| b.updated_at_ms.cmp(&a.updated_at_ms));
    Ok(out)
}

fn workspace_record_from_meta(artifact: WorkspaceTaskMetaArtifact) -> WorkspaceTaskRecord {
    WorkspaceTaskRecord {
        item: artifact.item,
        intent: normalize_intent(&artifact.intent).to_string(),
        source_lang: if artifact.source_lang.trim().is_empty() {
            "auto".to_string()
        } else {
            artifact.source_lang
        },
        target_lang: if artifact.target_lang.trim().is_empty() {
            "zh-CN".to_string()
        } else {
            artifact.target_lang
        },
        max_retries: artifact.max_retries,
        settings_snapshot: artifact.settings_snapshot,
    }
}

fn workspace_meta_from_record(record: &WorkspaceTaskRecord) -> WorkspaceTaskMetaArtifact {
    WorkspaceTaskMetaArtifact {
        version: task_meta_version(),
        item: record.item.clone(),
        intent: record.intent.clone(),
        source_lang: record.source_lang.clone(),
        target_lang: record.target_lang.clone(),
        max_retries: record.max_retries,
        settings_snapshot: record.settings_snapshot.clone(),
        updated_at_ms: now_millis(),
    }
}

fn persist_task_meta(record: &WorkspaceTaskRecord) -> Result<(), String> {
    let meta_path = task_meta_path_for_item(&record.item);
    let artifact = workspace_meta_from_record(record);
    write_json_file(&meta_path, &artifact)
}

fn remove_task_meta(item: &WorkspaceQueueItem) {
    let meta_path = task_meta_path_for_item(item);
    let _ = std::fs::remove_file(meta_path);
    let legacy_meta_path = task_output_dir_for_item(item).join(TASK_META_FILE_NAME);
    let _ = std::fs::remove_file(legacy_meta_path);
}

fn task_meta_path_for_item(item: &WorkspaceQueueItem) -> PathBuf {
    task_artifact_dir_for_item(item).join(TASK_META_FILE_NAME)
}

fn task_output_dir_for_item(item: &WorkspaceQueueItem) -> PathBuf {
    let path = item.path.trim();
    if path.is_empty() {
        crate::services::task_path::task_output_dir_by_id(&item.id)
    } else {
        crate::services::task_path::task_output_dir(&item.id, Path::new(path))
    }
}

fn task_artifact_dir_for_item(item: &WorkspaceQueueItem) -> PathBuf {
    let path = item.path.trim();
    if path.is_empty() {
        crate::services::task_path::task_artifacts_dir_by_id(&item.id)
    } else {
        crate::services::task_path::task_artifacts_dir(&item.id, Path::new(path))
    }
}

fn migrate_legacy_artifacts(task_output_dir: &Path, artifact_dir: &Path) -> Result<(), String> {
    if !task_output_dir.exists() {
        return Ok(());
    }
    std::fs::create_dir_all(artifact_dir).map_err(|err| err.to_string())?;
    migrate_legacy_logs_dir(task_output_dir, artifact_dir)?;
    let entries = std::fs::read_dir(task_output_dir).map_err(|err| err.to_string())?;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|v| v.to_str()) else {
            continue;
        };
        let Some(target_name) = migrate_target_artifact_name(name) else {
            continue;
        };
        let target_path = artifact_dir.join(target_name);
        if target_path.exists() {
            let _ = std::fs::remove_file(&path);
            continue;
        }
        std::fs::rename(&path, &target_path)
            .or_else(|_| std::fs::copy(&path, &target_path).map(|_| ()))
            .map_err(|err| err.to_string())?;
        let _ = std::fs::remove_file(&path);
    }
    Ok(())
}

fn migrate_legacy_logs_dir(task_output_dir: &Path, artifact_dir: &Path) -> Result<(), String> {
    let legacy_log_dir = task_output_dir.join("logs");
    if !legacy_log_dir.is_dir() {
        return Ok(());
    }
    let target_log_dir = artifact_dir.join("logs");
    if std::fs::rename(&legacy_log_dir, &target_log_dir).is_ok() {
        return Ok(());
    }
    move_directory_contents(&legacy_log_dir, &target_log_dir)?;
    let _ = std::fs::remove_dir_all(&legacy_log_dir);
    Ok(())
}

fn move_directory_contents(source_dir: &Path, target_dir: &Path) -> Result<(), String> {
    if !source_dir.is_dir() {
        return Ok(());
    }
    std::fs::create_dir_all(target_dir).map_err(|err| err.to_string())?;
    let entries = std::fs::read_dir(source_dir).map_err(|err| err.to_string())?;
    for entry in entries.flatten() {
        let source_path = entry.path();
        let target_path = target_dir.join(entry.file_name());
        if source_path.is_dir() {
            move_directory_contents(&source_path, &target_path)?;
            let _ = std::fs::remove_dir(&source_path);
            continue;
        }
        if !source_path.is_file() {
            continue;
        }
        if target_path.exists() {
            let _ = std::fs::remove_file(&source_path);
            continue;
        }
        std::fs::rename(&source_path, &target_path)
            .or_else(|_| std::fs::copy(&source_path, &target_path).map(|_| ()))
            .map_err(|err| err.to_string())?;
        let _ = std::fs::remove_file(&source_path);
    }
    Ok(())
}

fn migrate_target_artifact_name(name: &str) -> Option<&'static str> {
    match name {
        "step_01_asr.json" => Some(STEP_01_ASR_FILE),
        "step_01_5_hotwords.json" => Some(STEP_01_5_HOTWORDS_FILE),
        "step_02_segments.json" => Some(STEP_02_SEGMENTS_FILE),
        "step_03_terminology.json" => Some(STEP_03_TERMINOLOGY_FILE),
        "step_04_translation.json" => Some(STEP_04_TRANSLATION_FILE),
        "step_05_01_source_split.json" => Some(STEP_05_01_SOURCE_SPLIT_FILE),
        "step_05_02_translation_align.json" => Some(STEP_05_02_TRANSLATION_ALIGN_FILE),
        "step_05_03_translation_polish.json" => Some(STEP_05_03_TRANSLATION_POLISH_FILE),
        "step_06_final_check.json" => Some(STEP_06_FINAL_CHECK_FILE),
        "gpt.log" => Some("gpt.log"),
        "task_meta.json" => Some(TASK_META_FILE_NAME),
        _ => None,
    }
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn resolve_runtime_settings(
    snapshot: &Value,
    require_translate_llm: bool,
) -> Result<PipelineRuntimeSettings, String> {
    let snapshot_parsed =
        serde_json::from_value::<SettingsSnapshotInput>(snapshot.clone()).unwrap_or_default();
    let saved = crate::services::preferences::load_saved_settings_from_default_path()
        .unwrap_or_else(|_| fallback_saved_settings());

    let provider = snapshot_parsed
        .provider
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| saved.provider.clone());
    let chunk_target_seconds = snapshot_parsed
        .chunk_target_seconds
        .unwrap_or(saved.chunk_target_seconds)
        .clamp(30, 300);

    let translate_api_key = snapshot_parsed
        .translate_api_key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| saved.translate_api_key.clone());
    let translate_base_url = snapshot_parsed
        .translate_base_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| saved.translate_base_url.clone());
    let translate_model = snapshot_parsed
        .translate_model
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| saved.translate_model.clone());

    if require_translate_llm && translate_api_key.trim().is_empty() {
        return Err("translateApiKey is required for step_03~step_05".to_string());
    }
    if require_translate_llm && translate_base_url.trim().is_empty() {
        return Err("translateBaseUrl is required for step_03~step_05".to_string());
    }
    if require_translate_llm && translate_model.trim().is_empty() {
        return Err("translateModel is required for step_03~step_05".to_string());
    }

    let llm_concurrency = snapshot_parsed
        .llm_concurrency
        .unwrap_or(saved.llm_concurrency)
        .clamp(1, 16);
    let subtitle_max_words_per_segment = snapshot_parsed
        .subtitle_max_words_per_segment
        .unwrap_or(saved.subtitle_max_words_per_segment)
        .clamp(8, 40);
    let subtitle_length_reference = snapshot_parsed
        .subtitle_length_reference
        .unwrap_or(saved.subtitle_length_reference)
        .clamp(8, 80);
    let enable_terminology = snapshot_parsed
        .enable_terminology
        .unwrap_or(saved.enable_terminology);
    let snapshot_has_hotword_settings =
        snapshot.get("hotwordGroups").is_some() || snapshot.get("enableHotwords").is_some();
    let enable_hotwords = if snapshot_has_hotword_settings {
        snapshot_parsed.enable_hotwords.unwrap_or(true)
    } else {
        saved.enable_hotwords
    };
    let enable_subtitle_beautify = snapshot_parsed
        .enable_subtitle_beautify
        .unwrap_or(saved.enable_subtitle_beautify);

    let terminology_entries = if enable_terminology {
        let mut seen = HashSet::<(String, String)>::new();
        let mut out = Vec::<crate::commands::translate::TranslateTerminologyEntryCommand>::new();
        let snapshot_entries = snapshot_parsed
            .terminology_groups
            .unwrap_or_default()
            .into_iter()
            .flat_map(|group| group.terms.into_iter())
            .map(
                |term| crate::commands::translate::TranslateTerminologyEntryCommand {
                    source: term.origin.trim().to_string(),
                    target: term.target.trim().to_string(),
                    note: term.note.trim().to_string(),
                },
            )
            .collect::<Vec<_>>();

        for entry in snapshot_entries
            .into_iter()
            .chain(saved_terminology_entries(&saved).into_iter())
        {
            let source = entry.source.trim().to_string();
            let target = entry.target.trim().to_string();
            if source.is_empty() || target.is_empty() {
                continue;
            }
            let key = (source.to_ascii_lowercase(), target.to_ascii_lowercase());
            if !seen.insert(key) {
                continue;
            }
            out.push(
                crate::commands::translate::TranslateTerminologyEntryCommand {
                    source,
                    target,
                    note: entry.note.trim().to_string(),
                },
            );
        }
        out
    } else {
        Vec::new()
    };

    let hotword_entries = if enable_hotwords {
        let entries = if snapshot_has_hotword_settings {
            snapshot_hotword_entries(snapshot_parsed.hotword_groups.unwrap_or_default())
        } else {
            saved_hotword_entries(&saved)
        };
        deduplicated_hotword_entries(entries)
    } else {
        Vec::new()
    };

    Ok(PipelineRuntimeSettings {
        provider,
        chunk_target_seconds,
        translate_api_key,
        translate_base_url,
        translate_model,
        llm_concurrency,
        subtitle_max_words_per_segment,
        subtitle_length_reference,
        terminology_entries,
        hotword_entries,
        enable_hotwords,
        enable_subtitle_beautify,
    })
}

fn snapshot_hotword_entries(
    groups: Vec<SettingsSnapshotHotwordGroup>,
) -> Vec<crate::services::hotwords::HotwordEntry> {
    groups
        .into_iter()
        .flat_map(|group| group.terms.into_iter())
        .map(|term| crate::services::hotwords::HotwordEntry {
            word: term.word.trim().to_string(),
            aliases: term
                .aliases
                .into_iter()
                .map(|alias| alias.trim().to_string())
                .filter(|alias| !alias.is_empty())
                .collect(),
            lang: hotword_lang_from_settings_value(&term.lang),
            note: {
                let note = term.note.trim();
                if note.is_empty() {
                    None
                } else {
                    Some(note.to_string())
                }
            },
        })
        .filter(|entry| !entry.word.trim().is_empty())
        .collect()
}

fn fallback_saved_settings() -> crate::services::preferences::SavedSettings {
    crate::services::preferences::SavedSettings {
        provider: "cpu".to_string(),
        chunk_target_seconds: 180,
        subtitle_max_words_per_segment: 20,
        subtitle_length_reference: 28,
        asr_model: "parakeet-tdt-0.6b-v2".to_string(),
        demucs_model: "htdemucs_ft".to_string(),
        enable_vocal_separation: false,
        translate_api_key: String::new(),
        translate_base_url: "https://api.openai.com/v1".to_string(),
        translate_model: "gpt-4.1-mini".to_string(),
        llm_concurrency: 4,
        terminology_groups: Vec::new(),
        enable_terminology: true,
        hotword_groups: Vec::new(),
        enable_hotwords: true,
        enable_subtitle_beautify: true,
        auto_burn_hard_subtitle: false,
        subtitle_burn_mode: "bilingualSourceFirst".to_string(),
        subtitle_render_style: crate::services::preferences::SubtitleRenderStyle::default(),
    }
}

fn saved_terminology_entries(
    saved: &crate::services::preferences::SavedSettings,
) -> Vec<crate::commands::translate::TranslateTerminologyEntryCommand> {
    saved
        .terminology_groups
        .iter()
        .flat_map(|group| group.terms.iter())
        .map(
            |term| crate::commands::translate::TranslateTerminologyEntryCommand {
                source: term.origin.clone(),
                target: term.target.clone(),
                note: term.note.clone(),
            },
        )
        .collect()
}

fn saved_hotword_entries(
    saved: &crate::services::preferences::SavedSettings,
) -> Vec<crate::services::hotwords::HotwordEntry> {
    saved
        .hotword_groups
        .iter()
        .flat_map(|group| group.terms.iter())
        .map(|term| crate::services::hotwords::HotwordEntry {
            word: term.word.trim().to_string(),
            aliases: term
                .aliases
                .iter()
                .map(|alias| alias.trim().to_string())
                .filter(|alias| !alias.is_empty())
                .collect(),
            lang: hotword_lang_from_settings_value(&term.lang),
            note: {
                let note = term.note.trim();
                if note.is_empty() {
                    None
                } else {
                    Some(note.to_string())
                }
            },
        })
        .filter(|entry| !entry.word.trim().is_empty())
        .collect()
}

fn deduplicated_hotword_entries(
    entries: Vec<crate::services::hotwords::HotwordEntry>,
) -> Vec<crate::services::hotwords::HotwordEntry> {
    let mut seen = HashSet::<(String, Vec<String>, String, String)>::new();
    let mut out = Vec::new();

    for entry in entries {
        let word = entry.word.trim().to_string();
        if word.is_empty() {
            continue;
        }
        let aliases = entry
            .aliases
            .into_iter()
            .map(|alias| alias.trim().to_string())
            .filter(|alias| !alias.is_empty())
            .collect::<Vec<_>>();
        let note = entry.note.as_deref().unwrap_or_default().trim().to_string();
        let key = (
            word.to_ascii_lowercase(),
            aliases
                .iter()
                .map(|alias| alias.to_ascii_lowercase())
                .collect::<Vec<_>>(),
            format!("{:?}", entry.lang),
            note.to_ascii_lowercase(),
        );
        if !seen.insert(key) {
            continue;
        }
        out.push(crate::services::hotwords::HotwordEntry {
            word,
            aliases,
            lang: entry.lang,
            note: if note.is_empty() { None } else { Some(note) },
        });
    }

    out
}

fn hotword_lang_from_settings_value(value: &str) -> crate::services::hotwords::HotwordLang {
    match value.trim() {
        "zh" => crate::services::hotwords::HotwordLang::Zh,
        "non_zh" => crate::services::hotwords::HotwordLang::NonZh,
        _ => crate::services::hotwords::HotwordLang::Auto,
    }
}

fn map_step2_segments_for_translate(
    segments: &[crate::commands::transcription::GroupedSentenceSegmentCommandDto],
) -> Vec<crate::commands::translate::SourceSegmentForTerminologyCommand> {
    segments
        .iter()
        .map(
            |segment| crate::commands::translate::SourceSegmentForTerminologyCommand {
                segment: segment.segment.clone(),
                start: segment.start,
                end: segment.end,
                tokens: segment
                    .tokens
                    .iter()
                    .map(
                        |token| crate::commands::translate::SegmentTokenForTerminologyCommand {
                            text: token.text.clone(),
                            start: token.start,
                            end: token.end,
                        },
                    )
                    .collect(),
            },
        )
        .collect()
}

fn write_json_file<T: Serialize>(path: &Path, payload: &T) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let content = serde_json::to_string_pretty(payload).map_err(|err| err.to_string())?;
    std::fs::write(path, content.as_bytes()).map_err(|err| err.to_string())
}

fn read_json_file_if_exists<T: DeserializeOwned>(path: &Path) -> Result<Option<T>, String> {
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(path).map_err(|err| err.to_string())?;
    let parsed = serde_json::from_str::<T>(&raw).ok();
    Ok(parsed)
}

fn task_progress_state(
    stage: TaskStage,
    detail: impl Into<String>,
    current: u32,
    total: u32,
) -> WorkspaceTaskProgressState {
    WorkspaceTaskProgressState {
        stage: WorkspaceTaskStageState {
            code: stage.code().to_string(),
            label: stage.label().to_string(),
            order: stage.order(),
            detail: detail.into(),
            current,
            total,
        },
    }
}

fn done_task_progress_state() -> WorkspaceTaskProgressState {
    WorkspaceTaskProgressState {
        stage: WorkspaceTaskStageState::default(),
    }
}

fn report_task_stage(
    app: &AppHandle,
    task_id: &str,
    stage: TaskStage,
    detail: impl Into<String>,
    current: u32,
    total: u32,
) -> Result<(), String> {
    let progress = task_progress_state(stage, detail, current, total);
    patch_task_item(app, task_id, |task| {
        task.item.transcribe_status = "processing".to_string();
        task.item.task_progress = progress;
        task.item.transcribe_error = String::new();
    })
}

fn mark_task_failed(app: &AppHandle, task_id: &str, error: &str) -> Result<(), String> {
    patch_task_item(app, task_id, |task| {
        task.item.transcribe_status = "error".to_string();
        task.item.task_progress = WorkspaceTaskProgressState::default();
        task.item.transcribe_error = error.to_string();
    })
}

fn log_task_failure_to_main(task_id: &str, error: &str) {
    let payload = task_failure_log_payload(error);
    let logger = match get_task_record(task_id) {
        Ok(record) if !record.item.path.trim().is_empty() => {
            TaskLogger::main_with_media(task_id.to_string(), record.item.path)
        }
        _ => TaskLogger::main(task_id.to_string()),
    };
    logger.event(task_log_event::TASK_FAILED, Some(&payload));
}

fn task_failure_log_payload(error: &str) -> Value {
    serde_json::json!({
        "status": "error",
        "error": error,
    })
}

fn patch_task_item(
    app: &AppHandle,
    task_id: &str,
    mutator: impl FnOnce(&mut WorkspaceTaskRecord),
) -> Result<(), String> {
    let updated_item = {
        let mut store = lock_workspace_store()?;
        let Some(task) = find_task_mut(&mut store, task_id) else {
            return Err(format!("task not found: {task_id}"));
        };
        mutator(task);
        persist_task_meta(task)?;
        task.item.clone()
    };
    emit_task_state_changed(app, &updated_item);
    Ok(())
}

fn get_task_record(task_id: &str) -> Result<WorkspaceTaskRecord, String> {
    let store = lock_workspace_store()?;
    let Some(task) = store.tasks.iter().find(|entry| entry.item.id == task_id) else {
        return Err(format!("task not found: {task_id}"));
    };
    Ok(task.clone())
}

pub fn get_task_queue_item_for_export(task_id: &str) -> Result<WorkspaceQueueItem, String> {
    let normalized = task_id.trim();
    if normalized.is_empty() {
        return Err("taskId is required".to_string());
    }
    ensure_workspace_hydrated_from_disk()?;
    let record = get_task_record(normalized)?;
    Ok(record.item)
}

pub fn add_task_total_tokens(task_id: &str, delta_tokens: u64) -> Result<u64, String> {
    let task_id = task_id.trim();
    if task_id.is_empty() || delta_tokens == 0 {
        return Ok(0);
    }

    ensure_workspace_hydrated_from_disk()?;
    let updated_total = {
        let mut store = lock_workspace_store()?;
        let Some(task) = find_task_mut(&mut store, task_id) else {
            return Ok(0);
        };
        task.item.llm_total_tokens = task.item.llm_total_tokens.saturating_add(delta_tokens);
        persist_task_meta(task)?;
        task.item.llm_total_tokens
    };
    Ok(updated_total)
}

pub fn get_task_total_tokens_from_workspace(task_id: &str) -> Result<u64, String> {
    let task_id = task_id.trim();
    if task_id.is_empty() {
        return Ok(0);
    }

    ensure_workspace_hydrated_from_disk()?;
    let store = lock_workspace_store()?;
    let Some(task) = store.tasks.iter().find(|entry| entry.item.id == task_id) else {
        return Ok(0);
    };
    Ok(task.item.llm_total_tokens)
}

fn find_task_mut<'a>(
    store: &'a mut WorkspaceStore,
    task_id: &str,
) -> Option<&'a mut WorkspaceTaskRecord> {
    store
        .tasks
        .iter_mut()
        .find(|entry| entry.item.id == task_id)
}

fn normalize_media_kind(raw: &str) -> &str {
    match raw.trim() {
        "video" => "video",
        _ => "audio",
    }
}

fn normalize_intent(raw: &str) -> &str {
    match raw.trim() {
        "TRANSCRIBE_TRANSLATE" => "TRANSCRIBE_TRANSLATE",
        _ => "TRANSCRIBE",
    }
}

fn emit_task_state_changed(app: &AppHandle, item: &WorkspaceQueueItem) {
    let _ = app.emit("task-state-changed", item);
}

fn seconds_to_millis(value: f64) -> u64 {
    if !value.is_finite() || value <= 0.0 {
        return 0;
    }
    (value * 1000.0).round() as u64
}

#[cfg(test)]
mod tests {
    use super::{
        beautify_subtitle_text, collapse_multiple_spaces, ensure_step6_final_check_passed,
        is_ascii_word_char, is_cjk_char, need_cjk_ascii_space, task_failure_log_payload,
        trim_bounding_punctuation,
    };

    #[test]
    fn final_check_hard_failure_blocks_completion() {
        let result = ensure_step6_final_check_passed(
            &crate::commands::translate::Step5QualitySummaryCommand {
                passed: false,
                hard_fail_count: 2,
                issue_count: 3,
                soft_score: 60.0,
            },
        );

        assert_eq!(result, Err("最终检查未通过: 2 项硬失败".to_string()));
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

    #[test]
    fn subtitle_beautify_text_handles_empty() {
        assert_eq!(beautify_subtitle_text(""), "");
        assert_eq!(beautify_subtitle_text("   "), "");
    }

    #[test]
    fn subtitle_beautify_text_removes_boundary_punctuation_and_commas() {
        assert_eq!(beautify_subtitle_text(" (Hello, world), "), "Hello world");
        assert_eq!(
            beautify_subtitle_text("代码,IPC,sockets"),
            "代码 IPC sockets"
        );
    }

    #[test]
    fn cjk_ascii_space_helpers() {
        assert!(is_cjk_char('中'));
        assert!(is_ascii_word_char('A'));
        assert!(need_cjk_ascii_space('码', 'v'));
        assert!(!need_cjk_ascii_space('码', ','));
        assert_eq!(collapse_multiple_spaces("a   b"), "a b");
        assert_eq!(trim_bounding_punctuation("「Hello，"), "Hello");
    }
}
