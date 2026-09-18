use serde_json::{Value, json};

use crate::services::task_log::TaskLogger;

use super::base_url::normalize_base_url;
use super::port::{LlmCallContext, LlmConfig, LlmTokenUsage};

const EPHEMERAL_PHASE_CONNECTIVITY_TEST: &str = "connectivity_test";
const ATTEMPT_PAYLOAD_KEYS: &[&str] = &[
    "attempt",
    "maxAttempts",
    "model",
    "baseUrl",
    "provider",
    "transport",
    "phase",
    "llmId",
    "request",
];

pub(super) fn logger_for_context(context: &LlmCallContext) -> Option<TaskLogger> {
    if context.phase == EPHEMERAL_PHASE_CONNECTIVITY_TEST {
        return None;
    }
    match context.media_path.as_deref() {
        Some(path) if !path.trim().is_empty() => Some(TaskLogger::llm_with_media(
            context.task_id.clone(),
            path.to_string(),
        )),
        _ => Some(TaskLogger::llm(context.task_id.clone())),
    }
}

pub(super) fn log_llm_call(logger: Option<&TaskLogger>, payload: Value) {
    if let Some(logger) = logger {
        logger.event("llm.call", Some(&payload));
    }
}

pub(super) fn attempt_base_payload(
    config: &LlmConfig,
    context: &LlmCallContext,
    request_id: &str,
    attempt: u32,
    max_attempts: u32,
    user_prompt: &str,
) -> Value {
    // Record the dialect actually in use, not a fixed label: the same profile
    // can resolve to either one, and a task log that says "anthropic" for a
    // chat-completions call sends debugging in the wrong direction.
    let protocol = config.capabilities.resolve_protocol(&config.base_url, &config.model);
    json!({
        "attempt": attempt,
        "maxAttempts": max_attempts,
        "model": &config.model,
        "baseUrl": normalize_base_url(&config.base_url),
        "provider": protocol.provider_id(),
        "transport": protocol.transport_id(),
        "phase": &context.phase,
        "llmId": request_id,
        "request": {
            "userPrompt": user_prompt,
        }
    })
}

pub(super) fn attempt_event_payload(base_payload: &Value, mut payload: Value) -> Value {
    if let Some(map) = payload.as_object_mut() {
        for key in ATTEMPT_PAYLOAD_KEYS {
            if let Some(value) = base_payload.get(*key) {
                map.insert((*key).to_string(), value.clone());
            }
        }
    }
    payload
}

#[allow(clippy::too_many_arguments)]
pub(super) fn failed_attempt_payload(
    base_payload: &Value,
    status: &str,
    error: &str,
    error_kind: &str,
    retryable: bool,
    retry_hint: Option<&str>,
    raw_text: Option<&str>,
    backoff_ms: Option<u64>,
) -> Value {
    attempt_event_payload(
        base_payload,
        json!({
            "status": status,
            "error": error,
            "errorKind": error_kind,
            "retryable": retryable,
            "retryHint": retry_hint,
            "response": { "text": raw_text },
            "backoffMs": backoff_ms,
        }),
    )
}

pub(super) fn success_attempt_payload(
    base_payload: &Value,
    local_repair_source: &str,
    raw_text: &str,
    elapsed_ms: u128,
    usage: &LlmTokenUsage,
) -> Value {
    attempt_event_payload(
        base_payload,
        json!({
            "status": "ok",
            "localRepairSource": local_repair_source,
            "response": { "text": raw_text },
            "elapsedMs": elapsed_ms,
            "usage": {
                "promptTokens": usage.prompt_tokens,
                "completionTokens": usage.completion_tokens,
                "totalTokens": usage.total_tokens,
                "cacheReadTokens": usage.cache_read_tokens,
                "cacheCreationTokens": usage.cache_creation_tokens,
            }
        }),
    )
}

pub(super) fn invalid_semantic_attempt_payload(
    base_payload: &Value,
    error: &str,
    error_kind: &str,
    retryable: bool,
    retry_hint: Option<&str>,
    raw_text: &str,
    backoff_ms: Option<u64>,
) -> Value {
    attempt_event_payload(
        base_payload,
        json!({
            "status": "invalid_semantic",
            "error": error,
            "errorKind": error_kind,
            "retryable": retryable,
            "retryHint": retry_hint,
            "response": { "text": raw_text },
            "backoffMs": backoff_ms,
        }),
    )
}

pub(super) fn http_error_attempt_payload(
    base_payload: &Value,
    error: &str,
    error_kind: &str,
    retryable: bool,
    retry_hint: Option<&str>,
    backoff_ms: Option<u64>,
) -> Value {
    attempt_event_payload(
        base_payload,
        json!({
            "status": "http_error",
            "error": error,
            "errorKind": error_kind,
            "retryable": retryable,
            "retryHint": retry_hint,
            "backoffMs": backoff_ms,
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::llm::capabilities::CapabilityMemo;
    use std::sync::Arc;

    fn context() -> LlmCallContext {
        LlmCallContext {
            task_id: "log-label-test".to_string(),
            media_path: None,
            phase: "translate".to_string(),
            store: None,
            output_schema: None,
        }
    }

    #[test]
    fn attempt_payload_labels_follow_the_resolved_dialect() {
        // Same log shape for both dialects — only the labels move, so a task
        // log never claims "anthropic" for a chat-completions call.
        let config = LlmConfig::new(
            "http://log-label.local/anthropic".to_string(),
            "k".to_string(),
            "claude-sonnet-5".to_string(),
        )
        .with_capabilities(Arc::new(CapabilityMemo::new()));
        let payload = attempt_base_payload(&config, &context(), "req-1", 1, 3, "hi");
        assert_eq!(payload["provider"], json!("anthropic"));
        assert_eq!(payload["transport"], json!("messages"));

        // A URL that carries the OpenAI route outright resolves there.
        let config = LlmConfig::new(
            "http://log-label-chat.local/v1/chat/completions".to_string(),
            "k".to_string(),
            "deepseek/oss-model".to_string(),
        )
        .with_capabilities(Arc::new(CapabilityMemo::new()));
        let payload = attempt_base_payload(&config, &context(), "req-2", 1, 3, "hi");
        assert_eq!(payload["provider"], json!("openai"));
        assert_eq!(payload["transport"], json!("chat_completions"));
        assert_eq!(payload["model"], json!("deepseek/oss-model"));
        assert_eq!(payload["request"]["userPrompt"], json!("hi"));
    }

    #[test]
    fn connectivity_test_phase_is_never_written_to_a_task_log() {
        let mut ctx = context();
        ctx.task_id = "settings-llm-test".to_string();
        ctx.phase = "connectivity_test".to_string();
        assert!(logger_for_context(&ctx).is_none());
    }
}


