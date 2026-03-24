use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::path::Path;

pub const INTENT_TRANSCRIBE: &str = "TRANSCRIBE";
pub const INTENT_TRANSCRIBE_TRANSLATE: &str = "TRANSCRIBE_TRANSLATE";

const TASK_STAGES: [&str; 9] = [
    "init",
    "separate",
    "asr",
    "punctuate",
    "segment",
    "summarize",
    "translate",
    "segment_optimize",
    "compose",
];

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnqueueTaskRequest {
    pub id: String,
    pub media_path: String,
    pub name: String,
    pub media_kind: String,
    pub size_bytes: u64,
    pub intent: String,
    #[serde(default)]
    pub source_lang: String,
    #[serde(default)]
    pub target_lang: String,
    #[serde(default)]
    pub max_retries: u32,
    #[serde(default)]
    pub settings_snapshot: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterTaskUploadRequest {
    pub id: String,
    pub media_path: String,
    pub name: String,
    pub media_kind: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListTaskRunsRequest {
    pub intent: Option<String>,
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetTaskRunRequest {
    pub task_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteTasksRequest {
    pub media_path: Option<String>,
    pub task_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskRunRecord {
    pub id: String,
    pub media_path: String,
    pub name: String,
    pub media_kind: String,
    pub size_bytes: u64,
    pub intent: String,
    pub retry_count: u32,
    pub max_retries: u32,
    pub settings_policy_version: String,
    pub settings_snapshot_json: String,
    pub source_lang: String,
    pub target_lang: String,
    pub queued_at: i64,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
    pub overall_status: String,
    pub current_stage: String,
    pub progress_percent: u32,
    pub phase_detail: String,
    pub segment_current: u32,
    pub segment_total: u32,
    pub error_message: String,
    pub result_text: String,
    pub result_srt: String,
    pub subtitle_segments_json: String,
    pub translated_srt: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskStepRunRecord {
    pub id: i64,
    pub task_id: String,
    pub step: String,
    pub attempt: u32,
    pub status: String,
    pub input_hash: String,
    pub output_json: String,
    pub metrics_json: String,
    pub error_code: String,
    pub error_message: String,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
    pub duration_ms: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskArtifactRecord {
    pub id: i64,
    pub task_id: String,
    pub kind: String,
    pub path: String,
    pub checksum: String,
    pub size_bytes: u64,
    pub produced_by_step: String,
    pub metadata_json: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskRunDetail {
    pub run: TaskRunRecord,
    pub steps: Vec<TaskStepRunRecord>,
    pub artifacts: Vec<TaskArtifactRecord>,
}

pub async fn enqueue_task(pool: &SqlitePool, request: EnqueueTaskRequest) -> Result<TaskRunRecord, String> {
    validate_enqueue_request(&request)?;
    let source_lang = non_empty_or_default(&request.source_lang, "auto");
    let target_lang = non_empty_or_default(&request.target_lang, "zh-CN");
    let normalized_intent = normalize_intent(&request.intent);
    let snapshot = serde_json::to_string(&request.settings_snapshot).map_err(|err| err.to_string())?;
    let now = unix_now();
    let exists = sqlx::query_scalar::<_, i64>("SELECT COUNT(1) FROM task_runs WHERE id = ?")
        .bind(&request.id)
        .fetch_one(pool)
        .await
        .map_err(|err| err.to_string())?
        > 0;

    if exists {
        sqlx::query(
            "UPDATE task_runs
             SET media_path = ?,
                 name = ?,
                 media_kind = ?,
                 size_bytes = ?,
                 intent = ?,
                 max_retries = ?,
                 settings_snapshot_json = ?,
                 source_lang = ?,
                 target_lang = ?,
                 queued_at = ?,
                 updated_at = ?
             WHERE id = ?",
        )
        .bind(&request.media_path)
        .bind(&request.name)
        .bind(&request.media_kind)
        .bind(request.size_bytes as i64)
        .bind(&normalized_intent)
        .bind(request.max_retries as i64)
        .bind(snapshot)
        .bind(source_lang)
        .bind(target_lang)
        .bind(now)
        .bind(now)
        .bind(&request.id)
        .execute(pool)
        .await
        .map_err(|err| err.to_string())?;
    } else {
        sqlx::query(
            "INSERT INTO task_runs (
                id, media_path, name, media_kind, size_bytes,
                intent, retry_count, max_retries,
                settings_policy_version, settings_snapshot_json,
                source_lang, target_lang, queued_at, started_at, finished_at, created_at, updated_at,
                sort_order, overall_status, current_stage, progress_percent, phase_detail, segment_current, segment_total,
                error_message, result_text, result_srt, subtitle_segments_json, translated_srt
             ) VALUES (?, ?, ?, ?, ?, ?, 0, ?, 'v1', ?, ?, ?, ?, NULL, NULL, ?, ?, (SELECT COALESCE(MAX(sort_order), -1) + 1 FROM task_runs), ?, ?, 0, '', 0, 0, '', '', '', '[]', '')",
        )
        .bind(&request.id)
        .bind(&request.media_path)
        .bind(&request.name)
        .bind(&request.media_kind)
        .bind(request.size_bytes as i64)
        .bind(&normalized_intent)
        .bind(request.max_retries as i64)
        .bind(snapshot)
        .bind(source_lang)
        .bind(target_lang)
        .bind(now)
        .bind(now)
        .bind(now)
        .bind(initial_overall_status("queued"))
        .bind(initial_stage(&normalized_intent))
        .execute(pool)
        .await
        .map_err(|err| err.to_string())?;

        reset_task_stages(pool, &request.id).await?;
    }

    get_task_run(pool, GetTaskRunRequest { task_id: request.id })
        .await?
        .run
        .pipe(Ok)
}

pub async fn register_task_upload(
    pool: &SqlitePool,
    request: RegisterTaskUploadRequest,
) -> Result<TaskRunRecord, String> {
    validate_upload_request(&request)?;
    ensure_task_output_dir_for_upload(&request)?;
    let now = unix_now();

    sqlx::query(
        "INSERT INTO task_runs (
            id, media_path, name, media_kind, size_bytes,
            intent, retry_count, max_retries,
            settings_policy_version, settings_snapshot_json,
            source_lang, target_lang, queued_at, started_at, finished_at, created_at, updated_at,
            sort_order, overall_status, current_stage, progress_percent, phase_detail, segment_current, segment_total,
            error_message, result_text, result_srt, subtitle_segments_json, translated_srt
         ) VALUES (?, ?, ?, ?, ?, ?, 0, 0, 'v1', '{}', 'auto', 'zh-CN', ?, NULL, NULL, ?, ?, (SELECT COALESCE(MAX(sort_order), -1) + 1 FROM task_runs), 'pending', '', 0, '', 0, 0, '', '', '', '[]', '')
         ON CONFLICT(id) DO UPDATE SET
            media_path = excluded.media_path,
            name = excluded.name,
            media_kind = excluded.media_kind,
            size_bytes = excluded.size_bytes,
            intent = excluded.intent,
            retry_count = 0,
            max_retries = 0,
            settings_snapshot_json = '{}',
            source_lang = 'auto',
            target_lang = 'zh-CN',
            queued_at = excluded.queued_at,
            started_at = NULL,
            finished_at = NULL,
            updated_at = excluded.updated_at,
            overall_status = excluded.overall_status,
            current_stage = excluded.current_stage,
            progress_percent = excluded.progress_percent,
            phase_detail = excluded.phase_detail,
            segment_current = excluded.segment_current,
            segment_total = excluded.segment_total,
            error_message = excluded.error_message,
            result_text = excluded.result_text,
            result_srt = excluded.result_srt,
            subtitle_segments_json = excluded.subtitle_segments_json,
            translated_srt = excluded.translated_srt",
    )
    .bind(&request.id)
    .bind(&request.media_path)
    .bind(&request.name)
    .bind(&request.media_kind)
    .bind(request.size_bytes as i64)
    .bind(INTENT_TRANSCRIBE)
    .bind(now)
    .bind(now)
    .bind(now)
    .execute(pool)
    .await
    .map_err(|err| err.to_string())?;

    reset_task_stages(pool, &request.id).await?;

    get_task_run(pool, GetTaskRunRequest { task_id: request.id })
        .await?
        .run
        .pipe(Ok)
}

pub async fn list_task_runs(pool: &SqlitePool, request: ListTaskRunsRequest) -> Result<Vec<TaskRunRecord>, String> {
    let limit = request.limit.unwrap_or(200).clamp(1, 2000) as i64;
    let rows = match request.intent {
        Some(intent) => {
            sqlx::query_as::<_, TaskRunRow>(
                "SELECT id, media_path, name, media_kind, size_bytes, intent, retry_count, max_retries,
                        settings_policy_version, settings_snapshot_json, source_lang, target_lang,
                        queued_at, started_at, finished_at, created_at, updated_at, overall_status,
                        current_stage, progress_percent, phase_detail, segment_current, segment_total,
                        error_message, result_text, result_srt, subtitle_segments_json, translated_srt
                 FROM task_runs
                 WHERE intent = ?
                 ORDER BY updated_at DESC, created_at DESC
                 LIMIT ?",
            )
            .bind(intent.trim().to_uppercase())
            .bind(limit)
            .fetch_all(pool)
            .await
            .map_err(|err| err.to_string())?
        }
        None => {
            sqlx::query_as::<_, TaskRunRow>(
                "SELECT id, media_path, name, media_kind, size_bytes, intent, retry_count, max_retries,
                        settings_policy_version, settings_snapshot_json, source_lang, target_lang,
                        queued_at, started_at, finished_at, created_at, updated_at, overall_status,
                        current_stage, progress_percent, phase_detail, segment_current, segment_total,
                        error_message, result_text, result_srt, subtitle_segments_json, translated_srt
                 FROM task_runs
                 ORDER BY updated_at DESC, created_at DESC
                 LIMIT ?",
            )
            .bind(limit)
            .fetch_all(pool)
            .await
            .map_err(|err| err.to_string())?
        }
    };
    Ok(rows.into_iter().map(TaskRunRecord::from).collect())
}

pub async fn get_task_run(pool: &SqlitePool, request: GetTaskRunRequest) -> Result<TaskRunDetail, String> {
    if request.task_id.trim().is_empty() {
        return Err("taskId is required".to_string());
    }
    let run = sqlx::query_as::<_, TaskRunRow>(
        "SELECT id, media_path, name, media_kind, size_bytes, intent, retry_count, max_retries,
                settings_policy_version, settings_snapshot_json, source_lang, target_lang,
                queued_at, started_at, finished_at, created_at, updated_at, overall_status,
                current_stage, progress_percent, phase_detail, segment_current, segment_total,
                error_message, result_text, result_srt, subtitle_segments_json, translated_srt
         FROM task_runs
         WHERE id = ?",
    )
    .bind(request.task_id.trim())
    .fetch_optional(pool)
    .await
    .map_err(|err| err.to_string())?
    .ok_or_else(|| "task not found".to_string())?;

    let steps = sqlx::query_as::<_, TaskStageRow>(
        "SELECT rowid as id, task_id, stage, attempt, status, input_hash, output_json, metrics_json,
                error_code, error_message, started_at, finished_at, duration_ms, updated_at
         FROM task_stage_runs
         WHERE task_id = ?
         ORDER BY updated_at ASC",
    )
    .bind(request.task_id.trim())
    .fetch_all(pool)
    .await
    .map_err(|err| err.to_string())?
    .into_iter()
    .map(TaskStepRunRecord::from)
    .collect();

    let artifacts = sqlx::query_as::<_, TaskArtifactRow>(
        "SELECT id, task_id, kind, path, checksum, size_bytes, produced_by_stage,
                metadata_json, created_at, updated_at
         FROM task_artifacts
         WHERE task_id = ?
         ORDER BY created_at ASC, id ASC",
    )
    .bind(request.task_id.trim())
    .fetch_all(pool)
    .await
    .map_err(|err| err.to_string())?
    .into_iter()
    .map(TaskArtifactRecord::from)
    .collect();

    Ok(TaskRunDetail {
        run: TaskRunRecord::from(run),
        steps,
        artifacts,
    })
}

pub async fn delete_tasks(pool: &SqlitePool, request: DeleteTasksRequest) -> Result<(), String> {
    match (request.task_id, request.media_path) {
        (Some(task_id), Some(media_path)) => {
            sqlx::query("DELETE FROM task_runs WHERE id = ? OR media_path = ?")
                .bind(task_id)
                .bind(media_path)
                .execute(pool)
                .await
                .map_err(|e| e.to_string())?;
        }
        (Some(task_id), None) => {
            sqlx::query("DELETE FROM task_runs WHERE id = ?")
                .bind(task_id)
                .execute(pool)
                .await
                .map_err(|e| e.to_string())?;
        }
        (None, Some(media_path)) => {
            sqlx::query("DELETE FROM task_runs WHERE media_path = ?")
                .bind(media_path)
                .execute(pool)
                .await
                .map_err(|e| e.to_string())?;
        }
        (None, None) => {
            sqlx::query("DELETE FROM task_runs").execute(pool).await.map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

#[derive(Debug, sqlx::FromRow)]
struct TaskRunRow {
    id: String,
    media_path: String,
    name: String,
    media_kind: String,
    size_bytes: i64,
    intent: String,
    retry_count: i64,
    max_retries: i64,
    settings_policy_version: String,
    settings_snapshot_json: String,
    source_lang: String,
    target_lang: String,
    queued_at: i64,
    started_at: Option<i64>,
    finished_at: Option<i64>,
    created_at: i64,
    updated_at: i64,
    overall_status: String,
    current_stage: String,
    progress_percent: i64,
    phase_detail: String,
    segment_current: i64,
    segment_total: i64,
    error_message: String,
    result_text: String,
    result_srt: String,
    subtitle_segments_json: String,
    translated_srt: String,
}

#[derive(Debug, sqlx::FromRow)]
struct TaskStageRow {
    id: i64,
    task_id: String,
    stage: String,
    attempt: i64,
    status: String,
    input_hash: String,
    output_json: String,
    metrics_json: String,
    error_code: String,
    error_message: String,
    started_at: Option<i64>,
    finished_at: Option<i64>,
    duration_ms: i64,
    updated_at: i64,
}

#[derive(Debug, sqlx::FromRow)]
struct TaskArtifactRow {
    id: i64,
    task_id: String,
    kind: String,
    path: String,
    checksum: String,
    size_bytes: i64,
    produced_by_stage: String,
    metadata_json: String,
    created_at: i64,
    updated_at: i64,
}

impl From<TaskRunRow> for TaskRunRecord {
    fn from(row: TaskRunRow) -> Self {
        Self {
            id: row.id,
            media_path: row.media_path,
            name: row.name,
            media_kind: row.media_kind,
            size_bytes: row.size_bytes.max(0) as u64,
            intent: row.intent,
            retry_count: row.retry_count.max(0) as u32,
            max_retries: row.max_retries.max(0) as u32,
            settings_policy_version: row.settings_policy_version,
            settings_snapshot_json: row.settings_snapshot_json,
            source_lang: row.source_lang,
            target_lang: row.target_lang,
            queued_at: row.queued_at,
            started_at: row.started_at,
            finished_at: row.finished_at,
            created_at: row.created_at,
            updated_at: row.updated_at,
            overall_status: row.overall_status,
            current_stage: row.current_stage,
            progress_percent: row.progress_percent.clamp(0, 100) as u32,
            phase_detail: row.phase_detail,
            segment_current: row.segment_current.max(0) as u32,
            segment_total: row.segment_total.max(0) as u32,
            error_message: row.error_message,
            result_text: row.result_text,
            result_srt: row.result_srt,
            subtitle_segments_json: row.subtitle_segments_json,
            translated_srt: row.translated_srt,
        }
    }
}

impl From<TaskStageRow> for TaskStepRunRecord {
    fn from(row: TaskStageRow) -> Self {
        Self {
            id: row.id,
            task_id: row.task_id,
            step: row.stage,
            attempt: row.attempt.max(0) as u32,
            status: row.status,
            input_hash: row.input_hash,
            output_json: row.output_json,
            metrics_json: row.metrics_json,
            error_code: row.error_code,
            error_message: row.error_message,
            started_at: row.started_at,
            finished_at: row.finished_at,
            duration_ms: row.duration_ms.max(0),
            updated_at: row.updated_at,
        }
    }
}

impl From<TaskArtifactRow> for TaskArtifactRecord {
    fn from(row: TaskArtifactRow) -> Self {
        Self {
            id: row.id,
            task_id: row.task_id,
            kind: row.kind,
            path: row.path,
            checksum: row.checksum,
            size_bytes: row.size_bytes.max(0) as u64,
            produced_by_step: row.produced_by_stage,
            metadata_json: row.metadata_json,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

async fn reset_task_stages(pool: &SqlitePool, task_id: &str) -> Result<(), String> {
    sqlx::query("DELETE FROM task_stage_runs WHERE task_id = ?")
        .bind(task_id)
        .execute(pool)
        .await
        .map_err(|err| err.to_string())?;

    for stage in TASK_STAGES {
        sqlx::query(
            "INSERT INTO task_stage_runs (
                task_id, stage, status, attempt, input_hash, output_json, metrics_json,
                error_code, error_message, started_at, finished_at, duration_ms, updated_at
             ) VALUES (?, ?, 'pending', 0, '', '{}', '{}', '', '', NULL, NULL, 0, strftime('%s','now'))",
        )
        .bind(task_id)
        .bind(stage)
        .execute(pool)
        .await
        .map_err(|err| err.to_string())?;
    }
    Ok(())
}

fn initial_overall_status(status: &str) -> String {
    status.to_string()
}

fn initial_stage(intent: &str) -> String {
    let _ = intent;
    "init".to_string()
}

fn normalize_intent(raw: &str) -> String {
    if raw.trim().eq_ignore_ascii_case(INTENT_TRANSCRIBE_TRANSLATE) {
        INTENT_TRANSCRIBE_TRANSLATE.to_string()
    } else {
        INTENT_TRANSCRIBE.to_string()
    }
}

fn validate_enqueue_request(request: &EnqueueTaskRequest) -> Result<(), String> {
    if request.id.trim().is_empty() {
        return Err("id is required".to_string());
    }
    if request.media_path.trim().is_empty() {
        return Err("mediaPath is required".to_string());
    }
    if request.name.trim().is_empty() {
        return Err("name is required".to_string());
    }
    if request.media_kind.trim().is_empty() {
        return Err("mediaKind is required".to_string());
    }
    let intent = request.intent.trim().to_uppercase();
    if !matches!(intent.as_str(), INTENT_TRANSCRIBE | INTENT_TRANSCRIBE_TRANSLATE) {
        return Err("intent is invalid".to_string());
    }
    Ok(())
}

fn validate_upload_request(request: &RegisterTaskUploadRequest) -> Result<(), String> {
    if request.id.trim().is_empty() {
        return Err("id is required".to_string());
    }
    if request.media_path.trim().is_empty() {
        return Err("mediaPath is required".to_string());
    }
    if request.name.trim().is_empty() {
        return Err("name is required".to_string());
    }
    if request.media_kind.trim().is_empty() {
        return Err("mediaKind is required".to_string());
    }
    Ok(())
}

fn non_empty_or_default(value: &str, fallback: &str) -> String {
    if value.trim().is_empty() {
        fallback.to_string()
    } else {
        value.trim().to_string()
    }
}

fn ensure_task_output_dir_for_upload(request: &RegisterTaskUploadRequest) -> Result<(), String> {
    let media_path = Path::new(request.media_path.as_str());
    let target_dir = if request.media_path.starts_with("youtube://") {
        let safe_name = crate::services::task_path::sanitize_filename_component(&request.name);
        let safe_task_id = crate::services::task_path::sanitize_filename_component(&request.id);
        let base_name = if safe_name.is_empty() {
            "youtube_task".to_string()
        } else {
            safe_name
        };
        crate::services::output::resolve_output_dir().join(format!("{base_name}_{safe_task_id}"))
    } else {
        crate::services::task_path::task_output_dir(&request.id, media_path)
    };
    std::fs::create_dir_all(target_dir).map_err(|err| format!("创建任务目录失败: {err}"))
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

trait Pipe: Sized {
    fn pipe<T>(self, f: impl FnOnce(Self) -> T) -> T {
        f(self)
    }
}

impl<T> Pipe for T {}
