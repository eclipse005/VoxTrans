use serde_json::Value;
use sqlx::SqlitePool;

use crate::services::task_context::{
    STAGE_ASR, STAGE_COMPOSE, STAGE_INIT, STAGE_PUNCTUATE, STAGE_SEGMENT, STAGE_SEGMENT_OPTIMIZE,
    STAGE_SEPARATE, STAGE_SUMMARIZE, STAGE_TRANSLATE, TaskContext, TaskContextSeed,
};
use crate::services::task_projection::TaskProjectionState;
use crate::services::task_projection_store::{
    TaskProjectionHydrationInput, hydrate_task_projection as hydrate_projection_from_store,
    persist_task_projection,
};
use crate::services::task_stage_store::{
    TaskStageSnapshot, load_task_stage_snapshot_rows, persist_task_stage_snapshots,
};
use crate::services::task_status::{TaskRuntimeStatus, runtime_status_from_db};

use super::events::TaskStateChangedEvent;

#[derive(Debug, Clone, sqlx::FromRow)]
pub(super) struct TaskRunExecRow {
    pub id: String,
    pub name: String,
    pub media_path: String,
    pub media_kind: String,
    pub size_bytes: i64,
    pub intent: String,
    pub source_lang: String,
    pub target_lang: String,
    pub settings_snapshot_json: String,
    pub created_at: i64,
    pub overall_status: String,
    pub current_stage: String,
    pub progress_percent: i64,
    pub phase_detail: String,
    pub segment_current: i64,
    pub segment_total: i64,
    pub error_message: String,
    pub result_text: String,
    pub result_srt: String,
    pub subtitle_segments_json: String,
    pub translated_srt: String,
}

pub(super) fn set_queue_projection(
    context: &mut TaskContext,
    projection: &mut TaskProjectionState,
    status: &str,
    phase: &str,
    phase_detail: &str,
    progress_percent: u32,
    current: u32,
    total: u32,
    error: &str,
) {
    context.runtime.progress_percent = projection.set_queue(
        status,
        phase,
        phase_detail,
        progress_percent,
        current,
        total,
        error,
    );
}

pub(super) async fn hydrate_task_context(
    pool: &SqlitePool,
    task: &TaskRunExecRow,
    settings_snapshot: Value,
) -> Result<TaskContext, String> {
    let mut context = TaskContext::new(TaskContextSeed {
        task_id: task.id.clone(),
        intent: task.intent.clone(),
        source_lang: task.source_lang.clone(),
        target_lang: task.target_lang.clone(),
        media_path: task.media_path.clone(),
        media_kind: task.media_kind.clone(),
        media_size_bytes: task.size_bytes.max(0) as u64,
        settings_snapshot,
        created_at: task.created_at,
    });

    context.runtime.status = runtime_status_from_db(&task.overall_status);
    context.runtime.current_stage = task.current_stage.clone();
    context.runtime.progress_percent = task.progress_percent.clamp(0, 100) as u32;

    let rows = load_task_stage_snapshot_rows(pool, &task.id).await?;

    for row in rows {
        let output = serde_json::from_str::<Value>(&row.output_json).unwrap_or(Value::Null);
        let metrics = serde_json::from_str::<Value>(&row.metrics_json).unwrap_or(Value::Null);
        context.set_stage_snapshot(
            &row.stage,
            row.status,
            row.started_at,
            row.finished_at,
            output,
            metrics,
            row.error_code,
            row.error_message,
        );
    }

    Ok(context)
}

pub(super) fn hydrate_task_projection(task: &TaskRunExecRow) -> TaskProjectionState {
    hydrate_projection_from_store(TaskProjectionHydrationInput {
        overall_status: task.overall_status.clone(),
        current_stage: task.current_stage.clone(),
        progress_percent: task.progress_percent,
        phase_detail: task.phase_detail.clone(),
        segment_current: task.segment_current,
        segment_total: task.segment_total,
        error_message: task.error_message.clone(),
        subtitle_segments_json: task.subtitle_segments_json.clone(),
        result_text: task.result_text.clone(),
        result_srt: task.result_srt.clone(),
        translated_srt: task.translated_srt.clone(),
    })
}

/// Build a TaskStateChangedEvent from task data and projection.
pub(super) fn build_task_state_changed_event(
    task: &TaskRunExecRow,
    projection: &TaskProjectionState,
) -> TaskStateChangedEvent {
    TaskStateChangedEvent {
        id: task.id.clone(),
        path: task.media_path.clone(),
        name: task.name.clone(),
        media_kind: task.media_kind.clone(),
        size_bytes: task.size_bytes.max(0) as u64,
        transcribe_status: projection.queue.transcribe_status.clone(),
        transcribe_progress: projection.queue.progress_percent,
        transcribe_segment_current: projection.queue.transcribe_segment_current,
        transcribe_segment_total: projection.queue.transcribe_segment_total,
        transcribe_phase: projection.queue.phase.clone(),
        transcribe_phase_detail: projection.queue.phase_detail.clone(),
        transcribe_error: projection.queue.transcribe_error.clone(),
        result_text: projection.editor.result_text.clone(),
        result_srt: projection.editor.result_srt.clone(),
        subtitle_segments_json: projection.editor.subtitle_segments_json.clone(),
    }
}

pub(super) async fn persist_task_context(
    pool: &SqlitePool,
    task_id: &str,
    context: &TaskContext,
    projection: &TaskProjectionState,
) -> Result<(), String> {
    let now = unix_now();
    let is_final = matches!(
        context.runtime.status,
        TaskRuntimeStatus::Failed | TaskRuntimeStatus::Completed
    );
    persist_task_projection(pool, task_id, &context.runtime, projection, now, is_final).await?;

    let snapshots = [
        (STAGE_INIT, &context.stages.init),
        (STAGE_SEPARATE, &context.stages.separate),
        (STAGE_ASR, &context.stages.asr),
        (STAGE_PUNCTUATE, &context.stages.punctuate),
        (STAGE_SEGMENT, &context.stages.segment),
        (STAGE_SUMMARIZE, &context.stages.summarize),
        (STAGE_TRANSLATE, &context.stages.translate),
        (STAGE_SEGMENT_OPTIMIZE, &context.stages.segment_optimize),
        (STAGE_COMPOSE, &context.stages.compose),
    ]
    .iter()
    .map(|(stage, envelope)| TaskStageSnapshot {
        stage: (*stage).to_string(),
        status: envelope.status.clone(),
        started_at: envelope.started_at,
        finished_at: envelope.finished_at,
        output: envelope.output.clone(),
        metrics: envelope.metrics.clone(),
        error_code: envelope.error.as_ref().map(|e| e.code.clone()).unwrap_or_default(),
        error_message: envelope.error.as_ref().map(|e| e.message.clone()).unwrap_or_default(),
    })
    .collect::<Vec<_>>();
    persist_task_stage_snapshots(pool, task_id, &snapshots, now).await?;

    Ok(())
}

pub(super) fn persist_task_context_boxed<'a>(
    pool: &'a SqlitePool,
    task_id: &'a str,
    context: &'a TaskContext,
    projection: &'a TaskProjectionState,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + 'a>> {
    Box::pin(persist_task_context(pool, task_id, context, projection))
}

pub(super) async fn load_task_runtime_error(pool: &SqlitePool, task_id: &str) -> Option<String> {
    let task_error = sqlx::query_scalar::<_, String>("SELECT error_message FROM task_runs WHERE id = ?")
        .bind(task_id)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten()?;
    if !task_error.trim().is_empty() {
        return Some(task_error);
    }
    sqlx::query_scalar::<_, String>(
        "SELECT error_message FROM task_stage_runs
         WHERE task_id = ? AND error_message <> ''
         ORDER BY updated_at DESC LIMIT 1",
    )
    .bind(task_id)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
