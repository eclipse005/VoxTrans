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
    normalize_task_target_lang, patch_task_item,
};

pub(super) async fn register_task_upload_internal(
    app: &AppHandle,
    request: RegisterTaskUploadCommandRequest,
) -> WorkspaceResult<WorkspaceQueueItem> {
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

    let is_srt = crate::services::subtitle_import::is_srt_path(media_path)
        || normalize_media_kind(&request.media_kind) == "subtitle";

    let (resolved_path, media_kind, size_bytes, segments_json, result_text, result_srt) =
        if is_srt {
            let imported = crate::services::subtitle_import::import_srt_for_task(
                id,
                std::path::Path::new(media_path),
                &request.name,
            )
            .map_err(WorkspaceError::InvalidRequest)?;
            (
                imported.path,
                "subtitle".to_string(),
                imported.size_bytes,
                imported.subtitle_segments_json,
                imported.result_text,
                imported.result_srt,
            )
        } else {
            (
                media_path.to_string(),
                normalize_media_kind(&request.media_kind).to_string(),
                request.size_bytes,
                "[]".to_string(),
                String::new(),
                String::new(),
            )
        };

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
                &resolved_path,
                request.name.clone(),
                &media_kind,
                size_bytes,
            );
            if is_srt {
                record.item.subtitle_segments_json = segments_json.clone();
                record.item.result_text = result_text.clone();
                record.item.result_srt = result_srt.clone();
                record.intent = "TRANSLATE_SRT".to_string();
            }
        })
        .await?;
    } else {
        let enqueue_seq = db_store.next_enqueue_seq().await?;
        let review_defaults = db_store
            .load_settings()
            .await
            .map(|s| (s.default_review_source, s.default_review_target))
            .unwrap_or((false, false));
        let mut item = new_workspace_queue_item(
            id,
            &resolved_path,
            request.name,
            &media_kind,
            size_bytes,
            "pending",
        );
        item.review_source = review_defaults.0;
        item.review_target = review_defaults.1;
        if is_srt {
            item.subtitle_segments_json = segments_json;
            item.result_text = result_text;
            item.result_srt = result_srt;
        }
        let record = WorkspaceTaskRecord {
            item,
            intent: if is_srt {
                "TRANSLATE_SRT".to_string()
            } else {
                "TRANSCRIBE".to_string()
            },
            source_lang: default_task_source_lang(),
            target_lang: default_task_target_lang(),
            max_retries: 0,
            frozen: FrozenSettings::default(),
            enqueue_seq,
        };
        persist_task_meta(db_store, &record).await?;
        let mut store = lock_workspace_store()?;
        store.push_task(record);
    }

    // Persist SRT segments to the structured table so hydrate reloads them.
    if is_srt {
        let store = app.state::<TaskStore>().inner().clone();
        let task_id = id.to_string();
        let item_snapshot = {
            let store_mem = lock_workspace_store()?;
            store_mem
                .tasks()
                .iter()
                .find(|entry| entry.item.id == id)
                .map(|task| task.item.clone())
                .ok_or_else(|| WorkspaceError::TaskNotFound(id.to_string()))?
        };
        let segs: Vec<crate::services::workspace_subtitle::WorkspaceSubtitleSegment> =
            serde_json::from_str(&item_snapshot.subtitle_segments_json).unwrap_or_default();
        if !segs.is_empty() {
            store
                .replace_segments(&task_id, &segs)
                .await
                .map_err(|e| WorkspaceError::TaskFailed(format!("persist srt segments: {e}")))?;
        }
    }

    let item = {
        let store = lock_workspace_store()?;
        store
            .tasks()
            .iter()
            .find(|entry| entry.item.id == id)
            .map(|task| task.item.clone())
            .ok_or_else(|| WorkspaceError::TaskNotFound(id.to_string()))?
    };
    emit_task_state_changed(app, &item);
    Ok(item)
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
        // Full re-queue clears non-SRT SoT — never allowed while awaiting review.
        {
            let store = lock_workspace_store()?;
            if let Some(task) = store.tasks().iter().find(|entry| entry.item.id == id) {
                super::review_flow::reject_full_run_if_awaiting_review(
                    &task.item.transcribe_status,
                )?;
            }
        }
        let terminology_group_id = request
            .terminology_group_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .unwrap_or_default();
        let frozen = freeze_current_settings(db_store, &terminology_group_id)?;
        patch_task_item(app, id, |record| {
            apply_enqueue_request(record, request.clone(), frozen);
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
            .map(str::to_string)
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
        let intent = normalize_intent(&request.intent).to_string();
        let is_srt = intent == "TRANSLATE_SRT"
            || normalize_media_kind(&request.media_kind) == "subtitle"
            || crate::services::subtitle_import::is_srt_path(media_path);

        let mut item = if is_srt {
            // Cold path: still materialize task-dir copy + parse cues.
            let imported = crate::services::subtitle_import::import_srt_for_task(
                id,
                std::path::Path::new(media_path),
                &request.name,
            )
            .map_err(WorkspaceError::InvalidRequest)?;
            let mut item = new_workspace_queue_item(
                id,
                &imported.path,
                request.name.clone(),
                "subtitle",
                imported.size_bytes,
                "queued",
            );
            item.subtitle_segments_json = imported.subtitle_segments_json;
            item.result_text = imported.result_text;
            item.result_srt = imported.result_srt;
            item
        } else {
            new_workspace_queue_item(
                id,
                media_path,
                request.name.clone(),
                &request.media_kind,
                request.size_bytes,
                "queued",
            )
        };
        item.source_lang = source_lang.clone();
        item.target_lang = target_lang.clone();
        item.terminology_group_id = terminology_group_id.clone();
        let review_defaults = db_store
            .load_settings()
            .await
            .map(|s| (s.default_review_source, s.default_review_target))
            .unwrap_or((false, false));
        item.review_source = review_defaults.0;
        item.review_target = review_defaults.1;
        let mut frozen = freeze_current_settings(db_store, &terminology_group_id)?;
        if is_srt {
            frozen.enable_subtitle_beautify = false;
        }
        let enqueue_seq = db_store.next_enqueue_seq().await?;
        let record = WorkspaceTaskRecord {
            item,
            intent: if is_srt {
                "TRANSLATE_SRT".to_string()
            } else {
                intent
            },
            source_lang,
            target_lang,
            max_retries: request.max_retries.unwrap_or(0),
            frozen,
            enqueue_seq,
        };
        let emitted = record.item.clone();
        persist_task_meta(db_store, &record).await?;
        let mut store = lock_workspace_store()?;
        store.push_task(record);
        emitted
    };

    // Keep structured segments table aligned with cleared-translation JSON so
    // hydrate after restart does not resurrect old translations.
    if queued_item.media_kind.trim() == "subtitle"
        || crate::services::subtitle_import::is_srt_path(&queued_item.path)
    {
        let segs: Vec<crate::services::workspace_subtitle::WorkspaceSubtitleSegment> =
            serde_json::from_str(&queued_item.subtitle_segments_json).unwrap_or_default();
        if !segs.is_empty() {
            db_store
                .replace_segments(id, &segs)
                .await
                .map_err(|e| {
                    WorkspaceError::TaskFailed(format!("persist srt segments on enqueue: {e}"))
                })?;
        }
    }

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
        if task.item.transcribe_status == "processing" {
            return Err(WorkspaceError::TaskBusy);
        }
    }

    let source_lang = request.source_lang;
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
        if task.item.transcribe_status == "processing" {
            return Err(WorkspaceError::TaskBusy);
        }
    }

    let terminology_group_id = request.terminology_group_id.trim().to_string();
    let db_store = app.state::<TaskStore>().inner().clone();
    let frozen = freeze_current_settings(&db_store, &terminology_group_id)?;
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
    // Only 'processing' is truly busy (a runner thread owns it). 'queued' is
    // just "in the memory queue" and can be removed at any time — especially
    // after a restart when the queue is empty but the DB still shows 'queued'.
    let busy = item.transcribe_status == "processing";
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
        review_source: false,
        review_target: false,
        resume_from: String::new(),
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

/// Explicit start / redo: may clear generated outputs for non-SRT media.
/// Must not be called for `review_*` (caller rejects first).
fn apply_enqueue_request(
    record: &mut WorkspaceTaskRecord,
    request: EnqueueTaskRunCommandRequest,
    frozen: FrozenSettings,
) {
    let intent = normalize_intent(&request.intent).to_string();
    let media_kind = normalize_media_kind(&request.media_kind).to_string();
    let is_srt_task = intent == "TRANSLATE_SRT" || media_kind == "subtitle";

    // Sole intentional wipe of media-task SoT before a full re-run.
    // SRT translate keeps source cues and only clears translations.
    let preserved_segments = if is_srt_task {
        clear_translations_in_segments_json(&record.item.subtitle_segments_json)
    } else {
        "[]".to_string()
    };
    let preserved_result_text = if is_srt_task {
        record.item.result_text.clone()
    } else {
        String::new()
    };
    let preserved_result_srt = if is_srt_task {
        record.item.result_srt.clone()
    } else {
        String::new()
    };
    let path = if is_srt_task && !record.item.path.trim().is_empty() {
        // Prefer the already-copied task-dir original (or renamed-if-reserved) over upload path.
        record.item.path.clone()
    } else {
        request.media_path.trim().to_string()
    };

    apply_upload_fields(
        &mut record.item,
        &path,
        request.name,
        if is_srt_task { "subtitle" } else { &request.media_kind },
        request.size_bytes,
    );
    record.item.transcribe_status = "queued".to_string();
    record.item.task_progress = WorkspaceTaskProgressState::default();
    record.item.transcribe_error = String::new();
    record.item.result_text = preserved_result_text;
    record.item.result_srt = preserved_result_srt;
    record.item.subtitle_segments_json = preserved_segments;
    record.item.llm_total_tokens = 0;
    // Full re-run always starts from the top of the pipeline.
    record.item.resume_from = String::new();

    record.intent = if is_srt_task {
        "TRANSLATE_SRT".to_string()
    } else {
        intent
    };
    record.source_lang = request
        .source_lang
        .as_deref()
        .map(str::to_string)
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
    // SRT tasks must not reshape cue boundaries via beautify.
    let mut frozen = frozen;
    if is_srt_task {
        frozen.enable_subtitle_beautify = false;
    }
    record.frozen = frozen;
}

fn clear_translations_in_segments_json(raw: &str) -> String {
    let mut segments: Vec<crate::services::workspace_subtitle::WorkspaceSubtitleSegment> =
        serde_json::from_str(raw).unwrap_or_default();
    for segment in &mut segments {
        segment.translated_text.clear();
    }
    crate::services::workspace_subtitle::serialize_segments(&segments)
}

/// Snapshot the user-frozen subset of the current saved settings. Tasks
/// keep their own copy so subtitle-shape and terminology decisions stay
/// consistent across the whole run even if the user edits settings
/// mid-pipeline.
fn freeze_current_settings(
    db_store: &TaskStore,
    selected_group_id: &str,
) -> Result<FrozenSettings, String> {
    let saved = crate::services::preferences::load_saved_settings_from_default_path(db_store)?;
    Ok(FrozenSettings::from_saved(&saved, selected_group_id))
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
            enqueue_seq: 0,
        }
    }
}
