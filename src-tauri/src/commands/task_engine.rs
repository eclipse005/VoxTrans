use tauri::State;

use crate::app_state::AppState;
use crate::commands::dto::{TaskRunCommandRecord, from_service_task_run};
use crate::services::task_engine::{
    self, delete_tasks as delete_tasks_service, enqueue_task as enqueue_task_service,
    get_task_run as get_task_run_service, list_task_runs as list_task_runs_service,
    register_task_upload as register_task_upload_service,
};
use crate::services::task_executor::{
    self, enqueue_and_execute_task_batch_via_worker as enqueue_and_execute_task_batch_service,
    execute_task_batch_via_worker as execute_task_batch_service,
    execute_task_run_via_worker as execute_task_run_service,
};
use crate::services::task_worker;

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnqueueTaskCommandRequest {
    pub id: String,
    pub media_path: String,
    pub name: String,
    pub media_kind: String,
    pub size_bytes: u64,
    pub intent: String,
    #[serde(default)]
    pub source_lang: String,
    #[serde(default)]
    pub target_lang: String,
    #[serde(default)]
    pub max_retries: u32,
    #[serde(default)]
    pub settings_snapshot: serde_json::Value,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterTaskUploadCommandRequest {
    pub id: String,
    pub media_path: String,
    pub name: String,
    pub media_kind: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListTaskRunsCommandRequest {
    pub intent: Option<String>,
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetTaskRunCommandRequest {
    pub task_id: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteTasksCommandRequest {
    pub media_path: Option<String>,
    pub task_id: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecuteTaskRunCommandRequest {
    pub task_id: String,
    #[serde(default)]
    pub intent: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecuteTaskBatchItemCommand {
    pub task_id: String,
    #[serde(default)]
    pub intent: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecuteTaskBatchCommandRequest {
    pub items: Vec<ExecuteTaskBatchItemCommand>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnqueueAndExecuteTaskBatchItemCommand {
    pub id: String,
    pub media_path: String,
    pub name: String,
    pub media_kind: String,
    pub size_bytes: u64,
    pub intent: String,
    #[serde(default)]
    pub source_lang: String,
    #[serde(default)]
    pub target_lang: String,
    #[serde(default)]
    pub max_retries: u32,
    #[serde(default)]
    pub settings_snapshot: serde_json::Value,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnqueueAndExecuteTaskBatchCommandRequest {
    pub items: Vec<EnqueueAndExecuteTaskBatchItemCommand>,
}

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

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecuteTaskBatchFailureCommand {
    pub task_id: String,
    pub error: String,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecuteTaskBatchCommandResponse {
    pub succeeded_task_ids: Vec<String>,
    pub failed: Vec<ExecuteTaskBatchFailureCommand>,
}

#[tauri::command]
pub async fn enqueue_task_run(
    state: State<'_, AppState>,
    request: EnqueueTaskCommandRequest,
) -> Result<TaskRunCommandRecord, String> {
    enqueue_task_service(
        &state.pool,
        task_engine::EnqueueTaskRequest {
            id: request.id,
            media_path: request.media_path,
            name: request.name,
            media_kind: request.media_kind,
            size_bytes: request.size_bytes,
            intent: request.intent,
            source_lang: request.source_lang,
            target_lang: request.target_lang,
            max_retries: request.max_retries,
            settings_snapshot: request.settings_snapshot,
        },
    )
    .await
    .map(from_service_task_run)
}

#[tauri::command]
pub async fn register_task_upload(
    state: State<'_, AppState>,
    request: RegisterTaskUploadCommandRequest,
) -> Result<TaskRunCommandRecord, String> {
    register_task_upload_service(
        &state.pool,
        task_engine::RegisterTaskUploadRequest {
            id: request.id,
            media_path: request.media_path,
            name: request.name,
            media_kind: request.media_kind,
            size_bytes: request.size_bytes,
        },
    )
    .await
    .map(from_service_task_run)
}

#[tauri::command]
pub async fn list_task_runs(
    state: State<'_, AppState>,
    request: ListTaskRunsCommandRequest,
) -> Result<Vec<TaskRunCommandRecord>, String> {
    list_task_runs_service(
        &state.pool,
        task_engine::ListTaskRunsRequest {
            intent: request.intent,
            limit: request.limit,
        },
    )
    .await
    .map(|items| items.into_iter().map(from_service_task_run).collect())
}

#[tauri::command]
pub async fn get_task_run(
    state: State<'_, AppState>,
    request: GetTaskRunCommandRequest,
) -> Result<TaskRunDetailCommand, String> {
    get_task_run_service(
        &state.pool,
        task_engine::GetTaskRunRequest {
            task_id: request.task_id,
        },
    )
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
        task_executor::ExecuteTaskRunRequest {
            task_id: request.task_id,
            intent: request.intent,
        },
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
        task_executor::ExecuteTaskBatchRequest {
            items: request
                .items
                .into_iter()
                .map(|item| task_executor::ExecuteTaskBatchItem {
                    task_id: item.task_id,
                    intent: item.intent,
                })
                .collect(),
        },
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
        task_executor::EnqueueAndExecuteTaskBatchRequest {
            items: request
                .items
                .into_iter()
                .map(|item| task_executor::EnqueueAndExecuteTaskBatchItem {
                    id: item.id,
                    media_path: item.media_path,
                    name: item.name,
                    media_kind: item.media_kind,
                    size_bytes: item.size_bytes,
                    intent: item.intent,
                    source_lang: item.source_lang,
                    target_lang: item.target_lang,
                    max_retries: item.max_retries,
                    settings_snapshot: item.settings_snapshot,
                })
                .collect(),
        },
    )
    .await
    .map(from_service_execute_batch_response)
}

#[tauri::command]
pub async fn delete_tasks(
    state: State<'_, AppState>,
    request: DeleteTasksCommandRequest,
) -> Result<(), String> {
    if let Some(task_id) = request.task_id.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
        let _ = task_worker::kill_worker_if_running(&state.task_worker_runtime, task_id);
    }
    delete_tasks_service(
        &state.pool,
        task_engine::DeleteTasksRequest {
            media_path: request.media_path,
            task_id: request.task_id,
        },
    )
    .await
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

fn from_service_execute_batch_response(
    response: task_executor::ExecuteTaskBatchResponse,
) -> ExecuteTaskBatchCommandResponse {
    ExecuteTaskBatchCommandResponse {
        succeeded_task_ids: response.succeeded_task_ids,
        failed: response
            .failed
            .into_iter()
            .map(|item| ExecuteTaskBatchFailureCommand {
                task_id: item.task_id,
                error: item.error,
            })
            .collect(),
    }
}
