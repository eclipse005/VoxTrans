use tauri::{AppHandle, Emitter, Manager};
use crate::db::store::TaskStore;
use crate::domain::error::{WorkspaceError, WorkspaceResult};
use crate::commands::workspace::runtime_settings::FrozenSettings;
use crate::domain::task::stage::TaskStage;

pub mod adapters;
mod execution_flow;
mod log_payload;
mod meta;
mod output_completion;
mod pipeline_runner;
mod pipeline_steps;
mod preview;
mod progress;
mod queue_ops;
mod review_flow;
// Shared with the headless CLI (`crate::cli`), which resolves its settings
// through the same functions so GUI and CLI cannot disagree about what a
// setting means.
pub(crate) mod runtime_settings;
mod store;
mod task_logs;
mod translation_flow;
mod types;

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
    crate::services::app_log::guard_async("workspace", "LOAD_WORKSPACE_STATE_FAILED", async {
        ensure_workspace_hydrated_from_db(&app).await?;
        let store = lock_workspace_store()?;
        Ok(WorkspaceStateResponse {
            queue: store.tasks().iter().map(|task| task.item.clone()).collect(),
        })
    })
    .await
}

#[tauri::command]
pub async fn load_workspace_task(
    app: AppHandle,
    request: LoadWorkspaceTaskCommandRequest,
) -> Result<WorkspaceTaskResponse, String> {
    crate::services::app_log::guard_async("workspace", "LOAD_WORKSPACE_TASK_FAILED", async {
        let task_id = require_task_id(&request.task_id)?;
        ensure_workspace_hydrated_from_db(&app).await?;
        let task = get_task_record(task_id)?;
        Ok(WorkspaceTaskResponse {
            item: task.item.clone(),
        })
    })
    .await
}

#[tauri::command]
pub async fn register_task_upload(
    app: AppHandle,
    request: RegisterTaskUploadCommandRequest,
) -> Result<WorkspaceQueueItem, String> {
    crate::services::app_log::guard_async("workspace", "REGISTER_TASK_UPLOAD_FAILED", async {
        Ok(register_task_upload_internal(&app, request).await?)
    })
    .await
}

#[tauri::command]
pub async fn delete_tasks(
    app: AppHandle,
    request: DeleteTasksCommandRequest,
) -> Result<(), String> {
    crate::services::app_log::guard_async("workspace", "DELETE_TASKS_FAILED", async {
        Ok(delete_tasks_internal(&app, request).await?)
    })
    .await
}

#[tauri::command]
pub async fn enqueue_task_run(
    app: AppHandle,
    request: EnqueueTaskRunCommandRequest,
) -> Result<(), String> {
    crate::services::app_log::guard_async("workspace", "ENQUEUE_TASK_RUN_FAILED", async {
        Ok(enqueue_task_run_internal(&app, request).await?)
    })
    .await
}

#[tauri::command]
pub async fn update_task_languages(
    app: AppHandle,
    request: UpdateTaskLanguagesCommandRequest,
) -> Result<(), String> {
    crate::services::app_log::guard_async("workspace", "UPDATE_TASK_LANGUAGES_FAILED", async {
        Ok(update_task_languages_internal(&app, request).await?)
    })
    .await
}

#[tauri::command]
pub async fn update_task_terminology(
    app: AppHandle,
    request: UpdateTaskTerminologyCommandRequest,
) -> Result<(), String> {
    crate::services::app_log::guard_async("workspace", "UPDATE_TASK_TERMINOLOGY_FAILED", async {
        Ok(update_task_terminology_internal(&app, request).await?)
    })
    .await
}

#[tauri::command]
pub async fn update_task_review_flags(
    app: AppHandle,
    request: review_flow::UpdateTaskReviewFlagsRequest,
) -> Result<WorkspaceQueueItem, String> {
    crate::services::app_log::guard_async("workspace", "UPDATE_TASK_REVIEW_FLAGS_FAILED", async {
        ensure_workspace_hydrated_from_db(&app).await?;
        Ok(review_flow::update_task_review_flags_internal(&app, request).await?)
    })
    .await
}

#[tauri::command]
pub async fn resume_task_after_review(
    app: AppHandle,
    request: review_flow::ResumeTaskAfterReviewRequest,
) -> Result<(), String> {
    crate::services::app_log::guard_async("workspace", "RESUME_TASK_AFTER_REVIEW_FAILED", async {
        ensure_workspace_hydrated_from_db(&app).await?;
        Ok(review_flow::resume_task_after_review_internal(&app, request).await?)
    })
    .await
}

#[tauri::command]
pub async fn save_subtitle_editor(
    app: AppHandle,
    request: SaveSubtitleEditorCommandRequest,
) -> Result<(), String> {
    crate::services::app_log::guard_async("workspace", "SAVE_SUBTITLE_EDITOR_FAILED", async {
        let task_id = require_task_id(&request.task_id)?;
        ensure_workspace_hydrated_from_db(&app).await?;
        let subtitle_segments_json = request.subtitle_segments_json.clone();
        // Persist segments to DB FIRST: if it fails, surface the error and
        // leave the task meta untouched so callers see a consistent state.
        // Doing it in the other order would mark the task with new
        // result_srt + segments_json while the DB segments table silently
        // stays stale -- a restart hydrate then resets segments to empty.
        let store = app.state::<TaskStore>().inner().clone();
        let segments = parse_subtitle_segments_json(&subtitle_segments_json)?;
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
    })
    .await
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BeautifySubtitleEditorRequest {
    pub task_id: String,
    pub subtitle_segments_json: String,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BeautifySubtitleEditorResponse {
    pub subtitle_segments_json: String,
    /// Cues whose timing or translated text actually changed. The editor uses
    /// it to report "fixed N" instead of claiming success blindly.
    pub changed_cues: usize,
}

/// Apply subtitle beautify on demand from the editor.
///
/// Deliberately the *same* function the automatic pipeline runs
/// (`services::subtitle_beautify::beautify_workspace_segments`), so the editor
/// button and the auto flow can never disagree about what beautifying means.
/// The settings toggle does not gate this: it only decides whether the
/// pipeline runs beautify on its own.
pub fn beautify_subtitle_editor_internal(
    app: &AppHandle,
    request: BeautifySubtitleEditorRequest,
) -> WorkspaceResult<BeautifySubtitleEditorResponse> {
    let task_id = require_task_id(&request.task_id)?;
    let mut segments = parse_subtitle_segments_json(&request.subtitle_segments_json)?;
    let store = app.state::<TaskStore>().inner().clone();
    let (_, subtitle_length_preset, target_lang) =
        task_subtitle_beautify_context(&store, task_id)?;

    let before: Vec<(u64, u64, String)> = segments
        .iter()
        .map(|seg| (seg.start_ms, seg.end_ms, seg.translated_text.clone()))
        .collect();
    crate::services::subtitle_beautify::beautify_workspace_segments(
        &mut segments,
        subtitle_length_preset.as_str(),
        &target_lang,
    );
    // The shared transform returns cues sorted by start (the pipeline SoT
    // wants that); the editor folds the response back by position, so undo
    // the sort here or out-of-order drafts would receive neighbours' text.
    let request_starts: Vec<u64> = before.iter().map(|(start, _, _)| *start).collect();
    let segments = restore_display_order(segments, &request_starts);
    let changed_cues = segments
        .iter()
        .zip(before.iter())
        .filter(|(seg, (start, end, translated))| {
            seg.start_ms != *start || seg.end_ms != *end || seg.translated_text != *translated
        })
        .count();

    let subtitle_segments_json = crate::domain::workspace_subtitle::serialize_segments(&segments);
    Ok(BeautifySubtitleEditorResponse {
        subtitle_segments_json,
        changed_cues,
    })
}

/// Map a start-sorted beautify result back onto the request order. Both the
/// transform's sort and this one are stable, so equal starts keep their
/// relative order and the rank mapping is exact.
fn restore_display_order(
    sorted: Vec<crate::domain::workspace_subtitle::WorkspaceSubtitleSegment>,
    request_starts: &[u64],
) -> Vec<crate::domain::workspace_subtitle::WorkspaceSubtitleSegment> {
    let mut order: Vec<usize> = (0..request_starts.len()).collect();
    order.sort_by_key(|&i| request_starts[i]);
    let mut indexed: Vec<(usize, crate::domain::workspace_subtitle::WorkspaceSubtitleSegment)> =
        order
            .into_iter()
            .enumerate()
            .map(|(rank, original)| (original, sorted[rank].clone()))
            .collect();
    indexed.sort_by_key(|(original, _)| *original);
    indexed.into_iter().map(|(_, segment)| segment).collect()
}

#[tauri::command]
pub async fn beautify_subtitle_editor(
    app: AppHandle,
    request: BeautifySubtitleEditorRequest,
) -> Result<BeautifySubtitleEditorResponse, String> {
    crate::services::app_log::guard_async("workspace", "BEAUTIFY_SUBTITLE_EDITOR_FAILED", async {
        ensure_workspace_hydrated_from_db(&app).await?;
        Ok(beautify_subtitle_editor_internal(&app, request)?)
    })
    .await
}

#[tauri::command]
pub async fn execute_task_batch(
    app: AppHandle,
    request: ExecuteTaskBatchCommandRequest,
) -> Result<ExecuteTaskBatchCommandResponse, String> {
    crate::services::app_log::guard_async("workspace", "EXECUTE_TASK_BATCH_FAILED", async {
        Ok(execute_task_batch_internal(&app, request.items).await)
    })
    .await
}

pub fn task_subtitle_beautify_context(
    _store: &TaskStore,
    task_id: &str,
) -> Result<(bool, String, String), String> {
    let record = get_task_record(task_id)?;
    // Read from the frozen settings captured at enqueue time, not the
    // current live settings — subtitle_length_preset and
    // enable_subtitle_beautify are Frozen by design (see CODEBUDDY.md /
    // FrozenSettings docs). Reading live here would make exports use a
    // different beautify/preset than the one that produced the segments.
    Ok((
        record.frozen.enable_subtitle_beautify,
        record.frozen.subtitle_length_preset.as_str().to_string(),
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

/// Parse the editor's segment payload. Malformed JSON must fail the command:
/// defaulting to an empty list here would replace the stored segments with
/// `[]` and silently wipe the user's structured subtitles.
fn parse_subtitle_segments_json(
    subtitle_segments_json: &str,
) -> WorkspaceResult<Vec<crate::domain::workspace_subtitle::WorkspaceSubtitleSegment>> {
    serde_json::from_str(subtitle_segments_json).map_err(|e| {
        WorkspaceError::InvalidRequest(format!("subtitle_segments_json is not valid JSON: {e}"))
    })
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
        "subtitle" => "subtitle",
        _ => "audio",
    }
}

fn normalize_intent(raw: &str) -> &str {
    match raw.trim() {
        "TRANSCRIBE_TRANSLATE" => "TRANSCRIBE_TRANSLATE",
        "TRANSLATE_SRT" => "TRANSLATE_SRT",
        _ => "TRANSCRIBE",
    }
}

fn default_task_source_lang() -> String {
    "en".to_string()
}

fn default_task_target_lang() -> String {
    "zh-CN".to_string()
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
    fn require_task_id_rejects_empty_input_with_invalid_request() {
        let err = require_task_id("  ").expect_err("empty task id should be rejected");

        assert_eq!(err.code(), "INVALID_REQUEST");
        assert_eq!(err.to_string(), "invalid request: taskId is required");
    }

    #[test]
    fn parse_subtitle_segments_rejects_invalid_json() {
        let err = parse_subtitle_segments_json("{not json")
            .expect_err("malformed JSON must fail instead of clearing segments");

        assert_eq!(err.code(), "INVALID_REQUEST");
        assert!(
            err.to_string().contains("subtitle_segments_json"),
            "message must name the payload: {err}"
        );
    }

    #[test]
    fn parse_subtitle_segments_accepts_empty_array() {
        let segments =
            parse_subtitle_segments_json("[]").expect("an explicit empty list must stay valid");

        assert!(segments.is_empty());
    }

    #[test]
    fn display_order_restore_maps_sorted_result_back_onto_request_order() {
        use crate::domain::workspace_subtitle::WorkspaceSubtitleSegment;
        let seg = |start_ms: u64, translated: &str| WorkspaceSubtitleSegment {
            start_ms,
            end_ms: start_ms + 1000,
            source_text: String::new(),
            translated_text: translated.into(),
            source_words: vec![],
            semantic_sentence_id: 0,
        };
        // BONE-BREAK target: returning the sorted vec verbatim hands the
        // editor's positional merge the neighbours' text and timings.
        let request_starts = vec![2000, 0, 1000, 0];
        let sorted = vec![seg(0, "a"), seg(0, "b"), seg(1000, "c"), seg(2000, "d")];
        let restored = restore_display_order(sorted, &request_starts);
        let texts: Vec<&str> = restored.iter().map(|s| s.translated_text.as_str()).collect();
        // Stable ranks: the two zero-start requests keep their relative order.
        assert_eq!(texts, vec!["d", "a", "c", "b"]);
        let starts: Vec<u64> = restored.iter().map(|s| s.start_ms).collect();
        assert_eq!(starts, request_starts);
    }
}
