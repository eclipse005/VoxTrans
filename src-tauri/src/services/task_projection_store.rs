use sqlx::SqlitePool;

use crate::services::task_context::RuntimeState;
use crate::services::task_projection::TaskProjectionState;

#[derive(Debug, Clone)]
pub struct TaskProjectionHydrationInput {
    pub overall_status: String,
    pub current_stage: String,
    pub progress_percent: i64,
    pub phase_detail: String,
    pub segment_current: i64,
    pub segment_total: i64,
    pub error_message: String,
    pub subtitle_segments_json: String,
    pub result_text: String,
    pub result_srt: String,
    pub translated_srt: String,
}

pub fn hydrate_task_projection(input: TaskProjectionHydrationInput) -> TaskProjectionState {
    let mut projection = TaskProjectionState::new();
    projection.set_queue(
        map_workspace_status(&input.overall_status),
        map_workspace_phase(&input.current_stage).as_str(),
        &input.phase_detail,
        input.progress_percent.clamp(0, 100) as u32,
        input.segment_current.max(0) as u32,
        input.segment_total.max(0) as u32,
        &input.error_message,
    );
    projection.set_editor(
        input.subtitle_segments_json,
        input.result_text,
        input.result_srt,
        input.translated_srt,
    );
    projection
}

pub async fn persist_task_projection(
    pool: &SqlitePool,
    task_id: &str,
    runtime: &RuntimeState,
    projection: &TaskProjectionState,
    now: i64,
    is_final: bool,
) -> Result<(), String> {
    sqlx::query(
        "UPDATE task_runs
         SET overall_status = ?,
             current_stage = ?,
             progress_percent = ?,
             phase_detail = ?,
             segment_current = ?,
             segment_total = ?,
             error_message = ?,
             result_text = ?,
             result_srt = ?,
             subtitle_segments_json = ?,
             translated_srt = ?,
             started_at = CASE
                 WHEN started_at IS NULL AND ? = 'running' THEN ?
                 ELSE started_at
             END,
             finished_at = CASE
                 WHEN ? = 1 THEN ?
                 ELSE NULL
             END,
             updated_at = ?
         WHERE id = ?",
    )
    .bind(normalize_overall_status(&runtime.status))
    .bind(&runtime.current_stage)
    .bind(runtime.progress_percent as i64)
    .bind(&projection.queue.phase_detail)
    .bind(projection.queue.transcribe_segment_current as i64)
    .bind(projection.queue.transcribe_segment_total as i64)
    .bind(&projection.queue.transcribe_error)
    .bind(&projection.editor.result_text)
    .bind(&projection.editor.result_srt)
    .bind(&projection.editor.subtitle_segments_json)
    .bind(&projection.editor.translated_srt)
    .bind(&runtime.status)
    .bind(now)
    .bind(if is_final { 1 } else { 0 })
    .bind(now)
    .bind(now)
    .bind(task_id)
    .execute(pool)
    .await
    .map_err(|err| err.to_string())?;
    Ok(())
}

fn normalize_overall_status(status: &str) -> &'static str {
    match status.trim().to_ascii_lowercase().as_str() {
        "running" => "running",
        "failed" => "failed",
        "completed" => "completed",
        "queued" => "queued",
        _ => "queued",
    }
}

fn map_workspace_status(status: &str) -> &'static str {
    match status.trim().to_ascii_lowercase().as_str() {
        "queued" => "queued",
        "running" => "processing",
        "completed" => "done",
        "failed" => "error",
        _ => "queued",
    }
}

fn map_workspace_phase(stage: &str) -> String {
    match stage.trim().to_ascii_lowercase().as_str() {
        "separate" => "separate".to_string(),
        "asr" => "transcribe".to_string(),
        "punctuate" => "punctuate".to_string(),
        "segment" => "segment".to_string(),
        "summarize" => "summarize".to_string(),
        "translate" => "translate".to_string(),
        "qa" => "qa".to_string(),
        "segment_optimize" => "segment_optimize".to_string(),
        "qa_quality" => "qa_quality".to_string(),
        "compose" => "compose".to_string(),
        "done" => "done".to_string(),
        _ => String::new(),
    }
}
