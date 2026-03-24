use serde_json::Value;
use sqlx::SqlitePool;

#[derive(Debug, Clone)]
pub struct TaskStageSnapshot {
    pub stage: String,
    pub status: String,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
    pub output: Value,
    pub metrics: Value,
    pub error_code: String,
    pub error_message: String,
}

#[derive(Debug, sqlx::FromRow)]
pub struct TaskStageSnapshotRow {
    pub stage: String,
    pub status: String,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
    pub output_json: String,
    pub metrics_json: String,
    pub error_code: String,
    pub error_message: String,
}

pub async fn load_task_stage_snapshot_rows(
    pool: &SqlitePool,
    task_id: &str,
) -> Result<Vec<TaskStageSnapshotRow>, String> {
    sqlx::query_as::<_, TaskStageSnapshotRow>(
        "SELECT stage, status, started_at, finished_at, output_json, metrics_json, error_code, error_message
         FROM task_stage_runs
         WHERE task_id = ?",
    )
    .bind(task_id)
    .fetch_all(pool)
    .await
    .map_err(|err| err.to_string())
}

pub async fn persist_task_stage_snapshots(
    pool: &SqlitePool,
    task_id: &str,
    snapshots: &[TaskStageSnapshot],
    now: i64,
) -> Result<(), String> {
    for snapshot in snapshots {
        let output_json =
            serde_json::to_string(&snapshot.output).unwrap_or_else(|_| "{}".to_string());
        let metrics_json =
            serde_json::to_string(&snapshot.metrics).unwrap_or_else(|_| "{}".to_string());
        let duration_ms = match (snapshot.started_at, snapshot.finished_at) {
            (Some(start), Some(end)) if end >= start => (end - start) * 1000,
            _ => 0,
        };
        sqlx::query(
            "INSERT INTO task_stage_runs (
                task_id, stage, status, attempt, input_hash, output_json, metrics_json, error_code, error_message,
                started_at, finished_at, duration_ms, updated_at
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(task_id, stage) DO UPDATE SET
                status = excluded.status,
                attempt = CASE
                    WHEN excluded.status = 'running' THEN task_stage_runs.attempt + 1
                    ELSE task_stage_runs.attempt
                END,
                output_json = excluded.output_json,
                metrics_json = excluded.metrics_json,
                error_code = excluded.error_code,
                error_message = excluded.error_message,
                started_at = excluded.started_at,
                finished_at = excluded.finished_at,
                duration_ms = excluded.duration_ms,
                updated_at = excluded.updated_at",
        )
        .bind(task_id)
        .bind(&snapshot.stage)
        .bind(&snapshot.status)
        .bind(if snapshot.status == "running" { 1_i64 } else { 0_i64 })
        .bind("")
        .bind(output_json)
        .bind(metrics_json)
        .bind(&snapshot.error_code)
        .bind(&snapshot.error_message)
        .bind(snapshot.started_at)
        .bind(snapshot.finished_at)
        .bind(duration_ms)
        .bind(now)
        .execute(pool)
        .await
        .map_err(|err| err.to_string())?;
    }
    Ok(())
}
