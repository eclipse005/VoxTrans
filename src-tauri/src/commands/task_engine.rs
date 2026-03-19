use tauri::State;

use crate::app_state::AppState;
use crate::services::task_engine::{
    CancelTaskRequest, EnqueueTaskRequest, GetTaskRunRequest, ListTaskRunsRequest, TaskRunDetail,
    TaskRunRecord, cancel_task as cancel_task_service, enqueue_task as enqueue_task_service,
    get_task_run as get_task_run_service, list_task_runs as list_task_runs_service,
};
use crate::services::task_executor::{
    ExecuteTaskBatchRequest, ExecuteTaskBatchResponse, ExecuteTaskRunRequest,
    execute_task_batch as execute_task_batch_service, execute_task_run as execute_task_run_service,
};

#[tauri::command]
pub async fn enqueue_task_run(
    state: State<'_, AppState>,
    request: EnqueueTaskRequest,
) -> Result<TaskRunRecord, String> {
    enqueue_task_service(&state.pool, request).await
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
pub async fn cancel_task_run(
    state: State<'_, AppState>,
    request: CancelTaskRequest,
) -> Result<TaskRunRecord, String> {
    cancel_task_service(&state.pool, request).await
}

#[tauri::command]
pub async fn execute_task_run(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    request: ExecuteTaskRunRequest,
) -> Result<(), String> {
    execute_task_run_service(&state.pool, app, request).await
}

#[tauri::command]
pub async fn execute_task_batch(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    request: ExecuteTaskBatchRequest,
) -> Result<ExecuteTaskBatchResponse, String> {
    execute_task_batch_service(&state.pool, app, request).await
}
