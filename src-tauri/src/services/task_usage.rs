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

    let _ = crate::commands::workspace::add_task_total_tokens(task_id, normalized_total)?;
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
    crate::commands::workspace::get_task_total_tokens_from_workspace(task_id)
}
