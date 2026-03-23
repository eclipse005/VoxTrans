use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceTaskRequest {
    pub task_id: String,
}

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

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceTaskResponse {
    pub item: QueueItemRecord,
}

pub async fn load_workspace_state(pool: &SqlitePool) -> Result<WorkspaceStateResponse, String> {
    let rows = sqlx::query_as::<_, QueueItemRow>(
        "SELECT id, media_path, name, media_kind, size_bytes,
                overall_status, progress_percent, segment_current, segment_total,
                current_stage, phase_detail, error_message
         FROM task_runs
         ORDER BY sort_order ASC, created_at ASC, id ASC",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    let queue = rows.into_iter().map(QueueItemRecord::from).collect();
    Ok(WorkspaceStateResponse { queue })
}

pub async fn load_workspace_task(
    pool: &SqlitePool,
    request: WorkspaceTaskRequest,
) -> Result<WorkspaceTaskResponse, String> {
    let task_id = request.task_id.trim();
    if task_id.is_empty() {
        return Err("taskId is required".to_string());
    }

    let row = sqlx::query_as::<_, TaskQueueItemRow>(
        "SELECT id, media_path, name, media_kind, size_bytes,
                overall_status, progress_percent, segment_current, segment_total,
                current_stage, phase_detail, error_message,
                result_text, result_srt, subtitle_segments_json
         FROM task_runs
         WHERE id = ?",
    )
    .bind(task_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| e.to_string())?
    .ok_or_else(|| "task not found".to_string())?;

    Ok(WorkspaceTaskResponse {
        item: QueueItemRecord::from(row),
    })
}

#[derive(Debug, sqlx::FromRow)]
struct QueueItemRow {
    id: String,
    media_path: String,
    name: String,
    media_kind: String,
    size_bytes: i64,
    overall_status: String,
    progress_percent: i64,
    segment_current: i64,
    segment_total: i64,
    current_stage: String,
    phase_detail: String,
    error_message: String,
}

impl From<QueueItemRow> for QueueItemRecord {
    fn from(row: QueueItemRow) -> Self {
        Self {
            id: row.id,
            path: row.media_path,
            name: row.name,
            media_kind: row.media_kind,
            size_bytes: row.size_bytes.max(0) as u64,
            transcribe_status: map_status(&row.overall_status),
            transcribe_progress: row.progress_percent.clamp(0, 100) as u32,
            transcribe_segment_current: row.segment_current.max(0) as u32,
            transcribe_segment_total: row.segment_total.max(0) as u32,
            transcribe_phase: map_phase(&row.current_stage),
            transcribe_phase_detail: row.phase_detail,
            transcribe_error: row.error_message,
            result_text: String::new(),
            result_srt: String::new(),
            subtitle_segments_json: "[]".to_string(),
        }
    }
}

#[derive(Debug, sqlx::FromRow)]
struct TaskQueueItemRow {
    id: String,
    media_path: String,
    name: String,
    media_kind: String,
    size_bytes: i64,
    overall_status: String,
    progress_percent: i64,
    segment_current: i64,
    segment_total: i64,
    current_stage: String,
    phase_detail: String,
    error_message: String,
    result_text: String,
    result_srt: String,
    subtitle_segments_json: String,
}

impl From<TaskQueueItemRow> for QueueItemRecord {
    fn from(row: TaskQueueItemRow) -> Self {
        Self {
            id: row.id,
            path: row.media_path,
            name: row.name,
            media_kind: row.media_kind,
            size_bytes: row.size_bytes.max(0) as u64,
            transcribe_status: map_status(&row.overall_status),
            transcribe_progress: row.progress_percent.clamp(0, 100) as u32,
            transcribe_segment_current: row.segment_current.max(0) as u32,
            transcribe_segment_total: row.segment_total.max(0) as u32,
            transcribe_phase: map_phase(&row.current_stage),
            transcribe_phase_detail: row.phase_detail,
            transcribe_error: row.error_message,
            result_text: row.result_text,
            result_srt: row.result_srt,
            subtitle_segments_json: normalize_segments_json(&row.subtitle_segments_json),
        }
    }
}

fn map_status(raw: &str) -> String {
    match raw.trim().to_lowercase().as_str() {
        "queued" => "queued".to_string(),
        "running" => "processing".to_string(),
        "completed" => "done".to_string(),
        "failed" => "error".to_string(),
        "pending" => "pending".to_string(),
        _ => "pending".to_string(),
    }
}

fn map_phase(raw: &str) -> String {
    match raw.trim().to_lowercase().as_str() {
        "separate" => "separating".to_string(),
        "asr" => "recognizing".to_string(),
        "" => String::new(),
        other => other.to_string(),
    }
}

fn normalize_segments_json(raw: &str) -> String {
    if raw.trim().is_empty() {
        "[]".to_string()
    } else {
        raw.to_string()
    }
}
