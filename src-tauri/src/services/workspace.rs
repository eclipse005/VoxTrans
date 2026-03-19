use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueItemRecord {
    pub id: String,
    pub path: String,
    pub name: String,
    pub media_kind: String,
    pub size_bytes: u64,
    pub transcribe_status: String,
    pub transcribe_progress: u32,
    pub transcribe_segment_current: u32,
    pub transcribe_segment_total: u32,
    #[serde(default)]
    pub transcribe_phase: String,
    pub transcribe_error: String,
    pub result_text: String,
    pub result_srt: String,
    pub subtitle_segments_json: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceStateResponse {
    pub queue: Vec<QueueItemRecord>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveQueueStateRequest {
    pub queue: Vec<QueueItemRecord>,
}

pub async fn load_workspace_state(pool: &SqlitePool) -> Result<WorkspaceStateResponse, String> {
    let rows = sqlx::query_as::<_, QueueItemRow>(
        "SELECT id, media_path, name, media_kind, size_bytes,
                transcribe_status, transcribe_progress, transcribe_segment_current, transcribe_segment_total,
                transcribe_phase, transcribe_error, result_text, result_srt, subtitle_segments_json
         FROM task_runs
         ORDER BY sort_order ASC, updated_at DESC, id ASC",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    let queue = rows.into_iter().map(QueueItemRecord::from).collect();
    Ok(WorkspaceStateResponse { queue })
}

pub async fn save_queue_state(
    pool: &SqlitePool,
    request: SaveQueueStateRequest,
) -> Result<(), String> {
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    let now = now_unix();

    for (index, item) in request.queue.iter().enumerate() {
        let state = map_transcribe_status_to_state(&item.transcribe_status);
        let intent = if item.transcribe_status == "done" {
            "TRANSCRIBE_TRANSLATE"
        } else {
            "TRANSCRIBE"
        };
        sqlx::query(
            "INSERT INTO task_runs (
               id, media_path, name, media_kind, size_bytes,
               intent, state, current_step, progress_percent, progress_note,
               error_code, error_message, retry_count, max_retries,
               settings_policy_version, settings_snapshot_json,
               source_lang, target_lang, queued_at, started_at, finished_at, created_at, updated_at,
               transcribe_status, transcribe_progress, transcribe_segment_current, transcribe_segment_total,
               transcribe_phase, transcribe_error, result_text, result_srt, subtitle_segments_json, sort_order
             ) VALUES (?, ?, ?, ?, ?, ?, ?, '', ?, '', '', ?, 0, 0, 'v1', '{}', 'auto', 'zh-CN', ?, NULL, NULL, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET
               media_path = excluded.media_path,
               name = excluded.name,
               media_kind = excluded.media_kind,
               size_bytes = excluded.size_bytes,
               state = excluded.state,
               progress_percent = excluded.progress_percent,
               error_message = excluded.error_message,
               updated_at = excluded.updated_at,
               transcribe_status = excluded.transcribe_status,
               transcribe_progress = excluded.transcribe_progress,
               transcribe_segment_current = excluded.transcribe_segment_current,
               transcribe_segment_total = excluded.transcribe_segment_total,
               transcribe_phase = excluded.transcribe_phase,
               transcribe_error = excluded.transcribe_error,
               result_text = excluded.result_text,
               result_srt = excluded.result_srt,
               subtitle_segments_json = excluded.subtitle_segments_json,
               sort_order = excluded.sort_order",
        )
        .bind(&item.id)
        .bind(&item.path)
        .bind(&item.name)
        .bind(&item.media_kind)
        .bind(item.size_bytes as i64)
        .bind(intent)
        .bind(state)
        .bind(item.transcribe_progress as i64)
        .bind(&item.transcribe_error)
        .bind(now)
        .bind(now)
        .bind(now)
        .bind(&item.transcribe_status)
        .bind(item.transcribe_progress as i64)
        .bind(item.transcribe_segment_current as i64)
        .bind(item.transcribe_segment_total as i64)
        .bind(&item.transcribe_phase)
        .bind(&item.transcribe_error)
        .bind(&item.result_text)
        .bind(&item.result_srt)
        .bind(&item.subtitle_segments_json)
        .bind(index as i64)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    }

    tx.commit().await.map_err(|e| e.to_string())
}

#[derive(Debug, sqlx::FromRow)]
struct QueueItemRow {
    id: String,
    media_path: String,
    name: String,
    media_kind: String,
    size_bytes: i64,
    transcribe_status: String,
    transcribe_progress: i64,
    transcribe_segment_current: i64,
    transcribe_segment_total: i64,
    transcribe_phase: String,
    transcribe_error: String,
    result_text: String,
    result_srt: String,
    subtitle_segments_json: String,
}

impl From<QueueItemRow> for QueueItemRecord {
    fn from(row: QueueItemRow) -> Self {
        Self {
            id: row.id,
            path: row.media_path,
            name: row.name,
            media_kind: row.media_kind,
            size_bytes: row.size_bytes.max(0) as u64,
            transcribe_status: row.transcribe_status,
            transcribe_progress: row.transcribe_progress.clamp(0, 100) as u32,
            transcribe_segment_current: row.transcribe_segment_current.max(0) as u32,
            transcribe_segment_total: row.transcribe_segment_total.max(0) as u32,
            transcribe_phase: row.transcribe_phase,
            transcribe_error: row.transcribe_error,
            result_text: row.result_text,
            result_srt: row.result_srt,
            subtitle_segments_json: row.subtitle_segments_json,
        }
    }
}

fn map_transcribe_status_to_state(status: &str) -> &'static str {
    match status {
        "queued" => "QUEUED",
        "processing" => "RUNNING",
        "done" => "COMPLETED",
        "error" => "FAILED",
        _ => "CREATED",
    }
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
