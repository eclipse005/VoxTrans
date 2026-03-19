use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use crate::services::task_context::{STAGE_INIT, TaskContext, TaskContextSeed};

pub const INTENT_TRANSCRIBE: &str = "TRANSCRIBE";
pub const INTENT_TRANSCRIBE_TRANSLATE: &str = "TRANSCRIBE_TRANSLATE";
pub const INTENT_TRANSLATE_ONLY: &str = "TRANSLATE_ONLY";

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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskRunRecord {
    pub id: String,
    pub media_path: String,
    pub name: String,
    pub media_kind: String,
    pub size_bytes: u64,
    pub intent: String,
    pub context_json: String,
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
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskStepRunRecord {
    pub id: i64,
    pub task_id: String,
    pub step: String,
    pub attempt: u32,
    pub status: String,
    pub binding_mode: String,
    pub input_hash: String,
    pub settings_snapshot_json: String,
    pub diagnostics_json: String,
    pub error_code: String,
    pub error_message: String,
    pub started_at: i64,
    pub finished_at: Option<i64>,
    pub created_at: i64,
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
    pub mime_type: String,
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
    let normalized_intent = request.intent.trim().to_uppercase();
    let snapshot = serde_json::to_string(&request.settings_snapshot).map_err(|err| err.to_string())?;
    let now = unix_now();
    let existing_context_json = sqlx::query_scalar::<_, String>(
        "SELECT context_json FROM task_runs WHERE id = ?",
    )
    .bind(&request.id)
    .fetch_optional(pool)
    .await
    .map_err(|err| err.to_string())?;
    let context_json = build_enqueue_context_json(
        &request,
        &normalized_intent,
        &source_lang,
        &target_lang,
        now,
        existing_context_json.as_deref(),
    )?;

    sqlx::query(
        "INSERT INTO task_runs (
            id, media_path, name, media_kind, size_bytes,
            intent, retry_count, max_retries,
            settings_policy_version, settings_snapshot_json,
            source_lang, target_lang, queued_at, started_at, finished_at, created_at, updated_at,
            sort_order, context_json
         ) VALUES (?, ?, ?, ?, ?, ?, 0, ?, 'v1', ?, ?, ?, ?, NULL, NULL, ?, ?, (SELECT COALESCE(MAX(sort_order), -1) + 1 FROM task_runs), ?)
         ON CONFLICT(id) DO UPDATE SET
            media_path = excluded.media_path,
            name = excluded.name,
            media_kind = excluded.media_kind,
            size_bytes = excluded.size_bytes,
            intent = excluded.intent,
            max_retries = excluded.max_retries,
            settings_snapshot_json = excluded.settings_snapshot_json,
            source_lang = excluded.source_lang,
            target_lang = excluded.target_lang,
            queued_at = excluded.queued_at,
            started_at = NULL,
            finished_at = NULL,
            updated_at = excluded.updated_at,
            context_json = excluded.context_json",
    )
    .bind(&request.id)
    .bind(&request.media_path)
    .bind(&request.name)
    .bind(&request.media_kind)
    .bind(request.size_bytes as i64)
    .bind(normalized_intent)
    .bind(request.max_retries as i64)
    .bind(snapshot)
    .bind(source_lang)
    .bind(target_lang)
    .bind(now)
    .bind(now)
    .bind(now)
    .bind(context_json)
    .execute(pool)
    .await
    .map_err(|err| err.to_string())?;

    get_task_run(pool, GetTaskRunRequest {
        task_id: request.id,
    })
    .await?
    .run
    .pipe(Ok)
}

pub async fn register_task_upload(
    pool: &SqlitePool,
    request: RegisterTaskUploadRequest,
) -> Result<TaskRunRecord, String> {
    validate_upload_request(&request)?;
    let now = unix_now();
    let context_json = build_upload_context_json(&request, now)?;

    sqlx::query(
        "INSERT INTO task_runs (
            id, media_path, name, media_kind, size_bytes,
            intent, retry_count, max_retries,
            settings_policy_version, settings_snapshot_json,
            source_lang, target_lang, queued_at, started_at, finished_at, created_at, updated_at,
            sort_order, context_json
         ) VALUES (?, ?, ?, ?, ?, ?, 0, 0, 'v1', '{}', 'auto', 'zh-CN', ?, NULL, NULL, ?, ?, (SELECT COALESCE(MAX(sort_order), -1) + 1 FROM task_runs), ?)
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
            context_json = excluded.context_json",
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
    .bind(context_json)
    .execute(pool)
    .await
    .map_err(|err| err.to_string())?;

    get_task_run(
        pool,
        GetTaskRunRequest {
            task_id: request.id,
        },
    )
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
                        queued_at, started_at, finished_at, created_at, updated_at, context_json
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
                        queued_at, started_at, finished_at, created_at, updated_at, context_json
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
                queued_at, started_at, finished_at, created_at, updated_at, context_json
         FROM task_runs
         WHERE id = ?",
    )
    .bind(request.task_id.trim())
    .fetch_optional(pool)
    .await
    .map_err(|err| err.to_string())?
    .ok_or_else(|| "task not found".to_string())?;

    let steps = sqlx::query_as::<_, TaskStepRunRow>(
        "SELECT id, task_id, step, attempt, status, binding_mode, input_hash, settings_snapshot_json,
                diagnostics_json, error_code, error_message, started_at, finished_at, created_at, updated_at
         FROM task_step_runs
         WHERE task_id = ?
         ORDER BY created_at ASC, id ASC",
    )
    .bind(request.task_id.trim())
    .fetch_all(pool)
    .await
    .map_err(|err| err.to_string())?
    .into_iter()
    .map(TaskStepRunRecord::from)
    .collect();

    let artifacts = sqlx::query_as::<_, TaskArtifactRow>(
        "SELECT id, task_id, kind, path, checksum, size_bytes, mime_type, produced_by_step,
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
    context_json: String,
}

#[derive(Debug, sqlx::FromRow)]
struct TaskStepRunRow {
    id: i64,
    task_id: String,
    step: String,
    attempt: i64,
    status: String,
    binding_mode: String,
    input_hash: String,
    settings_snapshot_json: String,
    diagnostics_json: String,
    error_code: String,
    error_message: String,
    started_at: i64,
    finished_at: Option<i64>,
    created_at: i64,
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
    mime_type: String,
    produced_by_step: String,
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
            context_json: row.context_json,
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
        }
    }
}

impl From<TaskStepRunRow> for TaskStepRunRecord {
    fn from(row: TaskStepRunRow) -> Self {
        Self {
            id: row.id,
            task_id: row.task_id,
            step: row.step,
            attempt: row.attempt.max(0) as u32,
            status: row.status,
            binding_mode: row.binding_mode,
            input_hash: row.input_hash,
            settings_snapshot_json: row.settings_snapshot_json,
            diagnostics_json: row.diagnostics_json,
            error_code: row.error_code,
            error_message: row.error_message,
            started_at: row.started_at,
            finished_at: row.finished_at,
            created_at: row.created_at,
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
            mime_type: row.mime_type,
            produced_by_step: row.produced_by_step,
            metadata_json: row.metadata_json,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
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
    if !matches!(
        intent.as_str(),
        INTENT_TRANSCRIBE | INTENT_TRANSCRIBE_TRANSLATE | INTENT_TRANSLATE_ONLY
    ) {
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

fn build_enqueue_context_json(
    request: &EnqueueTaskRequest,
    intent: &str,
    source_lang: &str,
    target_lang: &str,
    created_at: i64,
    existing_context_json: Option<&str>,
) -> Result<String, String> {
    let mut context = TaskContext::parse_or_new(
        existing_context_json.unwrap_or_default(),
        TaskContextSeed {
        task_id: request.id.clone(),
        intent: intent.to_string(),
        source_lang: source_lang.to_string(),
        target_lang: target_lang.to_string(),
        media_path: request.media_path.clone(),
        media_kind: request.media_kind.clone(),
        media_size_bytes: request.size_bytes,
        settings_snapshot: request.settings_snapshot.clone(),
        created_at,
        },
    );
    context.task.intent = intent.to_string();
    context.task.source_lang = source_lang.to_string();
    context.task.target_lang = target_lang.to_string();
    context.task.updated_at = created_at;
    context.input.media_path = request.media_path.clone();
    context.input.media_kind = request.media_kind.clone();
    context.input.media_size_bytes = request.size_bytes;
    context.input.settings_snapshot = request.settings_snapshot.clone();
    context.runtime.current_stage = STAGE_INIT.to_string();
    context.runtime.can_resume_from = STAGE_INIT.to_string();
    context.runtime.status = "queued".to_string();
    context.set_queue_projection("queued", "", 0, 0, 0, "");
    context.to_json_string()
}

fn build_upload_context_json(
    request: &RegisterTaskUploadRequest,
    created_at: i64,
) -> Result<String, String> {
    let mut context = TaskContext::new(TaskContextSeed {
        task_id: request.id.clone(),
        intent: INTENT_TRANSCRIBE.to_string(),
        source_lang: "auto".to_string(),
        target_lang: "zh-CN".to_string(),
        media_path: request.media_path.clone(),
        media_kind: request.media_kind.clone(),
        media_size_bytes: request.size_bytes,
        settings_snapshot: serde_json::json!({}),
        created_at,
    });
    context.runtime.status = "created".to_string();
    context.set_queue_projection("pending", "", 0, 0, 0, "");
    context.to_json_string()
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
