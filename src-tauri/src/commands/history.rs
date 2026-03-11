use tauri::State;

use crate::app_state::AppState;
use crate::services::history::{
    self, ClearTaskEventsRequest, DeleteTaskSummariesRequest, ListTaskEventsRequest,
    ListTaskSummariesRequest, RecordTaskEventRequest, TaskEventRecord, TaskSummary,
};

#[tauri::command]
pub async fn record_task_event(
    state: State<'_, AppState>,
    request: RecordTaskEventRequest,
) -> Result<(), String> {
    history::record_task_event(&state.pool, request).await
}

#[tauri::command]
pub async fn list_task_events(
    state: State<'_, AppState>,
    request: ListTaskEventsRequest,
) -> Result<Vec<TaskEventRecord>, String> {
    history::list_task_events(&state.pool, request).await
}

#[tauri::command]
pub async fn list_task_summaries(
    state: State<'_, AppState>,
    request: ListTaskSummariesRequest,
) -> Result<Vec<TaskSummary>, String> {
    history::list_task_summaries(&state.pool, request).await
}

#[tauri::command]
pub async fn clear_task_events(
    state: State<'_, AppState>,
    request: ClearTaskEventsRequest,
) -> Result<(), String> {
    history::clear_task_events(&state.pool, request).await
}

#[tauri::command]
pub async fn delete_task_summaries(
    state: State<'_, AppState>,
    request: DeleteTaskSummariesRequest,
) -> Result<(), String> {
    history::delete_task_summaries(&state.pool, request).await
}
