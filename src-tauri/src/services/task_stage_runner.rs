use serde_json::Value;
use sqlx::SqlitePool;
use std::future::Future;
use std::pin::Pin;

use crate::services::task_context::TaskContext;
use crate::services::task_projection::TaskProjectionState;

fn stage_is_done(status: &str) -> bool {
    status.trim().eq_ignore_ascii_case("done")
}

pub type PersistCallback = for<'a> fn(
    &'a SqlitePool,
    &'a str,
    &'a TaskContext,
    &'a TaskProjectionState,
) -> Pin<Box<dyn Future<Output = Result<(), String>> + 'a>>;

pub async fn run_stage<T, FLoad, FValid, FExec, Fut, FOutput, FMetrics>(
    pool: &SqlitePool,
    task_id: &str,
    context: &mut TaskContext,
    projection: &TaskProjectionState,
    stage: &str,
    load_existing: FLoad,
    validate: FValid,
    exec: FExec,
    output_of: FOutput,
    metrics_of: FMetrics,
    persist: PersistCallback,
) -> Result<T, String>
where
    FLoad: Fn(&TaskContext) -> Option<T>,
    FValid: Fn(&T) -> bool,
    FExec: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<T, String>>,
    FOutput: Fn(&T) -> Value,
    FMetrics: Fn(&T) -> Value,
{
    if stage_is_done(context.stage_status(stage)) {
        if let Some(existing) = load_existing(context) {
            if validate(&existing) {
                return Ok(existing);
            }
        }
    }

    context.mark_stage_running(stage);
    persist(pool, task_id, context, projection).await?;

    let value = match exec().await {
        Ok(v) => v,
        Err(err) => {
            context.mark_failed(stage, "STAGE_FAILED", &err, true);
            persist(pool, task_id, context, projection).await?;
            return Err(err);
        }
    };
    if !validate(&value) {
        let err = format!("{stage} failed: invalid output");
        context.mark_failed(stage, "INVALID_OUTPUT", &err, false);
        persist(pool, task_id, context, projection).await?;
        return Err(err);
    }

    context.mark_stage_done(stage, output_of(&value), metrics_of(&value));
    persist(pool, task_id, context, projection).await?;
    Ok(value)
}
