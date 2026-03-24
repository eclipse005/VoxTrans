use serde_json::Value;
use sqlx::SqlitePool;
use std::future::Future;

use crate::services::task_context::TaskContext;
use crate::services::task_projection::TaskProjectionState;
use crate::services::task_stage_runner::{PersistCallback, run_stage};

pub struct StageHandlers<'a> {
    pool: &'a SqlitePool,
    task_id: &'a str,
    context: &'a mut TaskContext,
    projection: &'a TaskProjectionState,
    persist: PersistCallback,
}

impl<'a> StageHandlers<'a> {
    pub fn new(
        pool: &'a SqlitePool,
        task_id: &'a str,
        context: &'a mut TaskContext,
        projection: &'a TaskProjectionState,
        persist: PersistCallback,
    ) -> Self {
        Self {
            pool,
            task_id,
            context,
            projection,
            persist,
        }
    }

    pub async fn run<T, FLoad, FValid, FExec, Fut, FOutput, FMetrics>(
        &mut self,
        stage: &str,
        load_existing: FLoad,
        validate: FValid,
        exec: FExec,
        output_of: FOutput,
        metrics_of: FMetrics,
    ) -> Result<T, String>
    where
        FLoad: Fn(&TaskContext) -> Option<T>,
        FValid: Fn(&T) -> bool,
        FExec: FnOnce() -> Fut,
        Fut: Future<Output = Result<T, String>>,
        FOutput: Fn(&T) -> Value,
        FMetrics: Fn(&T) -> Value,
    {
        run_stage(
            self.pool,
            self.task_id,
            self.context,
            self.projection,
            stage,
            load_existing,
            validate,
            exec,
            output_of,
            metrics_of,
            self.persist,
        )
        .await
    }
}

