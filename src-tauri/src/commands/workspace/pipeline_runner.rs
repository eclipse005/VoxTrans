use crate::db::store::TaskStore;
use crate::domain::error::{WorkspaceError, WorkspaceResult};
use crate::domain::pipeline::{PipelineStep, StepContext, StepExecution, execute_step};

pub(super) async fn execute_workspace_step<S>(
    step: &S,
    step_context: &StepContext<'_>,
    store: &TaskStore,
) -> WorkspaceResult<StepExecution<S::Output>>
where
    S: PipelineStep + Send + Sync,
{
    let execution = execute_step(step, step_context, store)
        .await
        .map_err(workspace_step_error)?;

    // The inner layer reports a discarded artifact instead of logging it (it must
    // not depend outward), so recording it is this layer's job. A recompute is
    // correct but wasteful, and it is the only symptom of schema drift — a deploy
    // that changed a step's output without bumping the artifact key.
    if let Some(reason) = execution.cache_discarded.as_deref() {
        crate::services::app_log::error(
            "pipeline.artifact",
            "ARTIFACT_CACHE_DISCARDED",
            reason,
            Some(serde_json::json!({
                "taskId": step_context.task_id,
                "step": step.name(),
            })),
        );
    }

    Ok(execution)
}

fn workspace_step_error(err: String) -> WorkspaceError {
    WorkspaceError::TaskFailed(err)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_errors_are_returned_for_outer_failure_owner() {
        let err = workspace_step_error("step failed".to_string());

        assert_eq!(err.code(), "TASK_FAILED");
        assert_eq!(err.to_string(), "task failed: step failed");
    }
}
