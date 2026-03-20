use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::SqlitePool;
use crate::services::task_context::{TaskContext, TaskContextSeed};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskSnapshot {
    pub id: String,
    pub media_path: String,
    pub name: String,
    pub media_kind: String,
    pub size_bytes: u64,
    pub last_status: String,
    pub last_error: String,
    pub output_srt_path: String,
    pub output_words_json: String,
    #[serde(default)]
    pub transcribe_status: String,
    #[serde(default)]
    pub transcribe_error: String,
    #[serde(default)]
    pub transcript_srt: String,
    #[serde(default)]
    pub subtitle_segments_json: String,
    #[serde(default)]
    pub transcribed_at: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordTaskEventRequest {
    pub task_id: Option<String>,
    pub event_type: String,
    pub payload: Option<Value>,
    pub task: Option<TaskSnapshot>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskEventRecord {
    pub id: i64,
    pub task_id: Option<String>,
    pub event_type: String,
    pub payload: Value,
    pub created_at: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListTaskEventsRequest {
    pub task_id: Option<String>,
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListTaskSummariesRequest {
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClearTaskEventsRequest {
    pub task_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteTaskSummariesRequest {
    pub media_path: Option<String>,
    pub task_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskSummary {
    pub id: String,
    pub media_path: String,
    pub name: String,
    pub media_kind: String,
    pub size_bytes: u64,
    pub last_status: String,
    pub last_error: String,
    pub output_srt_path: String,
    pub output_words_json: String,
    pub context_json: String,
    pub transcribed_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, sqlx::FromRow)]
struct TaskSummaryRow {
    id: String,
    media_path: String,
    name: String,
    media_kind: String,
    size_bytes: i64,
    last_status: String,
    last_error: String,
    output_srt_path: String,
    output_words_json: String,
    context_json: String,
    transcribed_at: Option<i64>,
    created_at: i64,
    updated_at: i64,
}

pub async fn record_task_event(
    pool: &SqlitePool,
    request: RecordTaskEventRequest,
) -> Result<(), String> {
    if request.event_type.trim().is_empty() {
        return Err("eventType is required".to_string());
    }

    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;

    if let Some(task) = request.task {
        let transcribe_status =
            non_empty_or_default(task.transcribe_status.clone(), &task.last_status);
        let transcribe_error = non_empty_or_default(task.transcribe_error.clone(), &task.last_error);
        let context_json = build_tasks_context_json_from_snapshot(&task, &transcribe_status, &transcribe_error)?;

        sqlx::query(
            "INSERT INTO tasks (
               id, media_path, name, media_kind, size_bytes, last_status, last_error,
               output_srt_path, output_words_json,
               context_json, transcribed_at,
               created_at, updated_at
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, strftime('%s','now'), strftime('%s','now'))
             ON CONFLICT(id) DO UPDATE SET
               media_path = excluded.media_path,
               name = excluded.name,
               media_kind = excluded.media_kind,
               size_bytes = excluded.size_bytes,
               last_status = excluded.last_status,
               last_error = excluded.last_error,
               output_srt_path = excluded.output_srt_path,
               output_words_json = excluded.output_words_json,
               context_json = excluded.context_json,
               transcribed_at = excluded.transcribed_at,
               updated_at = excluded.updated_at",
        )
        .bind(&task.id)
        .bind(&task.media_path)
        .bind(&task.name)
        .bind(&task.media_kind)
        .bind(task.size_bytes as i64)
        .bind(&task.last_status)
        .bind(&task.last_error)
        .bind(&task.output_srt_path)
        .bind(&task.output_words_json)
        .bind(context_json)
        .bind(task.transcribed_at)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;

        sync_task_run_from_snapshot(&mut tx, &task, &transcribe_status, &transcribe_error).await?;
    }

    let payload = request.payload.unwrap_or_else(|| serde_json::json!({}));
    let payload_str = serde_json::to_string(&payload).map_err(|e| e.to_string())?;

    sqlx::query(
        "INSERT INTO task_events (task_id, event_type, payload_json, created_at)
         VALUES (?, ?, ?, strftime('%s','now'))",
    )
    .bind(request.task_id)
    .bind(request.event_type)
    .bind(payload_str)
    .execute(&mut *tx)
    .await
    .map_err(|e| e.to_string())?;

    tx.commit().await.map_err(|e| e.to_string())
}

async fn sync_task_run_from_snapshot(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    task: &TaskSnapshot,
    transcribe_status: &str,
    transcribe_error: &str,
) -> Result<(), String> {
    let now = now_unix();
    let finished_at = if matches!(transcribe_status, "done" | "error") {
        task.transcribed_at.or(Some(now))
    } else {
        None
    };
    let context_json = build_context_json_from_snapshot(task, transcribe_status, transcribe_error, now)?;

    sqlx::query(
        "INSERT INTO task_runs (
            id, media_path, name, media_kind, size_bytes,
            intent, retry_count, max_retries,
            settings_policy_version, settings_snapshot_json,
            source_lang, target_lang, queued_at, started_at, finished_at, created_at, updated_at, sort_order, context_json
         ) VALUES (?, ?, ?, ?, ?, 'TRANSCRIBE', 0, 0, 'v1', '{}', 'auto', 'zh-CN', ?, NULL, ?, ?, ?, 0, ?)
         ON CONFLICT(id) DO UPDATE SET
            media_path = excluded.media_path,
            name = excluded.name,
            media_kind = excluded.media_kind,
            size_bytes = excluded.size_bytes,
            finished_at = excluded.finished_at,
            updated_at = excluded.updated_at,
            context_json = excluded.context_json",
    )
    .bind(&task.id)
    .bind(&task.media_path)
    .bind(&task.name)
    .bind(&task.media_kind)
    .bind(task.size_bytes as i64)
    .bind(task.transcribed_at.unwrap_or(now))
    .bind(finished_at)
    .bind(now)
    .bind(now)
    .bind(context_json)
    .execute(&mut **tx)
    .await
    .map_err(|err| err.to_string())?;

    Ok(())
}

fn build_context_json_from_snapshot(
    task: &TaskSnapshot,
    transcribe_status: &str,
    transcribe_error: &str,
    created_at: i64,
) -> Result<String, String> {
    let mut context = TaskContext::new(TaskContextSeed {
        task_id: task.id.clone(),
        intent: "TRANSCRIBE".to_string(),
        source_lang: "auto".to_string(),
        target_lang: "zh-CN".to_string(),
        media_path: task.media_path.clone(),
        media_kind: task.media_kind.clone(),
        media_size_bytes: task.size_bytes,
        settings_snapshot: serde_json::json!({}),
        created_at,
    });
    let (runtime_status, progress_percent) = match transcribe_status.trim() {
        "pending" => ("created", 0),
        "queued" => ("queued", 0),
        "processing" => ("running", 50),
        "done" => ("completed", 100),
        "error" => ("failed", 0),
        _ => ("created", 0),
    };
    context.runtime.status = runtime_status.to_string();
    context.runtime.progress_percent = progress_percent;
    context.set_queue_projection(
        transcribe_status,
        "",
        "",
        progress_percent,
        0,
        0,
        transcribe_error,
    );
    context.set_editor_projection(
        task.subtitle_segments_json.clone(),
        String::new(),
        task.transcript_srt.clone(),
        String::new(),
    );
    context.to_json_string()
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub async fn list_task_events(
    pool: &SqlitePool,
    request: ListTaskEventsRequest,
) -> Result<Vec<TaskEventRecord>, String> {
    let limit = request.limit.unwrap_or(100).clamp(1, 1000) as i64;
    let rows = if let Some(task_id) = request.task_id {
        sqlx::query_as::<_, (i64, Option<String>, String, String, i64)>(
            "SELECT id, task_id, event_type, payload_json, created_at
             FROM task_events
             WHERE task_id = ?
             ORDER BY created_at DESC, id DESC
             LIMIT ?",
        )
        .bind(task_id)
        .bind(limit)
        .fetch_all(pool)
        .await
        .map_err(|e| e.to_string())?
    } else {
        sqlx::query_as::<_, (i64, Option<String>, String, String, i64)>(
            "SELECT id, task_id, event_type, payload_json, created_at
             FROM task_events
             ORDER BY created_at DESC, id DESC
             LIMIT ?",
        )
        .bind(limit)
        .fetch_all(pool)
        .await
        .map_err(|e| e.to_string())?
    };

    Ok(rows
        .into_iter()
        .map(
            |(id, task_id, event_type, payload_json, created_at)| TaskEventRecord {
                id,
                task_id,
                event_type,
                payload: serde_json::from_str(&payload_json)
                    .unwrap_or_else(|_| serde_json::json!({ "raw": payload_json })),
                created_at,
            },
        )
        .collect())
}

pub async fn list_task_summaries(
    pool: &SqlitePool,
    request: ListTaskSummariesRequest,
) -> Result<Vec<TaskSummary>, String> {
    let limit = request.limit.unwrap_or(200).clamp(1, 2000) as i64;
    let rows = sqlx::query_as::<_, TaskSummaryRow>(
        "SELECT id, media_path, name, media_kind, size_bytes, last_status, last_error,
                output_srt_path, output_words_json,
                context_json, transcribed_at,
                created_at, updated_at
         FROM tasks
         ORDER BY updated_at DESC, created_at DESC
         LIMIT ?",
    )
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(rows.into_iter().map(TaskSummary::from).collect())
}

fn non_empty_or_default(value: String, fallback: &str) -> String {
    if value.trim().is_empty() {
        fallback.to_string()
    } else {
        value
    }
}

impl From<TaskSummaryRow> for TaskSummary {
    fn from(row: TaskSummaryRow) -> Self {
        Self {
            id: row.id,
            media_path: row.media_path,
            name: row.name,
            media_kind: row.media_kind,
            size_bytes: row.size_bytes.max(0) as u64,
            last_status: row.last_status,
            last_error: row.last_error,
            output_srt_path: row.output_srt_path,
            output_words_json: row.output_words_json,
            context_json: row.context_json,
            transcribed_at: row.transcribed_at,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

fn build_tasks_context_json_from_snapshot(
    task: &TaskSnapshot,
    transcribe_status: &str,
    transcribe_error: &str,
) -> Result<String, String> {
    let mut context = TaskContext::new(TaskContextSeed {
        task_id: task.id.clone(),
        intent: "TRANSCRIBE".to_string(),
        source_lang: "auto".to_string(),
        target_lang: "zh-CN".to_string(),
        media_path: task.media_path.clone(),
        media_kind: task.media_kind.clone(),
        media_size_bytes: task.size_bytes,
        settings_snapshot: serde_json::json!({}),
        created_at: now_unix(),
    });
    context.set_queue_projection(transcribe_status, "", "", 0, 0, 0, transcribe_error);
    context.set_editor_projection(
        task.subtitle_segments_json.clone(),
        String::new(),
        task.transcript_srt.clone(),
        String::new(),
    );
    context.to_json_string()
}

pub async fn clear_task_events(
    pool: &SqlitePool,
    request: ClearTaskEventsRequest,
) -> Result<(), String> {
    if let Some(task_id) = request.task_id {
        sqlx::query("DELETE FROM task_events WHERE task_id = ?")
            .bind(task_id)
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;
    } else {
        sqlx::query("DELETE FROM task_events")
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub async fn delete_task_summaries(
    pool: &SqlitePool,
    request: DeleteTaskSummariesRequest,
) -> Result<(), String> {
    match (request.task_id, request.media_path) {
        (Some(task_id), Some(media_path)) => {
            sqlx::query("DELETE FROM tasks WHERE id = ? OR media_path = ?")
                .bind(&task_id)
                .bind(&media_path)
                .execute(pool)
                .await
                .map_err(|e| e.to_string())?;
            sqlx::query("DELETE FROM task_runs WHERE id = ? OR media_path = ?")
                .bind(task_id)
                .bind(media_path)
                .execute(pool)
                .await
                .map_err(|e| e.to_string())?;
        }
        (Some(task_id), None) => {
            sqlx::query("DELETE FROM tasks WHERE id = ?")
                .bind(&task_id)
                .execute(pool)
                .await
                .map_err(|e| e.to_string())?;
            sqlx::query("DELETE FROM task_runs WHERE id = ?")
                .bind(task_id)
                .execute(pool)
                .await
                .map_err(|e| e.to_string())?;
        }
        (None, Some(media_path)) => {
            sqlx::query("DELETE FROM tasks WHERE media_path = ?")
                .bind(&media_path)
                .execute(pool)
                .await
                .map_err(|e| e.to_string())?;
            sqlx::query("DELETE FROM task_runs WHERE media_path = ?")
                .bind(media_path)
                .execute(pool)
                .await
                .map_err(|e| e.to_string())?;
        }
        (None, None) => {
            sqlx::query("DELETE FROM tasks")
                .execute(pool)
                .await
                .map_err(|e| e.to_string())?;
            sqlx::query("DELETE FROM task_runs")
                .execute(pool)
                .await
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}
