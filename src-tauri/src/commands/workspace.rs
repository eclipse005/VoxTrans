use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Emitter};

use crate::services::pipeline::{
    CheckpointPolicy, PipelineStep, StepContext, StepSource, execute_step,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceQueueItem {
    pub id: String,
    pub path: String,
    pub name: String,
    pub media_kind: String,
    pub size_bytes: u64,
    pub transcribe_status: String,
    pub transcribe_progress: u32,
    pub transcribe_segment_current: u32,
    pub transcribe_segment_total: u32,
    pub transcribe_phase: String,
    pub transcribe_phase_detail: String,
    pub transcribe_error: String,
    pub result_text: String,
    pub result_srt: String,
    pub subtitle_segments_json: String,
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
const STEP1_ASR_FILE: &str = "step1_asr.json";
const STEP2_SEGMENTS_FILE: &str = "step2_segments.json";
const STEP3_TERMINOLOGY_FILE: &str = "step3_terminology.json";
const STEP4_TRANSLATION_FILE: &str = "step4_translation.json";

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
    terminology_groups: Option<Vec<SettingsSnapshotTerminologyGroup>>,
    #[serde(default)]
    enable_terminology: Option<bool>,
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

#[derive(Debug, Clone)]
struct PipelineRuntimeSettings {
    provider: String,
    chunk_target_seconds: u32,
    translate_api_key: String,
    translate_base_url: String,
    translate_model: String,
    llm_concurrency: u32,
    terminology_entries: Vec<crate::commands::translate::TranslateTerminologyEntryCommand>,
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
        "step1_asr"
    }

    fn artifact_file(&self) -> &'static str {
        STEP1_ASR_FILE
    }

    fn policy(&self) -> CheckpointPolicy {
        CheckpointPolicy::ValidateThenSkip
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
                    let progress = if total > 0 {
                        10 + ((current as f64 / total as f64) * 35.0).round() as u32
                    } else {
                        10
                    }
                    .clamp(10, 45);
                    let _ = patch_task_item(&app_for_progress, &task_id_owned, |task| {
                        task.item.transcribe_status = "processing".to_string();
                        task.item.transcribe_progress = progress;
                        task.item.transcribe_segment_current = current as u32;
                        task.item.transcribe_segment_total = total as u32;
                        task.item.transcribe_phase = "recognizing".to_string();
                        task.item.transcribe_phase_detail = format!("{current}/{total}");
                    });
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
struct Step2SegmentsPipelineStep {
    task_id: String,
    media_path: String,
    source_lang: String,
    words: Vec<crate::commands::transcription::WordTokenCommandDto>,
    translate_api_key: String,
    translate_base_url: String,
    translate_model: String,
    llm_concurrency: u32,
}

#[async_trait]
impl PipelineStep for Step2SegmentsPipelineStep {
    type Output = Vec<crate::commands::transcription::GroupedSentenceSegmentCommandDto>;

    fn name(&self) -> &'static str {
        "step2_segments"
    }

    fn artifact_file(&self) -> &'static str {
        STEP2_SEGMENTS_FILE
    }

    fn policy(&self) -> CheckpointPolicy {
        CheckpointPolicy::ValidateThenSkip
    }

    fn validate(&self, output: &Self::Output) -> Result<(), String> {
        if output.is_empty() {
            return Err("invalid step2 artifact".to_string());
        }
        Ok(())
    }

    async fn run(&self, _ctx: &StepContext<'_>) -> Result<Self::Output, String> {
        let step2_request = crate::commands::transcription::BuildSourceSentencesCommandRequest {
            task_id: self.task_id.clone(),
            audio_path: self.media_path.clone(),
            source_lang: self.source_lang.clone(),
            words: self.words.clone(),
            translate_api_key: self.translate_api_key.clone(),
            translate_base_url: self.translate_base_url.clone(),
            translate_model: self.translate_model.clone(),
            llm_concurrency: self.llm_concurrency,
        };
        let step2_response =
            crate::commands::transcription::build_source_sentences(step2_request).await?;
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
        "step3_terminology"
    }

    fn artifact_file(&self) -> &'static str {
        STEP3_TERMINOLOGY_FILE
    }

    fn policy(&self) -> CheckpointPolicy {
        CheckpointPolicy::ValidateThenSkip
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
}

#[async_trait]
impl PipelineStep for Step4TranslationPipelineStep {
    type Output = crate::commands::translate::BuildTranslationLayerCommandResponse;

    fn name(&self) -> &'static str {
        "step4_translation"
    }

    fn artifact_file(&self) -> &'static str {
        STEP4_TRANSLATION_FILE
    }

    fn policy(&self) -> CheckpointPolicy {
        CheckpointPolicy::ValidateThenSkip
    }

    fn validate(&self, output: &Self::Output) -> Result<(), String> {
        if output.task_id.trim().is_empty() || output.media_path.trim().is_empty() {
            return Err("invalid step4 artifact".to_string());
        }
        Ok(())
    }

    async fn run(&self, _ctx: &StepContext<'_>) -> Result<Self::Output, String> {
        crate::commands::translate::build_translation_layer(
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
        )
        .await
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

    let persisted = {
        let mut store = lock_workspace_store()?;
        if let Some(existing) = find_task_mut(&mut store, id) {
            existing.item.path = media_path.to_string();
            existing.item.name = request.name;
            existing.item.media_kind = normalize_media_kind(&request.media_kind).to_string();
            existing.item.size_bytes = request.size_bytes;
            existing.clone()
        } else {
            let record = WorkspaceTaskRecord {
                item: WorkspaceQueueItem {
                    id: id.to_string(),
                    path: media_path.to_string(),
                    name: request.name,
                    media_kind: normalize_media_kind(&request.media_kind).to_string(),
                    size_bytes: request.size_bytes,
                    transcribe_status: "pending".to_string(),
                    transcribe_progress: 0,
                    transcribe_segment_current: 0,
                    transcribe_segment_total: 0,
                    transcribe_phase: String::new(),
                    transcribe_phase_detail: String::new(),
                    transcribe_error: String::new(),
                    result_text: String::new(),
                    result_srt: String::new(),
                    subtitle_segments_json: "[]".to_string(),
                },
                intent: "TRANSCRIBE".to_string(),
                source_lang: "auto".to_string(),
                target_lang: "zh-CN".to_string(),
                max_retries: 0,
                settings_snapshot: Value::Null,
            };
            let cloned = record.clone();
            store.tasks.push(record);
            cloned
        }
    };
    persist_task_meta(&persisted)?;

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
            existing.item.transcribe_progress = 0;
            existing.item.transcribe_segment_current = 0;
            existing.item.transcribe_segment_total = 0;
            existing.item.transcribe_phase = String::new();
            existing.item.transcribe_phase_detail = String::new();
            existing.item.transcribe_error = String::new();
            existing.item.result_text = String::new();
            existing.item.result_srt = String::new();
            existing.item.subtitle_segments_json = "[]".to_string();

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
            existing.clone()
        } else {
            let record = WorkspaceTaskRecord {
                item: WorkspaceQueueItem {
                    id: id.to_string(),
                    path: media_path.to_string(),
                    name: request.name,
                    media_kind: normalize_media_kind(&request.media_kind).to_string(),
                    size_bytes: request.size_bytes,
                    transcribe_status: "queued".to_string(),
                    transcribe_progress: 0,
                    transcribe_segment_current: 0,
                    transcribe_segment_total: 0,
                    transcribe_phase: String::new(),
                    transcribe_phase_detail: String::new(),
                    transcribe_error: String::new(),
                    result_text: String::new(),
                    result_srt: String::new(),
                    subtitle_segments_json: "[]".to_string(),
                },
                intent: normalize_intent(&request.intent).to_string(),
                source_lang: request.source_lang.unwrap_or_else(|| "auto".to_string()),
                target_lang: request.target_lang.unwrap_or_else(|| "zh-CN".to_string()),
                max_retries: request.max_retries.unwrap_or(0),
                settings_snapshot: request.settings_snapshot.unwrap_or(Value::Null),
            };
            let emitted = record.clone();
            store.tasks.push(record);
            emitted
        }
    };
    persist_task_meta(&queued_item)?;
    emit_task_state_changed(app, &queued_item.item);
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
            Err(err) => response.failed.push(ExecuteTaskBatchFailedItem {
                task_id,
                error: err,
            }),
        }
    }

    response
}

async fn execute_single_task(app: &AppHandle, task_id: &str) -> Result<(), String> {
    let record = get_task_record(task_id)?;
    let runtime = resolve_runtime_settings(&record.settings_snapshot)?;
    let intent = normalize_intent(&record.intent).to_string();
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
    let output_dir =
        crate::services::task_path::task_output_dir(task_id, Path::new(&record.item.path));
    std::fs::create_dir_all(&output_dir).map_err(|err| err.to_string())?;
    let step2_path = output_dir.join(STEP2_SEGMENTS_FILE);
    let step_context = StepContext {
        output_dir: &output_dir,
    };

    patch_task_item(app, task_id, |task| {
        task.item.transcribe_status = "processing".to_string();
        task.item.transcribe_progress = 1;
        task.item.transcribe_segment_current = 0;
        task.item.transcribe_segment_total = 0;
        task.item.transcribe_phase = "initializing".to_string();
        task.item.transcribe_phase_detail = String::new();
        task.item.transcribe_error = String::new();
        task.item.result_text = String::new();
        task.item.result_srt = String::new();
        task.item.subtitle_segments_json = "[]".to_string();
    })?;

    let step2_segments = match read_json_file_if_exists::<
        Vec<crate::commands::transcription::GroupedSentenceSegmentCommandDto>,
    >(&step2_path)?
    {
        Some(existing) if !existing.is_empty() => {
            patch_task_item(app, task_id, |task| {
                task.item.transcribe_status = "processing".to_string();
                task.item.transcribe_progress = 54;
                task.item.transcribe_phase = "segment".to_string();
                task.item.transcribe_phase_detail = "resume from step2".to_string();
            })?;
            existing
        }
        _ => {
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

            patch_task_item(app, task_id, |task| {
                task.item.transcribe_status = "processing".to_string();
                task.item.transcribe_progress = 50;
                task.item.transcribe_segment_current = 0;
                task.item.transcribe_segment_total = 0;
                task.item.transcribe_phase = "segment".to_string();
                task.item.transcribe_phase_detail = if step1_exec.source == StepSource::Cache {
                    "resume from step1".to_string()
                } else {
                    "building step2 segments".to_string()
                };
            })?;

            let step2_exec = match execute_step(
                &Step2SegmentsPipelineStep {
                    task_id: task_id.to_string(),
                    media_path: record.item.path.clone(),
                    source_lang: source_lang.clone(),
                    words: step1_exec.output.words.clone(),
                    translate_api_key: runtime.translate_api_key.clone(),
                    translate_base_url: runtime.translate_base_url.clone(),
                    translate_model: runtime.translate_model.clone(),
                    llm_concurrency: runtime.llm_concurrency,
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
            step2_exec.output
        }
    };
    let source_text = source_text_from_step2_segments(&step2_segments);
    let step2_srt = step2_segments_to_srt(&step2_segments);

    if intent == "TRANSCRIBE_TRANSLATE" {
        execute_translate_steps(
            app,
            task_id,
            &record,
            runtime,
            source_lang,
            target_lang,
            &output_dir,
            &step2_segments,
            step2_srt,
            source_text,
        )
        .await
    } else {
        finish_transcribe_only(app, task_id, &step2_segments, step2_srt, source_text)
    }
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
    step2_srt: String,
    source_text: String,
) -> Result<(), String> {
    // Checkpoint contract:
    // `step4_translation.json` exists => skip step3/step4 and finish directly.
    // We intentionally DO NOT auto-invalidate by input hash.
    // If user wants recompute, delete target step artifacts manually.
    if let Some(step4_existing) = read_json_file_if_exists::<
        crate::commands::translate::BuildTranslationLayerCommandResponse,
    >(&output_dir.join(STEP4_TRANSLATION_FILE))?
    {
        return finish_translate_with_step4(app, task_id, &step4_existing, step2_srt, source_text);
    }

    patch_task_item(app, task_id, |task| {
        task.item.transcribe_status = "processing".to_string();
        task.item.transcribe_progress = 68;
        task.item.transcribe_phase = "translate".to_string();
        task.item.transcribe_phase_detail = "step3 terminology".to_string();
    })?;

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

    patch_task_item(app, task_id, |task| {
        task.item.transcribe_status = "processing".to_string();
        task.item.transcribe_progress = if step3_exec.source == StepSource::Cache {
            84
        } else {
            82
        };
        task.item.transcribe_phase = "translate".to_string();
        task.item.transcribe_phase_detail = "step4 translation".to_string();
    })?;

    let step4_exec = match execute_step(
        &Step4TranslationPipelineStep {
            task_id: task_id.to_string(),
            media_path: record.item.path.clone(),
            source_lang,
            target_lang,
            segments: terminology_segments,
            theme_summary: step3_response.theme_summary.clone(),
            terminology_entries: step3_response.terminology_entries.clone(),
            translate_api_key: runtime.translate_api_key.clone(),
            translate_base_url: runtime.translate_base_url.clone(),
            translate_model: runtime.translate_model.clone(),
            llm_concurrency: runtime.llm_concurrency,
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
    finish_translate_with_step4(app, task_id, &step4_exec.output, step2_srt, source_text)
}

fn finish_transcribe_only(
    app: &AppHandle,
    task_id: &str,
    step2_segments: &[crate::commands::transcription::GroupedSentenceSegmentCommandDto],
    step2_srt: String,
    source_text: String,
) -> Result<(), String> {
    let subtitle_segments = step2_segments
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
        .collect::<Vec<_>>();
    let subtitle_segments_json =
        serde_json::to_string(&subtitle_segments).map_err(|err| err.to_string())?;

    patch_task_item(app, task_id, |task| {
        task.item.transcribe_status = "done".to_string();
        task.item.transcribe_progress = 100;
        task.item.transcribe_segment_current = 0;
        task.item.transcribe_segment_total = 0;
        task.item.transcribe_phase = String::new();
        task.item.transcribe_phase_detail = String::new();
        task.item.transcribe_error = String::new();
        task.item.result_text = source_text.clone();
        task.item.result_srt = step2_srt.clone();
        task.item.subtitle_segments_json = subtitle_segments_json.clone();
    })
}

fn finish_translate_with_step4(
    app: &AppHandle,
    task_id: &str,
    step4_response: &crate::commands::translate::BuildTranslationLayerCommandResponse,
    step2_srt: String,
    source_text: String,
) -> Result<(), String> {
    let subtitle_segments = step4_response
        .segments
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
        .collect::<Vec<_>>();
    let subtitle_segments_json =
        serde_json::to_string(&subtitle_segments).map_err(|err| err.to_string())?;

    patch_task_item(app, task_id, |task| {
        task.item.transcribe_status = "done".to_string();
        task.item.transcribe_progress = 100;
        task.item.transcribe_segment_current = 0;
        task.item.transcribe_segment_total = 0;
        task.item.transcribe_phase = String::new();
        task.item.transcribe_phase_detail = String::new();
        task.item.transcribe_error = String::new();
        task.item.result_text = source_text.clone();
        task.item.result_srt = step2_srt.clone();
        task.item.subtitle_segments_json = subtitle_segments_json.clone();
    })
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
    1
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
        let meta_path = path.join(TASK_META_FILE_NAME);
        let Some(mut artifact) = read_json_file_if_exists::<WorkspaceTaskMetaArtifact>(&meta_path)?
        else {
            continue;
        };
        if artifact
            .item
            .transcribe_phase_detail
            .to_ascii_lowercase()
            .contains("pipeline")
        {
            artifact.item.transcribe_phase_detail = String::new();
        }
        if artifact.item.transcribe_status == "processing" {
            artifact.item.transcribe_status = "error".to_string();
            artifact.item.transcribe_progress = 0;
            artifact.item.transcribe_segment_current = 0;
            artifact.item.transcribe_segment_total = 0;
            artifact.item.transcribe_phase = String::new();
            artifact.item.transcribe_phase_detail = String::new();
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
}

fn task_meta_path_for_item(item: &WorkspaceQueueItem) -> PathBuf {
    task_output_dir_for_item(item).join(TASK_META_FILE_NAME)
}

fn task_output_dir_for_item(item: &WorkspaceQueueItem) -> PathBuf {
    let path = item.path.trim();
    if path.is_empty() {
        crate::services::task_path::task_output_dir_by_id(&item.id)
    } else {
        crate::services::task_path::task_output_dir(&item.id, Path::new(path))
    }
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn resolve_runtime_settings(snapshot: &Value) -> Result<PipelineRuntimeSettings, String> {
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

    if translate_api_key.trim().is_empty() {
        return Err("translateApiKey is required for step2/step4".to_string());
    }
    if translate_base_url.trim().is_empty() {
        return Err("translateBaseUrl is required for step2/step4".to_string());
    }
    if translate_model.trim().is_empty() {
        return Err("translateModel is required for step2/step4".to_string());
    }

    let llm_concurrency = snapshot_parsed
        .llm_concurrency
        .unwrap_or(saved.llm_concurrency)
        .clamp(1, 16);
    let enable_terminology = snapshot_parsed
        .enable_terminology
        .unwrap_or(saved.enable_terminology);

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

    Ok(PipelineRuntimeSettings {
        provider,
        chunk_target_seconds,
        translate_api_key,
        translate_base_url,
        translate_model,
        llm_concurrency,
        terminology_entries,
    })
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
        enable_punctuation_optimization: false,
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

fn mark_task_failed(app: &AppHandle, task_id: &str, error: &str) -> Result<(), String> {
    patch_task_item(app, task_id, |task| {
        task.item.transcribe_status = "error".to_string();
        task.item.transcribe_progress = 0;
        task.item.transcribe_segment_current = 0;
        task.item.transcribe_segment_total = 0;
        task.item.transcribe_phase = String::new();
        task.item.transcribe_phase_detail = String::new();
        task.item.transcribe_error = error.to_string();
    })
}

fn patch_task_item(
    app: &AppHandle,
    task_id: &str,
    mutator: impl FnOnce(&mut WorkspaceTaskRecord),
) -> Result<(), String> {
    let updated_record = {
        let mut store = lock_workspace_store()?;
        let Some(task) = find_task_mut(&mut store, task_id) else {
            return Err(format!("task not found: {task_id}"));
        };
        mutator(task);
        task.clone()
    };
    persist_task_meta(&updated_record)?;
    emit_task_state_changed(app, &updated_record.item);
    Ok(())
}

fn get_task_record(task_id: &str) -> Result<WorkspaceTaskRecord, String> {
    let store = lock_workspace_store()?;
    let Some(task) = store.tasks.iter().find(|entry| entry.item.id == task_id) else {
        return Err(format!("task not found: {task_id}"));
    };
    Ok(task.clone())
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
