use std::sync::{Mutex, OnceLock};

use serde_json::Value;
use tauri::{AppHandle, Emitter};

use crate::domain::task::stage::TaskStage;

mod adapters;
mod artifact_migration;
mod execution_flow;
mod json_files;
mod log_payload;
mod meta;
mod output_completion;
mod pipeline_runner;
mod pipeline_steps;
mod preview;
mod progress;
mod queue_ops;
mod runtime_settings;
mod task_logs;
mod translation_flow;
mod types;

use meta::{ensure_workspace_hydrated_from_disk, persist_task_meta};
use queue_ops::{
    delete_tasks_internal, enqueue_task_run_internal, execute_task_batch_internal,
    register_task_upload_internal, update_task_languages_internal,
};
use runtime_settings::fallback_saved_settings;
pub use types::*;

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
const STEP_02_SEGMENTS_FILE: &str = "step_02_segments.json";
const STEP_03_TERMINOLOGY_FILE: &str = "step_03_terminology.json";
const STEP_04_TRANSLATION_FILE: &str = "step_04_translation.json";
const STEP_05_01_SOURCE_SPLIT_FILE: &str = "step_05_01_source_split.json";
const STEP_05_02_TRANSLATION_ALIGN_FILE: &str = "step_05_02_translation_align.json";

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
    register_task_upload_internal(request)
}

#[tauri::command]
pub async fn delete_tasks(request: DeleteTasksCommandRequest) -> Result<(), String> {
    delete_tasks_internal(request)
}

#[tauri::command]
pub async fn enqueue_task_run(
    app: AppHandle,
    request: EnqueueTaskRunCommandRequest,
) -> Result<(), String> {
    enqueue_task_run_internal(&app, request)
}

#[tauri::command]
pub async fn update_task_languages(
    app: AppHandle,
    request: UpdateTaskLanguagesCommandRequest,
) -> Result<(), String> {
    update_task_languages_internal(&app, request)
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

pub fn task_subtitle_beautify_context(task_id: &str) -> Result<(bool, String, String), String> {
    let record = get_task_record(task_id)?;
    let saved = crate::services::preferences::load_saved_settings_from_default_path()
        .unwrap_or_else(|_| fallback_saved_settings());
    Ok((
        saved.enable_subtitle_beautify,
        saved.subtitle_length_preset,
        record.target_lang,
    ))
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

fn default_task_source_lang() -> String {
    "en".to_string()
}

fn default_task_target_lang() -> String {
    "zh-CN".to_string()
}

fn normalize_task_source_lang(raw: &str) -> String {
    match raw.trim().to_ascii_lowercase().as_str() {
        "en" | "en-us" | "english" => "en".to_string(),
        "zh" | "zh-cn" | "zh-hans" | "chinese" | "mandarin" => "zh".to_string(),
        "yue" | "yue-hk" | "zh-yue" | "cantonese" | "粤语" | "廣東話" | "广东话" => {
            "yue".to_string()
        }
        "ja" | "ja-jp" | "japanese" => "ja".to_string(),
        "ko" | "ko-kr" | "korean" => "ko".to_string(),
        "fr" | "fr-fr" | "french" => "fr".to_string(),
        "de" | "de-de" | "german" => "de".to_string(),
        "it" | "it-it" | "italian" => "it".to_string(),
        "es" | "es-es" | "spanish" => "es".to_string(),
        "pt" | "pt-pt" | "pt-br" | "portuguese" => "pt".to_string(),
        _ => default_task_source_lang(),
    }
}

fn normalize_task_target_lang(raw: &str) -> String {
    let value = raw.trim();
    if value.is_empty() {
        default_task_target_lang()
    } else {
        value.to_string()
    }
}

fn emit_task_state_changed(app: &AppHandle, item: &WorkspaceQueueItem) {
    let _ = app.emit("task-state-changed", item);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_cantonese_source_language_aliases() {
        for alias in [
            "yue",
            "yue-HK",
            "zh-yue",
            "Cantonese",
            "粤语",
            "廣東話",
            "广东话",
        ] {
            assert_eq!(normalize_task_source_lang(alias), "yue");
        }
    }
}
