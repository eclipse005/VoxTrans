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
