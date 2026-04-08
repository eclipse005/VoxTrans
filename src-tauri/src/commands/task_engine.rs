use tauri::{Emitter, State};

use crate::app_state::AppState;
use crate::commands::dto::common::{TaskRunCommandRecord, from_service_task_run};
use crate::commands::dto::task_engine::{
    DeleteTasksCommandRequest, EnqueueAndExecuteTaskBatchCommandRequest, EnqueueTaskCommandRequest,
    ExecuteTaskBatchCommandRequest, ExecuteTaskBatchCommandResponse, ExecuteTaskRunCommandRequest,
    GetTaskRunCommandRequest, ListTaskRunsCommandRequest, RegisterTaskUploadCommandRequest,
    from_service_execute_batch_response, to_service_delete_tasks,
    to_service_enqueue_and_execute_task_batch, to_service_enqueue_task,
    to_service_execute_task_batch, to_service_execute_task_run, to_service_get_task_run,
    to_service_list_task_runs, to_service_register_task_upload,
};
use crate::services::task_engine::{
    self, delete_tasks as delete_tasks_service, enqueue_task as enqueue_task_service,
    get_task_run as get_task_run_service, list_task_runs as list_task_runs_service,
    register_task_upload as register_task_upload_service,
};
use crate::services::task_executor::{
    TaskStateChangedEvent,
    enqueue_and_execute_task_batch_via_worker as enqueue_and_execute_task_batch_service,
    execute_task_batch_via_worker as execute_task_batch_service,
    execute_task_run_via_worker as execute_task_run_service,
};
use crate::services::task_worker;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskStepRunCommandRecord {
    pub id: i64,
    pub task_id: String,
    pub step: String,
    pub attempt: u32,
    pub status: String,
    pub input_hash: String,
    pub output_json: String,
    pub metrics_json: String,
    pub error_code: String,
    pub error_message: String,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
    pub duration_ms: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskArtifactCommandRecord {
    pub id: i64,
    pub task_id: String,
    pub kind: String,
    pub path: String,
    pub checksum: String,
    pub size_bytes: u64,
    pub produced_by_step: String,
    pub metadata_json: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskRunDetailCommand {
    pub run: TaskRunCommandRecord,
    pub steps: Vec<TaskStepRunCommandRecord>,
    pub artifacts: Vec<TaskArtifactCommandRecord>,
}

/// Build a TaskStateChangedEvent from a TaskRunCommandRecord (used for enqueue events).
fn build_task_state_changed_event(record: &TaskRunCommandRecord) -> TaskStateChangedEvent {
    TaskStateChangedEvent {
        id: record.id.clone(),
        path: record.media_path.clone(),
        name: record.name.clone(),
        media_kind: record.media_kind.clone(),
        size_bytes: record.size_bytes,
        transcribe_status: map_queue_status(&record.overall_status),
        transcribe_progress: record.progress_percent,
        transcribe_segment_current: record.segment_current,
        transcribe_segment_total: record.segment_total,
        transcribe_phase: map_queue_phase(&record.current_stage),
        transcribe_phase_detail: record.phase_detail.clone(),
        transcribe_error: record.error_message.clone(),
        result_text: record.result_text.clone(),
        result_srt: record.result_srt.clone(),
        subtitle_segments_json: record.subtitle_segments_json.clone(),
    }
}

/// Map backend overall_status to frontend transcribe_status.
fn map_queue_status(status: &str) -> String {
    match status.trim().to_lowercase().as_str() {
        "queued" => "queued".to_string(),
        "running" | "processing" => "processing".to_string(),
        "completed" | "done" => "done".to_string(),
        "failed" | "error" => "error".to_string(),
        _ => "pending".to_string(),
    }
}

/// Map backend current_stage to frontend transcribe_phase.
fn map_queue_phase(stage: &str) -> String {
    match stage.trim().to_lowercase().as_str() {
        "separate" => "separating".to_string(),
        "asr" => "recognizing".to_string(),
        "init" | "" => String::new(),
        other => other.to_string(),
    }
}

#[tauri::command]
pub async fn enqueue_task_run(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    request: EnqueueTaskCommandRequest,
) -> Result<TaskRunCommandRecord, String> {
    let record = enqueue_task_service(&state.pool, to_service_enqueue_task(request))
        .await
        .map(from_service_task_run)?;
    // Emit task-state-changed event so frontend can update queue
    let event = build_task_state_changed_event(&record);
    let _ = app.emit("task-state-changed", &event);
    Ok(record)
}

#[tauri::command]
pub async fn register_task_upload(
    state: State<'_, AppState>,
    request: RegisterTaskUploadCommandRequest,
) -> Result<TaskRunCommandRecord, String> {
    register_task_upload_service(&state.pool, to_service_register_task_upload(request))
        .await
        .map(from_service_task_run)
}

#[tauri::command]
pub async fn list_task_runs(
    state: State<'_, AppState>,
    request: ListTaskRunsCommandRequest,
) -> Result<Vec<TaskRunCommandRecord>, String> {
    list_task_runs_service(&state.pool, to_service_list_task_runs(request))
        .await
        .map(|items| items.into_iter().map(from_service_task_run).collect())
}

#[tauri::command]
pub async fn get_task_run(
    state: State<'_, AppState>,
    request: GetTaskRunCommandRequest,
) -> Result<TaskRunDetailCommand, String> {
    get_task_run_service(&state.pool, to_service_get_task_run(request))
        .await
        .map(from_service_task_detail)
}

#[tauri::command]
pub async fn execute_task_run(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    request: ExecuteTaskRunCommandRequest,
) -> Result<(), String> {
    execute_task_run_service(
        &state.pool,
        &state.task_worker_runtime,
        app,
        to_service_execute_task_run(request),
    )
    .await
}

#[tauri::command]
pub async fn execute_task_batch(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    request: ExecuteTaskBatchCommandRequest,
) -> Result<ExecuteTaskBatchCommandResponse, String> {
    execute_task_batch_service(
        &state.pool,
        &state.task_worker_runtime,
        app,
        to_service_execute_task_batch(request),
    )
    .await
    .map(from_service_execute_batch_response)
}

#[tauri::command]
pub async fn enqueue_and_execute_task_batch(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    request: EnqueueAndExecuteTaskBatchCommandRequest,
) -> Result<ExecuteTaskBatchCommandResponse, String> {
    enqueue_and_execute_task_batch_service(
        &state.pool,
        &state.task_worker_runtime,
        app,
        to_service_enqueue_and_execute_task_batch(request),
    )
    .await
    .map(from_service_execute_batch_response)
}

#[tauri::command]
pub async fn delete_tasks(
    state: State<'_, AppState>,
    request: DeleteTasksCommandRequest,
) -> Result<(), String> {
    if let Some(task_id) = request
        .task_id
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        let _ = task_worker::kill_worker_if_running(&state.task_worker_runtime, task_id);
    }
    delete_tasks_service(&state.pool, to_service_delete_tasks(request)).await
}

fn from_service_task_detail(detail: task_engine::TaskRunDetail) -> TaskRunDetailCommand {
    TaskRunDetailCommand {
        run: from_service_task_run(detail.run),
        steps: detail
            .steps
            .into_iter()
            .map(|step| TaskStepRunCommandRecord {
                id: step.id,
                task_id: step.task_id,
                step: step.step,
                attempt: step.attempt,
                status: step.status,
                input_hash: step.input_hash,
                output_json: step.output_json,
                metrics_json: step.metrics_json,
                error_code: step.error_code,
                error_message: step.error_message,
                started_at: step.started_at,
                finished_at: step.finished_at,
                duration_ms: step.duration_ms,
                updated_at: step.updated_at,
            })
            .collect(),
        artifacts: detail
            .artifacts
            .into_iter()
            .map(|artifact| TaskArtifactCommandRecord {
                id: artifact.id,
                task_id: artifact.task_id,
                kind: artifact.kind,
                path: artifact.path,
                checksum: artifact.checksum,
                size_bytes: artifact.size_bytes,
                produced_by_step: artifact.produced_by_step,
                metadata_json: artifact.metadata_json,
                created_at: artifact.created_at,
                updated_at: artifact.updated_at,
            })
            .collect(),
    }
}
