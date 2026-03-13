use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::SqlitePool;

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
    #[serde(default = "default_source_lang")]
    pub source_lang: String,
    #[serde(default)]
    pub target_lang: String,
    #[serde(default)]
    pub transcribe_status: String,
    #[serde(default)]
    pub transcribe_error: String,
    #[serde(default)]
    pub transcript_srt: String,
    #[serde(default)]
    pub transcribed_at: Option<i64>,
    #[serde(default = "default_translate_status")]
    pub translate_status: String,
    #[serde(default)]
    pub translate_error: String,
    #[serde(default)]
    pub translated_srt: String,
    #[serde(default)]
    pub translated_srt_path: String,
    #[serde(default)]
    pub subtitle_segments_json: String,
    #[serde(default)]
    pub translate_model: String,
    #[serde(default)]
    pub hotword_status: String,
    #[serde(default)]
    pub hotword_changed_count: i64,
    #[serde(default)]
    pub hotword_replacements_json: String,
    #[serde(default)]
    pub translated_at: Option<i64>,
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
    pub source_lang: String,
    pub target_lang: String,
    pub transcribe_status: String,
    pub transcribe_error: String,
    pub transcript_srt: String,
    pub transcribed_at: Option<i64>,
    pub translate_status: String,
    pub translate_error: String,
    pub translated_srt: String,
    pub translated_srt_path: String,
    pub subtitle_segments_json: String,
    pub translate_model: String,
    pub hotword_status: String,
    pub hotword_changed_count: u64,
    pub hotword_replacements_json: String,
    pub translated_at: Option<i64>,
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
    source_lang: String,
    target_lang: String,
    transcribe_status: String,
    transcribe_error: String,
    transcript_srt: String,
    transcribed_at: Option<i64>,
    translate_status: String,
    translate_error: String,
    translated_srt: String,
    translated_srt_path: String,
    subtitle_segments_json: String,
    translate_model: String,
    hotword_status: String,
    hotword_changed_count: i64,
    hotword_replacements_json: String,
    translated_at: Option<i64>,
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
        let source_lang = non_empty_or_default(task.source_lang, "auto");
        let transcribe_status = non_empty_or_default(task.transcribe_status, &task.last_status);
        let transcribe_error = non_empty_or_default(task.transcribe_error, &task.last_error);
        let subtitle_segments_json = non_empty_or_default(task.subtitle_segments_json, "[]");

        sqlx::query(
            "INSERT INTO tasks (
               id, media_path, name, media_kind, size_bytes, last_status, last_error, output_srt_path, output_words_json,
               source_lang, target_lang, transcribe_status, transcribe_error, transcript_srt,
               transcribed_at, translate_status, translate_error,
               translated_srt, translated_srt_path, subtitle_segments_json, translate_model,
               hotword_status, hotword_changed_count, hotword_replacements_json, translated_at,
               created_at, updated_at
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, strftime('%s','now'), strftime('%s','now'))
             ON CONFLICT(id) DO UPDATE SET
               media_path = excluded.media_path,
               name = excluded.name,
               media_kind = excluded.media_kind,
               size_bytes = excluded.size_bytes,
               last_status = excluded.last_status,
               last_error = excluded.last_error,
               output_srt_path = excluded.output_srt_path,
               output_words_json = excluded.output_words_json,
               source_lang = excluded.source_lang,
               target_lang = excluded.target_lang,
               transcribe_status = excluded.transcribe_status,
               transcribe_error = excluded.transcribe_error,
               transcript_srt = excluded.transcript_srt,
               transcribed_at = excluded.transcribed_at,
               translate_status = excluded.translate_status,
               translate_error = excluded.translate_error,
               translated_srt = excluded.translated_srt,
               translated_srt_path = excluded.translated_srt_path,
               subtitle_segments_json = excluded.subtitle_segments_json,
               translate_model = excluded.translate_model,
               hotword_status = excluded.hotword_status,
               hotword_changed_count = excluded.hotword_changed_count,
               hotword_replacements_json = excluded.hotword_replacements_json,
               translated_at = excluded.translated_at,
               updated_at = excluded.updated_at",
        )
        .bind(task.id)
        .bind(task.media_path)
        .bind(task.name)
        .bind(task.media_kind)
        .bind(task.size_bytes as i64)
        .bind(task.last_status)
        .bind(task.last_error)
        .bind(task.output_srt_path)
        .bind(task.output_words_json)
        .bind(source_lang)
        .bind(task.target_lang)
        .bind(transcribe_status)
        .bind(transcribe_error)
        .bind(task.transcript_srt)
        .bind(task.transcribed_at)
        .bind(task.translate_status)
        .bind(task.translate_error)
        .bind(task.translated_srt)
        .bind(task.translated_srt_path)
        .bind(subtitle_segments_json)
        .bind(task.translate_model)
        .bind(task.hotword_status)
        .bind(task.hotword_changed_count)
        .bind(task.hotword_replacements_json)
        .bind(task.translated_at)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
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
        "SELECT id, media_path, name, media_kind, size_bytes, last_status, last_error, output_srt_path, output_words_json,
                source_lang, target_lang, transcribe_status, transcribe_error, transcript_srt,
                transcribed_at, translate_status, translate_error,
                translated_srt, translated_srt_path, subtitle_segments_json, translate_model,
                hotword_status, hotword_changed_count, hotword_replacements_json, translated_at,
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

fn default_source_lang() -> String {
    "auto".to_string()
}

fn default_translate_status() -> String {
    "idle".to_string()
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
            source_lang: row.source_lang,
            target_lang: row.target_lang,
            transcribe_status: row.transcribe_status,
            transcribe_error: row.transcribe_error,
            transcript_srt: row.transcript_srt,
            transcribed_at: row.transcribed_at,
            translate_status: row.translate_status,
            translate_error: row.translate_error,
            translated_srt: row.translated_srt,
            translated_srt_path: row.translated_srt_path,
            subtitle_segments_json: row.subtitle_segments_json,
            translate_model: row.translate_model,
            hotword_status: row.hotword_status,
            hotword_changed_count: row.hotword_changed_count.max(0) as u64,
            hotword_replacements_json: row.hotword_replacements_json,
            translated_at: row.translated_at,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
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
                .bind(task_id)
                .bind(media_path)
                .execute(pool)
                .await
                .map_err(|e| e.to_string())?;
        }
        (Some(task_id), None) => {
            sqlx::query("DELETE FROM tasks WHERE id = ?")
                .bind(task_id)
                .execute(pool)
                .await
                .map_err(|e| e.to_string())?;
        }
        (None, Some(media_path)) => {
            sqlx::query("DELETE FROM tasks WHERE media_path = ?")
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
        }
    }
    Ok(())
}
