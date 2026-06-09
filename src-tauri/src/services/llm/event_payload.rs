use serde_json::{Value, json};

use crate::services::task_log::TaskLogger;

use super::base_url::normalize_base_url;
use super::port::{LlmCallContext, LlmConfig, LlmTokenUsage};

const LLM_PROVIDER: &str = "openai_compatible_http";
const LLM_TRANSPORT: &str = "chat_completions";
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
    json!({
        "attempt": attempt,
        "maxAttempts": max_attempts,
        "model": &config.model,
        "baseUrl": normalize_base_url(&config.base_url),
        "provider": LLM_PROVIDER,
        "transport": LLM_TRANSPORT,
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

pub(super) fn repair_failed_attempt_payload(
    base_payload: &Value,
    status: &str,
    error: &str,
    error_kind: &str,
    retry_hint: Option<&str>,
    raw_text: &str,
) -> Value {
    attempt_event_payload(
        base_payload,
        json!({
            "status": status,
            "error": error,
            "errorKind": error_kind,
            "retryable": false,
            "retryHint": retry_hint,
            "repairMode": "llm_json_repair",
            "response": { "text": raw_text },
        }),
    )
}

pub(super) fn success_attempt_payload(
    base_payload: &Value,
    validation_failures: u32,
    local_repair_source: &str,
    raw_text: &str,
    elapsed_ms: u128,
    usage: &LlmTokenUsage,
) -> Value {
    attempt_event_payload(
        base_payload,
        json!({
            "status": if validation_failures > 0 { "ok_after_repair" } else { "ok" },
            "repairMode": if validation_failures > 0 { Some("llm_json_repair") } else { None::<&str> },
            "validationFailures": validation_failures,
            "localRepairSource": local_repair_source,
            "response": { "text": raw_text },
            "elapsedMs": elapsed_ms,
            "usage": {
                "promptTokens": usage.prompt_tokens,
                "completionTokens": usage.completion_tokens,
                "totalTokens": usage.total_tokens,
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

pub(super) fn repair_requested_payload(
    error_kind: &str,
    context: &LlmCallContext,
    request_id: &str,
) -> Value {
    json!({
        "status": "repair_requested",
        "repairMode": "llm_json_repair",
        "errorKind": error_kind,
        "phase": &context.phase,
        "llmId": request_id,
    })
}
