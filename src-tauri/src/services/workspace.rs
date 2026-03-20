use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use crate::services::task_context::{TaskContext, TaskContextSeed};

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
    #[serde(default)]
    pub transcribe_phase_detail: String,
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
                intent, source_lang, target_lang, created_at, context_json
         FROM task_runs
         ORDER BY sort_order ASC, created_at ASC, id ASC",
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
        let intent = if item.transcribe_status == "done" {
            "TRANSCRIBE_TRANSLATE"
        } else {
            "TRANSCRIBE"
        };
        let context_json = context_json_from_queue_item(item, intent, now)?;
        sqlx::query(
            "INSERT INTO task_runs (
               id, media_path, name, media_kind, size_bytes,
               intent, retry_count, max_retries,
               settings_policy_version, settings_snapshot_json,
               source_lang, target_lang, queued_at, started_at, finished_at, created_at, updated_at,
               sort_order, context_json
             ) VALUES (?, ?, ?, ?, ?, ?, 0, 0, 'v1', '{}', 'auto', 'zh-CN', ?, NULL, NULL, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET
               media_path = excluded.media_path,
               name = excluded.name,
               media_kind = excluded.media_kind,
               size_bytes = excluded.size_bytes,
               intent = excluded.intent,
               updated_at = excluded.updated_at,
               sort_order = excluded.sort_order,
               context_json = excluded.context_json",
        )
        .bind(&item.id)
        .bind(&item.path)
        .bind(&item.name)
        .bind(&item.media_kind)
        .bind(item.size_bytes as i64)
        .bind(intent)
        .bind(now)
        .bind(now)
        .bind(now)
        .bind(index as i64)
        .bind(context_json)
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
    intent: String,
    source_lang: String,
    target_lang: String,
    created_at: i64,
    context_json: String,
}

impl From<QueueItemRow> for QueueItemRecord {
    fn from(row: QueueItemRow) -> Self {
        let context = TaskContext::parse_or_new(
            &row.context_json,
            TaskContextSeed {
                task_id: row.id.clone(),
                intent: row.intent.clone(),
                source_lang: row.source_lang,
                target_lang: row.target_lang,
                media_path: row.media_path.clone(),
                media_kind: row.media_kind.clone(),
                media_size_bytes: row.size_bytes.max(0) as u64,
                settings_snapshot: serde_json::json!({}),
                created_at: row.created_at,
            },
        );
        let queue = context.projections.queue;
        let editor = context.projections.editor;
        Self {
            id: row.id,
            path: row.media_path,
            name: row.name,
            media_kind: row.media_kind,
            size_bytes: row.size_bytes.max(0) as u64,
            transcribe_status: queue.transcribe_status,
            transcribe_progress: queue.progress_percent.clamp(0, 100),
            transcribe_segment_current: queue.transcribe_segment_current,
            transcribe_segment_total: queue.transcribe_segment_total,
            transcribe_phase: queue.phase,
            transcribe_phase_detail: queue.phase_detail,
            transcribe_error: queue.transcribe_error,
            result_text: editor.result_text,
            result_srt: editor.result_srt,
            subtitle_segments_json: editor.subtitle_segments_json,
        }
    }
}

fn context_json_from_queue_item(item: &QueueItemRecord, intent: &str, created_at: i64) -> Result<String, String> {
    let mut context = TaskContext::new(TaskContextSeed {
        task_id: item.id.clone(),
        intent: intent.to_string(),
        source_lang: "auto".to_string(),
        target_lang: "zh-CN".to_string(),
        media_path: item.path.clone(),
        media_kind: item.media_kind.clone(),
        media_size_bytes: item.size_bytes,
        settings_snapshot: serde_json::json!({}),
        created_at,
    });
    context.set_queue_projection(
        &item.transcribe_status,
        &item.transcribe_phase,
        &item.transcribe_phase_detail,
        item.transcribe_progress,
        item.transcribe_segment_current,
        item.transcribe_segment_total,
        &item.transcribe_error,
    );
    context.set_editor_projection(
        item.subtitle_segments_json.clone(),
        item.result_text.clone(),
        item.result_srt.clone(),
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
