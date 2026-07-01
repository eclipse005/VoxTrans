use tauri::{AppHandle, Emitter, Manager};
use crate::db::store::TaskStore;
use crate::domain::error::{WorkspaceError, WorkspaceResult};
use crate::domain::task::runtime_settings::FrozenSettings;
use crate::domain::task::stage::TaskStage;

mod execution_flow;
mod log_payload;
mod meta;
mod output_completion;
mod pipeline_runner;
mod pipeline_steps;
mod preview;
mod progress;
mod queue_ops;
mod store;
mod task_logs;
mod translation_flow;
mod types;

use crate::domain::task::runtime_settings::fallback_saved_settings;
use meta::ensure_workspace_hydrated_from_db;
use queue_ops::{
    delete_tasks_internal, enqueue_task_run_internal, execute_task_batch_internal,
    register_task_upload_internal, update_task_languages_internal,
    update_task_terminology_internal,
};
use store::{TaskStore as _, find_task_mut, is_workspace_hydrated, lock_workspace_store};
pub use types::*;

#[derive(Debug, Clone)]
struct WorkspaceTaskRecord {
    item: WorkspaceQueueItem,
    intent: String,
    source_lang: String,
    target_lang: String,
    max_retries: u32,
    /// Captured at enqueue time; see `FrozenSettings` docs.
    frozen: FrozenSettings,
    /// 入队顺序号；hydrate 时从 DB 读出，persist 时透传给 upsert。
    /// ON CONFLICT 不更新该列，保证顺序稳定。
    enqueue_seq: i64,
}

#[tauri::command]
pub async fn load_workspace_state(
    app: AppHandle,
) -> Result<WorkspaceStateResponse, String> {
    ensure_workspace_hydrated_from_db(&app).await?;
    let store = lock_workspace_store()?;
    Ok(WorkspaceStateResponse {
        queue: store.tasks().iter().map(|task| task.item.clone()).collect(),
    })
}

#[tauri::command]
pub async fn load_workspace_task(
    app: AppHandle,
    request: LoadWorkspaceTaskCommandRequest,
) -> Result<WorkspaceTaskResponse, String> {
    let task_id = require_task_id(&request.task_id)?;
    ensure_workspace_hydrated_from_db(&app).await?;
    let task = get_task_record(task_id)?;
    Ok(WorkspaceTaskResponse {
        item: task.item.clone(),
    })
}

#[tauri::command]
pub async fn register_task_upload(
    app: AppHandle,
    request: RegisterTaskUploadCommandRequest,
) -> Result<(), String> {
    Ok(register_task_upload_internal(&app, request).await?)
}

#[tauri::command]
pub async fn delete_tasks(
    app: AppHandle,
    request: DeleteTasksCommandRequest,
) -> Result<(), String> {
    Ok(delete_tasks_internal(&app, request).await?)
}

#[tauri::command]
pub async fn enqueue_task_run(
    app: AppHandle,
    request: EnqueueTaskRunCommandRequest,
) -> Result<(), String> {
    Ok(enqueue_task_run_internal(&app, request).await?)
}

#[tauri::command]
pub async fn update_task_languages(
    app: AppHandle,
    request: UpdateTaskLanguagesCommandRequest,
) -> Result<(), String> {
    Ok(update_task_languages_internal(&app, request).await?)
}

#[tauri::command]
pub async fn update_task_terminology(
    app: AppHandle,
    request: UpdateTaskTerminologyCommandRequest,
) -> Result<(), String> {
    Ok(update_task_terminology_internal(&app, request).await?)
}

#[tauri::command]
pub async fn save_subtitle_editor(
    app: AppHandle,
    request: SaveSubtitleEditorCommandRequest,
) -> Result<(), String> {
    let task_id = require_task_id(&request.task_id)?;
    ensure_workspace_hydrated_from_db(&app).await?;
    let subtitle_segments_json = request.subtitle_segments_json.clone();
    // Persist segments to DB FIRST: if it fails, surface the error and
    // leave the task meta untouched so callers see a consistent state.
    // Doing it in the other order would mark the task with new
    // result_srt + segments_json while the DB segments table silently
    // stays stale -- a restart hydrate then resets segments to empty.
    let store = app.state::<TaskStore>().inner().clone();
    let segments: Vec<crate::services::workspace_subtitle::WorkspaceSubtitleSegment> =
        serde_json::from_str(&subtitle_segments_json).unwrap_or_default();
    store
        .replace_segments(task_id, &segments)
        .await
        .map_err(|e| format!("persist segments: {e}"))?;
    patch_task_item(&app, task_id, |task| {
        task.item.result_srt = request.content;
        task.item.subtitle_segments_json = request.subtitle_segments_json;
    })
    .await?;
    Ok(())
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
        match enqueue_task_run_internal(&app, item.clone()).await {
            Ok(()) => execute_items.push(ExecuteTaskRunCommandRequest {
                task_id: item.id,
                intent: Some(item.intent),
            }),
            Err(err) => failed.push(ExecuteTaskBatchFailedItem {
                task_id: item.id,
                error: err.to_command_error(),
            }),
        }
    }

    let mut response = execute_task_batch_internal(&app, execute_items).await;
    response.failed.splice(0..0, failed);
    Ok(response)
}

pub fn task_subtitle_beautify_context(
    store: &TaskStore,
    task_id: &str,
) -> Result<(bool, String, String), String> {
    let record = get_task_record(task_id)?;
    let saved = crate::services::preferences::load_saved_settings_from_default_path(store)
        .unwrap_or_else(|_| fallback_saved_settings());
    Ok((
        saved.enable_subtitle_beautify,
        saved.subtitle_length_preset,
        record.target_lang,
    ))
}

async fn patch_task_item(
    app: &AppHandle,
    task_id: &str,
    mutator: impl FnOnce(&mut WorkspaceTaskRecord),
) -> WorkspaceResult<()> {
    let mut updated = {
        let store = lock_workspace_store()?;
        let Some(task) = store
            .tasks()
            .iter()
            .find(|entry| entry.item.id == task_id)
        else {
            return Err(WorkspaceError::TaskNotFound(task_id.to_string()));
        };
        task.clone()
    };
    mutator(&mut updated);
    let db_store = app.state::<TaskStore>().inner();
    crate::commands::workspace::meta::persist_task_meta(db_store, &updated).await?;
    let updated_item = updated.item.clone();
    {
        let mut store = lock_workspace_store()?;
        if let Some(task) = find_task_mut(&mut store, task_id) {
            *task = updated;
        }
    }
    emit_task_state_changed(app, &updated_item);
    Ok(())
}

fn get_task_record(task_id: &str) -> WorkspaceResult<WorkspaceTaskRecord> {
    let store = lock_workspace_store()?;
    let Some(task) = store.tasks().iter().find(|entry| entry.item.id == task_id) else {
        return Err(WorkspaceError::TaskNotFound(task_id.to_string()));
    };
    Ok(task.clone())
}

fn require_task_id(task_id: &str) -> WorkspaceResult<&str> {
    let normalized = task_id.trim();
    if normalized.is_empty() {
        return Err(WorkspaceError::InvalidRequest(
            "taskId is required".to_string(),
        ));
    }
    Ok(normalized)
}

pub fn get_task_queue_item_for_export(task_id: &str) -> WorkspaceResult<WorkspaceQueueItem> {
    let normalized = require_task_id(task_id)?;
    if !is_workspace_hydrated() {
        return Err(WorkspaceError::NotHydrated);
    }
    let record = get_task_record(normalized)?;
    Ok(record.item)
}

pub fn add_task_total_tokens(task_id: &str, delta_tokens: u64) -> WorkspaceResult<u64> {
    let task_id = task_id.trim();
    if task_id.is_empty() || delta_tokens == 0 {
        return Ok(0);
    }

    if !is_workspace_hydrated() {
        // Workspace not yet hydrated; skip until next persist.
        return Ok(0);
    }
    let mut store = lock_workspace_store()?;
    let Some(task) = find_task_mut(&mut store, task_id) else {
        return Ok(0);
    };
    task.item.llm_total_tokens = task.item.llm_total_tokens.saturating_add(delta_tokens);
    let updated = task.item.clone();
    // The mirrored total is also written through to SQLite by
    // record_llm_usage() right after this; see services/task_usage.rs.
    Ok(updated.llm_total_tokens)
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

    #[test]
    fn require_task_id_rejects_empty_input_with_invalid_request() {
        let err = require_task_id("  ").expect_err("empty task id should be rejected");

        assert_eq!(err.code(), "INVALID_REQUEST");
        assert_eq!(err.to_string(), "invalid request: taskId is required");
    }
}
