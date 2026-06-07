use tauri::AppHandle;

use crate::domain::error::WorkspaceResult;

use super::{TaskStage, WorkspaceTaskProgressState, WorkspaceTaskStageState, patch_task_item};

pub(super) fn task_progress_state(
    stage: TaskStage,
    detail: impl Into<String>,
    current: u32,
    total: u32,
) -> WorkspaceTaskProgressState {
    WorkspaceTaskProgressState {
        stage: WorkspaceTaskStageState {
            code: stage.code().to_string(),
            label: stage.label().to_string(),
            order: stage.order(),
            detail: detail.into(),
            current,
            total,
        },
    }
}

pub(super) fn done_task_progress_state() -> WorkspaceTaskProgressState {
    WorkspaceTaskProgressState {
        stage: WorkspaceTaskStageState::default(),
    }
}

pub(super) async fn report_task_stage(
    app: &AppHandle,
    task_id: &str,
    stage: TaskStage,
    detail: impl Into<String>,
    current: u32,
    total: u32,
) -> WorkspaceResult<()> {
    let progress = task_progress_state(stage, detail, current, total);
    patch_task_item(app, task_id, |task| {
        task.item.transcribe_status = "processing".to_string();
        task.item.task_progress = progress;
        task.item.transcribe_error = String::new();
    })
    .await
}

pub(super) async fn mark_task_failed(
    app: &AppHandle,
    task_id: &str,
    error: &str,
) -> WorkspaceResult<()> {
    patch_task_item(app, task_id, |task| {
        task.item.transcribe_status = "error".to_string();
        task.item.task_progress = WorkspaceTaskProgressState::default();
        task.item.transcribe_error = error.to_string();
    })
    .await
}
