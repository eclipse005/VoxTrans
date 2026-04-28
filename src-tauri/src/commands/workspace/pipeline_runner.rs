use tauri::AppHandle;

use crate::services::pipeline::{PipelineStep, StepContext, StepExecution, execute_step};

use super::progress::mark_task_failed;

pub(super) async fn execute_workspace_step<S>(
    app: &AppHandle,
    task_id: &str,
    step: &S,
    step_context: &StepContext<'_>,
) -> Result<StepExecution<S::Output>, String>
where
    S: PipelineStep + Send + Sync,
{
    match execute_step(step, step_context).await {
        Ok(value) => Ok(value),
        Err(err) => {
            mark_task_failed(app, task_id, &err)?;
            Err(err)
        }
    }
}
