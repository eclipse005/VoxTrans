use tauri::State;

use crate::app_state::AppState;
use crate::services::usage::{
    self, GetTaskLlmUsageSummaryRequest, RecordTaskLlmUsageRequest, TaskLlmUsageSummary,
};

#[tauri::command]
pub async fn record_task_llm_usage(
    state: State<'_, AppState>,
    request: RecordTaskLlmUsageRequest,
) -> Result<(), String> {
    usage::record_task_llm_usage(&state.pool, request).await
}

#[tauri::command]
pub async fn get_task_llm_usage_summary(
    state: State<'_, AppState>,
    request: GetTaskLlmUsageSummaryRequest,
) -> Result<TaskLlmUsageSummary, String> {
    usage::get_task_llm_usage_summary(&state.pool, request).await
}
