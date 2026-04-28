use serde_json::Value;
use tauri::AppHandle;

use super::execution_flow::execute_single_task;
use super::meta::{ensure_workspace_hydrated_from_disk, persist_task_meta, remove_task_meta};
use super::task_logs::log_task_failure_to_main;
use super::{
    DeleteTasksCommandRequest, EnqueueTaskRunCommandRequest, ExecuteTaskBatchCommandResponse,
    ExecuteTaskBatchFailedItem, ExecuteTaskRunCommandRequest, RegisterTaskUploadCommandRequest,
    WorkspaceQueueItem, WorkspaceTaskProgressState, WorkspaceTaskRecord, emit_task_state_changed,
    find_task_mut, lock_workspace_store, normalize_intent, normalize_media_kind, patch_task_item,
};

pub(super) fn register_task_upload_internal(
    request: RegisterTaskUploadCommandRequest,
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

pub(super) fn delete_tasks_internal(request: DeleteTasksCommandRequest) -> Result<(), String> {
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
            apply_enqueue_request(existing, request);
            persist_task_meta(existing)?;
            existing.item.clone()
        } else {
            let record = WorkspaceTaskRecord {
                item: new_workspace_queue_item(
                    id,
                    media_path,
                    request.name,
                    &request.media_kind,
                    request.size_bytes,
                    "queued",
                ),
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
        .unwrap_or_else(|| "auto".to_string())
        .trim()
        .to_string();
    record.target_lang = request
        .target_lang
        .unwrap_or_else(|| "zh-CN".to_string())
        .trim()
        .to_string();
    record.max_retries = request.max_retries.unwrap_or(0);
    record.settings_snapshot = request.settings_snapshot.unwrap_or(Value::Null);
}
