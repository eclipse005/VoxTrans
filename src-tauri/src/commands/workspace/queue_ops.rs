use tauri::{AppHandle, Manager};

use crate::db::store::TaskStore;
use crate::domain::error::{WorkspaceError, WorkspaceResult};
use crate::domain::task::runtime_settings::FrozenSettings;

use super::execution_flow::execute_single_task;
use super::meta::{ensure_workspace_hydrated_from_db, persist_task_meta, remove_task_meta};
use super::store::{TaskStore as _, lock_workspace_store};
use super::task_logs::log_task_failure_to_main;
use super::{
    DeleteTasksCommandRequest, EnqueueTaskRunCommandRequest, ExecuteTaskBatchCommandResponse,
    ExecuteTaskBatchFailedItem, ExecuteTaskRunCommandRequest, RegisterTaskUploadCommandRequest,
    UpdateTaskLanguagesCommandRequest, UpdateTaskTerminologyCommandRequest, WorkspaceQueueItem,
    WorkspaceTaskProgressState, WorkspaceTaskRecord, default_task_source_lang,
    default_task_target_lang, emit_task_state_changed, normalize_intent, normalize_media_kind,
    normalize_task_source_lang, normalize_task_target_lang, patch_task_item,
};

pub(super) async fn register_task_upload_internal(
    app: &AppHandle,
    request: RegisterTaskUploadCommandRequest,
) -> WorkspaceResult<()> {
    ensure_workspace_hydrated_from_db(app).await?;
    let id = request.id.trim();
    let media_path = request.media_path.trim();
    if id.is_empty() {
        return Err(WorkspaceError::InvalidRequest("id is required".to_string()));
    }
    if media_path.is_empty() {
        return Err(WorkspaceError::InvalidRequest(
            "mediaPath is required".to_string(),
        ));
    }

    let db_store = app.state::<TaskStore>().inner();
    let existing = {
        let store = lock_workspace_store()?;
        store
            .tasks()
            .iter()
            .any(|entry| entry.item.id == id)
    };
    if existing {
        patch_task_item(app, id, |record| {
            apply_upload_fields(
                &mut record.item,
                media_path,
                request.name,
                &request.media_kind,
                request.size_bytes,
            );
        })
        .await?;
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
            frozen: FrozenSettings::default(),
        };
        persist_task_meta(db_store, &record).await?;
        let mut store = lock_workspace_store()?;
        store.push_task(record);
    }

    Ok(())
}

pub(super) async fn delete_tasks_internal(
    app: &AppHandle,
    request: DeleteTasksCommandRequest,
) -> WorkspaceResult<()> {
    ensure_workspace_hydrated_from_db(app).await?;
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

    let db_store = app.state::<TaskStore>().inner();
    let should_delete: Box<dyn Fn(&WorkspaceTaskRecord) -> bool + Send> = if task_id.is_none()
        && media_path.is_none()
    {
        Box::new(|_| true)
    } else {
        Box::new(move |task| {
            task_matches_delete(&task.item, task_id.as_deref(), media_path.as_deref())
        })
    };
    delete_task_records(db_store, should_delete).await
}

pub(super) async fn enqueue_task_run_internal(
    app: &AppHandle,
    request: EnqueueTaskRunCommandRequest,
) -> WorkspaceResult<()> {
    ensure_workspace_hydrated_from_db(app).await?;
    let id = request.id.trim();
    let media_path = request.media_path.trim();
    if id.is_empty() {
        return Err(WorkspaceError::InvalidRequest("id is required".to_string()));
    }
    if media_path.is_empty() {
        return Err(WorkspaceError::InvalidRequest(
            "mediaPath is required".to_string(),
        ));
    }

    let db_store = app.state::<TaskStore>().inner();
    let existing = {
        let store = lock_workspace_store()?;
        store
            .tasks()
            .iter()
            .any(|entry| entry.item.id == id)
    };
    let queued_item = if existing {
        patch_task_item(app, id, |record| {
            apply_enqueue_request(record, request.clone(), db_store);
        })
        .await?;
        let store = lock_workspace_store()?;
        store
            .tasks()
            .iter()
            .find(|entry| entry.item.id == id)
            .map(|task| task.item.clone())
            .ok_or_else(|| WorkspaceError::TaskNotFound(id.to_string()))?
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
        let terminology_group_id = request
            .terminology_group_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .unwrap_or_default();
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
        item.terminology_group_id = terminology_group_id.clone();
        let record = WorkspaceTaskRecord {
            item,
            intent: normalize_intent(&request.intent).to_string(),
            source_lang,
            target_lang,
            max_retries: request.max_retries.unwrap_or(0),
            frozen: freeze_current_settings(db_store, &terminology_group_id),
        };
        let emitted = record.item.clone();
        persist_task_meta(db_store, &record).await?;
        let mut store = lock_workspace_store()?;
        store.push_task(record);
        emitted
    };
    emit_task_state_changed(app, &queued_item);
    Ok(())
}

pub(super) async fn update_task_languages_internal(
    app: &AppHandle,
    request: UpdateTaskLanguagesCommandRequest,
) -> WorkspaceResult<()> {
    ensure_workspace_hydrated_from_db(app).await?;
    let task_id = request.task_id.trim();
    if task_id.is_empty() {
        return Err(WorkspaceError::InvalidRequest(
            "taskId is required".to_string(),
        ));
    }

    {
        let store = lock_workspace_store()?;
        let Some(task) = store
            .tasks()
            .iter()
            .find(|entry| entry.item.id == task_id)
        else {
            return Err(WorkspaceError::TaskNotFound(task_id.to_string()));
        };
        if task.item.transcribe_status == "processing" || task.item.transcribe_status == "queued" {
            return Err(WorkspaceError::TaskBusy);
        }
    }

    let source_lang = normalize_task_source_lang(&request.source_lang);
    let target_lang = normalize_task_target_lang(&request.target_lang);
    patch_task_item(app, task_id, |task| {
        task.source_lang = source_lang.clone();
        task.target_lang = target_lang.clone();
        task.item.source_lang = source_lang;
        task.item.target_lang = target_lang;
    })
    .await?;
    let updated_item = {
        let store = lock_workspace_store()?;
        store
            .tasks()
            .iter()
            .find(|entry| entry.item.id == task_id)
            .map(|task| task.item.clone())
            .ok_or_else(|| WorkspaceError::TaskNotFound(task_id.to_string()))?
    };
    emit_task_state_changed(app, &updated_item);
    Ok(())
}

pub(super) async fn update_task_terminology_internal(
    app: &AppHandle,
    request: UpdateTaskTerminologyCommandRequest,
) -> WorkspaceResult<()> {
    ensure_workspace_hydrated_from_db(app).await?;
    let task_id = request.task_id.trim();
    if task_id.is_empty() {
        return Err(WorkspaceError::InvalidRequest(
            "taskId is required".to_string(),
        ));
    }

    {
        let store = lock_workspace_store()?;
        let Some(task) = store
            .tasks()
            .iter()
            .find(|entry| entry.item.id == task_id)
        else {
            return Err(WorkspaceError::TaskNotFound(task_id.to_string()));
        };
        if task.item.transcribe_status == "processing" || task.item.transcribe_status == "queued" {
            return Err(WorkspaceError::TaskBusy);
        }
    }

    let terminology_group_id = request.terminology_group_id.trim().to_string();
    let db_store = app.state::<TaskStore>().inner().clone();
    let frozen = freeze_current_settings(&db_store, &terminology_group_id);
    // patch_task_item persists the full record (including frozen settings) to DB.
    patch_task_item(app, task_id, |task| {
        task.item.terminology_group_id = terminology_group_id.clone();
        task.frozen.terminology_groups = frozen.terminology_groups.clone();
    })
    .await?;
    let updated_item = {
        let store = lock_workspace_store()?;
        store
            .tasks()
            .iter()
            .find(|entry| entry.item.id == task_id)
            .map(|task| task.item.clone())
            .ok_or_else(|| WorkspaceError::TaskNotFound(task_id.to_string()))?
    };
    emit_task_state_changed(app, &updated_item);
    Ok(())
}

pub(super) async fn execute_task_batch_internal(
    app: &AppHandle,
    items: Vec<ExecuteTaskRunCommandRequest>,
) -> ExecuteTaskBatchCommandResponse {
    if let Err(err) = ensure_workspace_hydrated_from_db(app).await {
        return failed_task_response_for_requests(items, &err);
    }

    let mut response = ExecuteTaskBatchCommandResponse {
        succeeded_task_ids: Vec::new(),
        failed: Vec::new(),
    };

    for request in items {
        let task_id = request.task_id.trim().to_string();
        if task_id.is_empty() {
            response.failed.push(failed_task_item(
                task_id,
                &WorkspaceError::InvalidRequest("taskId is required".to_string()),
            ));
            continue;
        }

        if let Some(intent) = request.intent.as_deref() {
            if let Err(err) = patch_task_item(app, &task_id, |record| {
                record.intent = normalize_intent(intent).to_string();
            })
            .await
            {
                log_task_failure_to_main(&task_id, &err.to_string());
                response.failed.push(failed_task_item(task_id, &err));
                continue;
            }
        }

        match execute_single_task(app, &task_id).await {
            Ok(()) => response.succeeded_task_ids.push(task_id),
            Err(err) => {
                log_task_failure_to_main(&task_id, &err.to_string());
                response.failed.push(failed_task_item(task_id, &err));
            }
        }
    }

    response
}

fn failed_task_response_for_requests(
    items: Vec<ExecuteTaskRunCommandRequest>,
    error: &WorkspaceError,
) -> ExecuteTaskBatchCommandResponse {
    let failed = items
        .into_iter()
        .map(|request| {
            let task_id = request.task_id.trim().to_string();
            if task_id.is_empty() {
                return failed_task_item(
                    task_id,
                    &WorkspaceError::InvalidRequest("taskId is required".to_string()),
                );
            }
            failed_task_item(task_id, error)
        })
        .collect();

    ExecuteTaskBatchCommandResponse {
        succeeded_task_ids: Vec::new(),
        failed,
    }
}

fn failed_task_item(task_id: String, error: &WorkspaceError) -> ExecuteTaskBatchFailedItem {
    ExecuteTaskBatchFailedItem {
        task_id,
        error: error.to_command_error(),
    }
}

async fn delete_task_records(
    db_store: &TaskStore,
    should_delete: Box<dyn Fn(&WorkspaceTaskRecord) -> bool + Send>,
) -> WorkspaceResult<()> {
    let removed: Vec<WorkspaceQueueItem> = {
        let store = lock_workspace_store()?;
        store
            .tasks()
            .iter()
            .filter(|task| should_delete(task))
            .map(|task| task.item.clone())
            .collect()
    };

    if removed.iter().any(delete_is_blocked_by_task_state) {
        return Err(WorkspaceError::TaskBusy);
    }

    for item in &removed {
        remove_task_meta(db_store, item).await?;
    }

    {
        let mut store = lock_workspace_store()?;
        store.retain_tasks(|task| !should_delete(task));
    }
    Ok(())
}

fn task_matches_delete(
    item: &WorkspaceQueueItem,
    task_id: Option<&str>,
    media_path: Option<&str>,
) -> bool {
    let task_match = task_id.map(|id| item.id == id).unwrap_or(false);
    let media_match = media_path.map(|path| item.path == path).unwrap_or(false);
    task_match || media_match
}

fn delete_is_blocked_by_task_state(item: &WorkspaceQueueItem) -> bool {
    let busy = item.transcribe_status == "processing" || item.transcribe_status == "queued";
    busy && !is_youtube_placeholder_path(&item.path)
}

fn is_youtube_placeholder_path(path: &str) -> bool {
    path.trim().starts_with("youtube://pending/")
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
        terminology_group_id: String::new(),
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

fn apply_enqueue_request(
    record: &mut WorkspaceTaskRecord,
    request: EnqueueTaskRunCommandRequest,
    db_store: &TaskStore,
) {
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
    let terminology_group_id = request
        .terminology_group_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_default();
    record.item.source_lang = record.source_lang.clone();
    record.item.target_lang = record.target_lang.clone();
    record.item.terminology_group_id = terminology_group_id.clone();
    record.max_retries = request.max_retries.unwrap_or(0);
    record.frozen = freeze_current_settings(db_store, &terminology_group_id);
}

/// Snapshot the user-frozen subset of the current saved settings. Tasks
/// keep their own copy so subtitle-shape and terminology decisions stay
/// consistent across the whole run even if the user edits settings
/// mid-pipeline.
fn freeze_current_settings(db_store: &TaskStore, selected_group_id: &str) -> FrozenSettings {
    crate::services::preferences::load_saved_settings_from_default_path(db_store)
        .map(|saved| FrozenSettings::from_saved(&saved, selected_group_id))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::super::store::WorkspaceStore;
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

    #[test]
    fn failed_task_item_serializes_workspace_error_code() {
        let item = failed_task_item(
            "task-1".to_string(),
            &WorkspaceError::TaskFailed("missing runtime settings".to_string()),
        );
        let value: serde_json::Value =
            serde_json::from_str(&item.error).expect("structured queue error");

        assert_eq!(value["code"], "TASK_FAILED");
        assert_eq!(value["message"], "task failed: missing runtime settings");
    }

    #[test]
    fn failed_task_response_for_requests_preserves_hydration_error_code() {
        let response = failed_task_response_for_requests(
            vec![ExecuteTaskRunCommandRequest {
                task_id: " task-1 ".to_string(),
                intent: None,
            }],
            &WorkspaceError::Io(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "cannot hydrate workspace",
            )),
        );
        let value: serde_json::Value =
            serde_json::from_str(&response.failed[0].error).expect("structured queue error");

        assert!(response.succeeded_task_ids.is_empty());
        assert_eq!(response.failed[0].task_id, "task-1");
        assert_eq!(value["code"], "IO_ERROR");
    }

    #[test]
    fn delete_task_records_rejects_busy_local_pipeline_tasks() {
        let mut store = WorkspaceStore::default();
        let mut record = test_record("task-1", "D:\\media\\demo.mp4");
        record.item.transcribe_status = "processing".to_string();
        store.push_task(record);

        // The DB delete path is exercised in integration; here we verify the
        // pure in-memory busy-state guard via a sync variant.
        let removed: Vec<WorkspaceQueueItem> = store
            .tasks()
            .iter()
            .filter(|_| true)
            .map(|task| task.item.clone())
            .collect();
        let blocked = removed
            .iter()
            .any(delete_is_blocked_by_task_state);
        assert!(blocked);
    }

    fn test_record(task_id: &str, media_path: &str) -> WorkspaceTaskRecord {
        WorkspaceTaskRecord {
            item: new_workspace_queue_item(
                task_id,
                media_path,
                "demo.mp4".to_string(),
                "video",
                1,
                "pending",
            ),
            intent: "TRANSCRIBE".to_string(),
            source_lang: "en".to_string(),
            target_lang: "zh-CN".to_string(),
            max_retries: 0,
            frozen: FrozenSettings::default(),
        }
    }
}
