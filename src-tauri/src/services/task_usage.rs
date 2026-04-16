#[derive(Debug, Clone, Copy, Default)]
pub struct LlmTokenUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

pub async fn record_llm_usage(
    _task_id: &str,
    _phase: &str,
    _usage: LlmTokenUsage,
) -> Result<(), String> {
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
    let _ = task_id;
    Ok(0)
}
