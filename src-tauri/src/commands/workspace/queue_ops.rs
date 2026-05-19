use serde_json::Value;
use tauri::AppHandle;

use crate::domain::error::{WorkspaceError, WorkspaceResult};

use super::execution_flow::execute_single_task;
use super::meta::{ensure_workspace_hydrated_from_disk, persist_task_meta, remove_task_meta};
use super::task_logs::log_task_failure_to_main;
use super::{
    DeleteTasksCommandRequest, EnqueueTaskRunCommandRequest, ExecuteTaskBatchCommandResponse,
    ExecuteTaskBatchFailedItem, ExecuteTaskRunCommandRequest, RegisterTaskUploadCommandRequest,
    UpdateTaskLanguagesCommandRequest, WorkspaceQueueItem, WorkspaceTaskProgressState,
    WorkspaceTaskRecord, default_task_source_lang, default_task_target_lang,
    emit_task_state_changed, find_task_mut, lock_workspace_store, normalize_intent,
    normalize_media_kind, normalize_task_source_lang, normalize_task_target_lang, patch_task_item,
};

pub(super) fn register_task_upload_internal(
    request: RegisterTaskUploadCommandRequest,
) -> WorkspaceResult<()> {
    ensure_workspace_hydrated_from_disk()?;
    let id = request.id.trim();
    let media_path = request.media_path.trim();
    if id.is_empty() {
        return Err(WorkspaceError::InvalidRequest("id is required".to_string()));
    }
    if media_path.is_empty() {
        return Err(WorkspaceError::InvalidRequest("mediaPath is required".to_string()));
    }

    {
        let mut store = lock_workspace_store()?;
        if let Some(existing) = find_task_mut(&mut store, id) {
            apply_upload_fields(
                &mut existing.item,
                media_path,
                request.name,
                &request.media_kind,
                request.size_bytes,
            );
            persist_task_meta(existing)?;
        } else {
            let record = WorkspaceTaskRecord {
                item: new_workspace_queue_item(
                    id,
                    media_path,
                    request.name,
                    &request.media_kind,
                    request.size_bytes,
                    "pending",
                ),
                intent: "TRANSCRIBE".to_string(),
                source_lang: default_task_source_lang(),
                target_lang: default_task_target_lang(),
                max_retries: 0,
                settings_snapshot: Value::Null,
            };
            persist_task_meta(&record)?;
            store.tasks.push(record);
        }
    }

    Ok(())
}

pub(super) fn delete_tasks_internal(request: DeleteTasksCommandRequest) -> WorkspaceResult<()> {
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

pub(super) fn enqueue_task_run_internal(
    app: &AppHandle,
    request: EnqueueTaskRunCommandRequest,
) -> WorkspaceResult<()> {
    ensure_workspace_hydrated_from_disk()?;
    let id = request.id.trim();
    let media_path = request.media_path.trim();
    if id.is_empty() {
        return Err(WorkspaceError::InvalidRequest("id is required".to_string()));
    }
    if media_path.is_empty() {
        return Err(WorkspaceError::InvalidRequest("mediaPath is required".to_string()));
    }

    let queued_item = {
        let mut store = lock_workspace_store()?;
        if let Some(existing) = find_task_mut(&mut store, id) {
            apply_enqueue_request(existing, request);
            persist_task_meta(existing)?;
            existing.item.clone()
        } else {
            let source_lang = request
                .source_lang
                .as_deref()
                .map(normalize_task_source_lang)
                .unwrap_or_else(default_task_source_lang);
            let target_lang = request
                .target_lang
                .as_deref()
                .map(normalize_task_target_lang)
                .unwrap_or_else(default_task_target_lang);
            let mut item = new_workspace_queue_item(
                id,
                media_path,
                request.name,
                &request.media_kind,
                request.size_bytes,
                "queued",
            );
            item.source_lang = source_lang.clone();
            item.target_lang = target_lang.clone();
            let record = WorkspaceTaskRecord {
                item,
                intent: normalize_intent(&request.intent).to_string(),
                source_lang,
                target_lang,
                max_retries: request.max_retries.unwrap_or(0),
                settings_snapshot: crate::services::preferences::load_saved_settings_from_default_path()
                    .map(|settings| serde_json::to_value(settings).unwrap_or(Value::Null))
                    .unwrap_or(Value::Null),
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

pub(super) fn update_task_languages_internal(
    app: &AppHandle,
    request: UpdateTaskLanguagesCommandRequest,
) -> WorkspaceResult<()> {
    ensure_workspace_hydrated_from_disk()?;
    let task_id = request.task_id.trim();
    if task_id.is_empty() {
        return Err(WorkspaceError::InvalidRequest("taskId is required".to_string()));
    }

    let source_lang = normalize_task_source_lang(&request.source_lang);
    let target_lang = normalize_task_target_lang(&request.target_lang);
    let updated_item = {
        let mut store = lock_workspace_store()?;
        let Some(task) = find_task_mut(&mut store, task_id) else {
            return Err(WorkspaceError::TaskNotFound(task_id.to_string()));
        };
        if task.item.transcribe_status == "processing" || task.item.transcribe_status == "queued" {
            return Err(WorkspaceError::TaskBusy);
        }

        task.source_lang = source_lang.clone();
        task.target_lang = target_lang.clone();
        task.item.source_lang = source_lang;
        task.item.target_lang = target_lang;
        persist_task_meta(task)?;
        task.item.clone()
    };
    emit_task_state_changed(app, &updated_item);
    Ok(())
}

pub(super) async fn execute_task_batch_internal(
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

fn new_workspace_queue_item(
    id: &str,
    media_path: &str,
    name: String,
    media_kind: &str,
    size_bytes: u64,
    status: &str,
) -> WorkspaceQueueItem {
    WorkspaceQueueItem {
        id: id.to_string(),
        path: media_path.to_string(),
        name,
        media_kind: normalize_media_kind(media_kind).to_string(),
        size_bytes,
        source_lang: default_task_source_lang(),
        target_lang: default_task_target_lang(),
        transcribe_status: status.to_string(),
        task_progress: WorkspaceTaskProgressState::default(),
        transcribe_error: String::new(),
        result_text: String::new(),
        result_srt: String::new(),
        subtitle_segments_json: "[]".to_string(),
        llm_total_tokens: 0,
    }
}

fn apply_upload_fields(
    item: &mut WorkspaceQueueItem,
    media_path: &str,
    name: String,
    media_kind: &str,
    size_bytes: u64,
) {
    item.path = media_path.to_string();
    item.name = name;
    item.media_kind = normalize_media_kind(media_kind).to_string();
    item.size_bytes = size_bytes;
}

fn apply_enqueue_request(record: &mut WorkspaceTaskRecord, request: EnqueueTaskRunCommandRequest) {
    apply_upload_fields(
        &mut record.item,
        request.media_path.trim(),
        request.name,
        &request.media_kind,
        request.size_bytes,
    );
    record.item.transcribe_status = "queued".to_string();
    record.item.task_progress = WorkspaceTaskProgressState::default();
    record.item.transcribe_error = String::new();
    record.item.result_text = String::new();
    record.item.result_srt = String::new();
    record.item.subtitle_segments_json = "[]".to_string();
    record.item.llm_total_tokens = 0;

    record.intent = normalize_intent(&request.intent).to_string();
    record.source_lang = request
        .source_lang
        .as_deref()
        .map(normalize_task_source_lang)
        .unwrap_or_else(default_task_source_lang);
    record.target_lang = request
        .target_lang
        .as_deref()
        .map(normalize_task_target_lang)
        .unwrap_or_else(default_task_target_lang);
    record.item.source_lang = record.source_lang.clone();
    record.item.target_lang = record.target_lang.clone();
    record.max_retries = request.max_retries.unwrap_or(0);
    record.settings_snapshot = crate::services::preferences::load_saved_settings_from_default_path()
        .map(|settings| serde_json::to_value(settings).unwrap_or(Value::Null))
        .unwrap_or(Value::Null);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_queue_items_expose_default_task_languages() {
        let item = new_workspace_queue_item(
            "task-1",
            "D:\\media\\demo.mp4",
            "demo.mp4".to_string(),
            "video",
            123,
            "pending",
        );

        assert_eq!(item.source_lang, "en");
        assert_eq!(item.target_lang, "zh-CN");
    }
}
