use crate::db::store::TaskStore;

#[derive(Debug, Clone, Copy, Default)]
pub struct LlmTokenUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

pub async fn record_llm_usage(
    task_id: &str,
    _phase: &str,
    usage: LlmTokenUsage,
    store: Option<TaskStore>,
) -> Result<(), String> {
    let task_id = task_id.trim();
    if task_id.is_empty() {
        return Ok(());
    }

    let normalized_total = if usage.total_tokens > 0 {
        usage.total_tokens
    } else {
        usage.prompt_tokens.saturating_add(usage.completion_tokens)
    };
    if normalized_total == 0 {
        return Ok(());
    }

    let new_total = crate::commands::workspace::add_task_total_tokens(task_id, normalized_total)?;
    if let Some(store) = store {
        if let Err(e) = store.update_task_tokens(task_id, new_total).await {
            // DB write failure leaves the DB count behind the in-memory
            // total until the next persist_task_meta upsert rewrites it
            // from the record. Log so it's diagnosable instead of being
            // silently swallowed.
            eprintln!("warn: persist token count for task {task_id} failed: {e}");
        }
    }
    Ok(())
}

pub fn record_llm_usage_best_effort(
    task_id: &str,
    phase: &str,
    usage: LlmTokenUsage,
    store: Option<TaskStore>,
) {
    let task_id = task_id.to_string();
    let phase = phase.to_string();
    tauri::async_runtime::spawn(async move {
        let _ = record_llm_usage(&task_id, &phase, usage, store).await;
    });
}

pub async fn get_task_total_tokens(task_id: &str, store: &TaskStore) -> Result<u64, String> {
    let task_id = task_id.trim();
    if task_id.is_empty() {
        return Ok(0);
    }
    // DB-first: the SQLite row is the durable source of truth. The
    // in-memory workspace mirror may lag if the hydrate hasn't run
    // yet, or if a persist failed and we're recovering from log.
    store.get_task_total_tokens(task_id).await
}
