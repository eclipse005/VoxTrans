use sqlx::SqlitePool;
use std::sync::OnceLock;

static TASK_USAGE_POOL: OnceLock<SqlitePool> = OnceLock::new();

#[derive(Debug, Clone, Copy, Default)]
pub struct LlmTokenUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

pub fn init_task_usage_pool(pool: SqlitePool) {
    let _ = TASK_USAGE_POOL.set(pool);
}

pub async fn record_llm_usage(task_id: &str, phase: &str, usage: LlmTokenUsage) -> Result<(), String> {
    if task_id.trim().is_empty() {
        return Ok(());
    }
    if usage.total_tokens == 0 && usage.prompt_tokens == 0 && usage.completion_tokens == 0 {
        return Ok(());
    }
    let Some(pool) = TASK_USAGE_POOL.get() else {
        return Ok(());
    };
    let normalized_phase = normalize_phase(phase);
    sqlx::query(
        "INSERT INTO task_llm_usage_phase (
            task_id, phase, prompt_tokens, completion_tokens, total_tokens, created_at, updated_at
         ) VALUES (?, ?, ?, ?, ?, strftime('%s','now'), strftime('%s','now'))
         ON CONFLICT(task_id, phase) DO UPDATE SET
           prompt_tokens = prompt_tokens + excluded.prompt_tokens,
           completion_tokens = completion_tokens + excluded.completion_tokens,
           total_tokens = total_tokens + excluded.total_tokens,
           updated_at = strftime('%s','now')",
    )
    .bind(task_id)
    .bind(normalized_phase)
    .bind(usage.prompt_tokens as i64)
    .bind(usage.completion_tokens as i64)
    .bind(usage.total_tokens as i64)
    .execute(pool)
    .await
    .map_err(|err| err.to_string())?;
    Ok(())
}

pub fn record_llm_usage_best_effort(task_id: &str, phase: &str, usage: LlmTokenUsage) {
    let task_id = task_id.to_string();
    let phase = phase.to_string();
    tauri::async_runtime::spawn(async move {
        let _ = record_llm_usage(&task_id, &phase, usage).await;
    });
}

pub async fn get_task_total_tokens(task_id: &str) -> Result<u64, String> {
    if task_id.trim().is_empty() {
        return Ok(0);
    }
    let Some(pool) = TASK_USAGE_POOL.get() else {
        return Ok(0);
    };
    let total = match sqlx::query_scalar::<_, i64>(
        "SELECT COALESCE(SUM(total_tokens), 0) FROM task_llm_usage_phase WHERE task_id = ?",
    )
        .bind(task_id)
        .fetch_optional(pool)
        .await
    {
        Ok(v) => v.unwrap_or(0),
        Err(_) => 0,
    };
    Ok(total.max(0) as u64)
}

fn normalize_phase(phase: &str) -> String {
    let trimmed = phase.trim().to_lowercase();
    if trimmed.is_empty() {
        "unknown".to_string()
    } else {
        trimmed
    }
}
