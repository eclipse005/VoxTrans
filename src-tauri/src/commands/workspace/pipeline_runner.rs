use crate::db::store::TaskStore;
use crate::domain::error::{WorkspaceError, WorkspaceResult};
use crate::services::pipeline::{PipelineStep, StepContext, StepExecution, execute_step};

pub(super) async fn execute_workspace_step<S>(
    step: &S,
    step_context: &StepContext<'_>,
    store: &TaskStore,
) -> WorkspaceResult<StepExecution<S::Output>>
where
    S: PipelineStep + Send + Sync,
{
    execute_step(step, step_context, store)
        .await
        .map_err(workspace_step_error)
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
