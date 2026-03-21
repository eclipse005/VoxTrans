use tauri::State;

use crate::app_state::AppState;
use crate::services::task_engine::{
    DeleteTasksRequest, EnqueueTaskRequest, GetTaskRunRequest, ListTaskRunsRequest,
    RegisterTaskUploadRequest, TaskRunDetail, TaskRunRecord, delete_tasks as delete_tasks_service,
    enqueue_task as enqueue_task_service, get_task_run as get_task_run_service,
    list_task_runs as list_task_runs_service, register_task_upload as register_task_upload_service,
};
use crate::services::task_executor::{
    EnqueueAndExecuteTaskBatchRequest, ExecuteTaskBatchRequest, ExecuteTaskBatchResponse,
    ExecuteTaskRunRequest,
    enqueue_and_execute_task_batch_via_worker as enqueue_and_execute_task_batch_service,
    execute_task_batch_via_worker as execute_task_batch_service,
    execute_task_run_via_worker as execute_task_run_service,
};
use crate::services::task_worker;

#[tauri::command]
pub async fn enqueue_task_run(
    state: State<'_, AppState>,
    request: EnqueueTaskRequest,
) -> Result<TaskRunRecord, String> {
    enqueue_task_service(&state.pool, request).await
}

#[tauri::command]
pub async fn register_task_upload(
    state: State<'_, AppState>,
    request: RegisterTaskUploadRequest,
) -> Result<TaskRunRecord, String> {
    register_task_upload_service(&state.pool, request).await
}

#[tauri::command]
pub async fn list_task_runs(
    state: State<'_, AppState>,
    request: ListTaskRunsRequest,
) -> Result<Vec<TaskRunRecord>, String> {
    list_task_runs_service(&state.pool, request).await
}

#[tauri::command]
pub async fn get_task_run(
    state: State<'_, AppState>,
    request: GetTaskRunRequest,
) -> Result<TaskRunDetail, String> {
    get_task_run_service(&state.pool, request).await
}

#[tauri::command]
pub async fn execute_task_run(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    request: ExecuteTaskRunRequest,
) -> Result<(), String> {
    execute_task_run_service(&state.pool, &state.task_worker_runtime, app, request).await
}

#[tauri::command]
pub async fn execute_task_batch(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    request: ExecuteTaskBatchRequest,
) -> Result<ExecuteTaskBatchResponse, String> {
    execute_task_batch_service(&state.pool, &state.task_worker_runtime, app, request).await
}

#[tauri::command]
pub async fn enqueue_and_execute_task_batch(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    request: EnqueueAndExecuteTaskBatchRequest,
) -> Result<ExecuteTaskBatchResponse, String> {
    enqueue_and_execute_task_batch_service(&state.pool, &state.task_worker_runtime, app, request).await
}

#[tauri::command]
pub async fn delete_tasks(
    state: State<'_, AppState>,
    request: DeleteTasksRequest,
) -> Result<(), String> {
    if let Some(task_id) = request.task_id.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
        let _ = task_worker::kill_worker_if_running(&state.task_worker_runtime, task_id);
    }
    delete_tasks_service(&state.pool, request).await
}
