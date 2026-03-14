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
        "SELECT id, path, name, media_kind, size_bytes,
                transcribe_status, transcribe_progress, transcribe_segment_current, transcribe_segment_total,
                transcribe_error, result_text, result_srt, subtitle_segments_json
         FROM queue_items
         ORDER BY sort_order ASC, id ASC",
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
    sqlx::query("DELETE FROM queue_items")
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;

    for (index, item) in request.queue.iter().enumerate() {
        sqlx::query(
            "INSERT INTO queue_items (
               id, path, name, media_kind, size_bytes,
               transcribe_status, transcribe_progress, transcribe_segment_current, transcribe_segment_total,
               transcribe_error, result_text, result_srt, subtitle_segments_json, sort_order
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&item.id)
        .bind(&item.path)
        .bind(&item.name)
        .bind(&item.media_kind)
        .bind(item.size_bytes as i64)
        .bind(&item.transcribe_status)
        .bind(item.transcribe_progress as i64)
        .bind(item.transcribe_segment_current as i64)
        .bind(item.transcribe_segment_total as i64)
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
    path: String,
    name: String,
    media_kind: String,
    size_bytes: i64,
    transcribe_status: String,
    transcribe_progress: i64,
    transcribe_segment_current: i64,
    transcribe_segment_total: i64,
    transcribe_error: String,
    result_text: String,
    result_srt: String,
    subtitle_segments_json: String,
}

impl From<QueueItemRow> for QueueItemRecord {
    fn from(row: QueueItemRow) -> Self {
        Self {
            id: row.id,
            path: row.path,
            name: row.name,
            media_kind: row.media_kind,
            size_bytes: row.size_bytes.max(0) as u64,
            transcribe_status: row.transcribe_status,
            transcribe_progress: row.transcribe_progress.clamp(0, 100) as u32,
            transcribe_segment_current: row.transcribe_segment_current.max(0) as u32,
            transcribe_segment_total: row.transcribe_segment_total.max(0) as u32,
            transcribe_error: row.transcribe_error,
            result_text: row.result_text,
            result_srt: row.result_srt,
            subtitle_segments_json: row.subtitle_segments_json,
        }
    }
}
