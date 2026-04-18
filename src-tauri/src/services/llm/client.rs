use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::time::Duration;
use tokio::time::sleep;

use crate::services::task_log::TaskLogger;
use crate::services::task_usage::{LlmTokenUsage as TaskUsage, record_llm_usage_best_effort};

use super::cache::{append_cache_entry, read_cache_hit};
use super::error::{LlmError, LlmErrorKind};
use super::json_guard::{
    JsonRepairOutcome, JsonResponseValidator, extract_and_repair_json_with_outcome,
};
use super::port::{LlmCallContext, LlmConfig, LlmJsonResult, LlmPort, LlmTokenUsage};

const LLM_PROVIDER: &str = "openai_compatible_http";
const LLM_TRANSPORT: &str = "chat_completions";
const RETRY_HINT_MAX_CHARS: usize = 320;
const REPAIR_RAW_TEXT_MAX_CHARS: usize = 8_000;
const EPHEMERAL_PHASE_CONNECTIVITY_TEST: &str = "connectivity_test";

#[derive(Clone)]
pub struct OpenAiCompatLlmClient {
    config: LlmConfig,
    http: reqwest::Client,
}

#[derive(Debug, Clone)]
pub enum LlmSemanticValidationError {
    Retryable(String),
}

impl LlmSemanticValidationError {
    pub fn retryable(message: impl Into<String>) -> Self {
        Self::Retryable(message.into())
    }
}

#[derive(Debug, Clone)]
pub struct LlmValidatedJsonResult<T> {
    pub value: T,
}

#[derive(Debug, Clone)]
struct RetryFeedback {
    error_kind: String,
    retryable: bool,
    retry_hint: Option<String>,
    detail: String,
}

impl OpenAiCompatLlmClient {
    pub fn new(config: LlmConfig) -> Result<Self, LlmError> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .map_err(|err| {
                LlmError::new(
                    LlmErrorKind::Config,
                    format!("failed to create http client: {err}"),
                )
            })?;
        Ok(Self { config, http })
    }

    fn should_persist_artifacts(context: &LlmCallContext) -> bool {
        context.phase != EPHEMERAL_PHASE_CONNECTIVITY_TEST
    }

    fn logger_for_context(context: &LlmCallContext) -> Option<TaskLogger> {
        if !Self::should_persist_artifacts(context) {
            return None;
        }
        match context.media_path.as_deref() {
            Some(path) if !path.trim().is_empty() => {
                Some(TaskLogger::llm_with_media(context.task_id.clone(), path.to_string()))
            }
            _ => Some(TaskLogger::llm(context.task_id.clone())),
        }
    }

    async fn call_once(&self, user_prompt: &str) -> Result<(String, LlmTokenUsage), LlmError> {
        let request = ChatCompletionsRequest {
            model: self.config.model.clone(),
            messages: vec![ChatMessageRequest {
                role: "user".to_string(),
                content: user_prompt.to_string(),
            }],
            temperature: 0.2,
            stream: false,
        };
        let endpoint = chat_completions_endpoint(&self.config.base_url);
        let response = self
            .http
            .post(&endpoint)
            .bearer_auth(self.config.api_key.trim())
            .json(&request)
            .send()
            .await
            .map_err(|err| {
                LlmError::new(LlmErrorKind::Http, format!("http request failed: {err}"))
            })?;
        let status = response.status();
        let text = response.text().await.map_err(|err| {
            LlmError::new(
                LlmErrorKind::Http,
                format!("http response read failed: {err}"),
            )
        })?;
        if !status.is_success() {
            return Err(LlmError::new(
                LlmErrorKind::Http,
                format!("http status {}: {}", status.as_u16(), text),
            ));
        }
        let parsed: ChatCompletionsResponse = serde_json::from_str(&text).map_err(|err| {
            LlmError::new(
                LlmErrorKind::Http,
                format!("chat completion decode failed: {err}; raw={text}"),
            )
        })?;
        let content = parsed
            .choices
            .first()
            .and_then(|choice| extract_text_content(&choice.message.content))
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                LlmError::new(
                    LlmErrorKind::Http,
                    "response missing assistant text content",
                )
            })?;
        let usage = LlmTokenUsage {
            prompt_tokens: parsed.usage.as_ref().map(|u| u.prompt_tokens).unwrap_or(0),
            completion_tokens: parsed
                .usage
                .as_ref()
                .map(|u| u.completion_tokens)
                .unwrap_or(0),
            total_tokens: parsed.usage.as_ref().map(|u| u.total_tokens).unwrap_or(0),
        };
        Ok((content, usage))
    }

    pub async fn call_json_validated<T, F>(
        &self,
        context: &LlmCallContext,
        request_id: &str,
        user_prompt: &str,
        response_validator: Option<&JsonResponseValidator>,
        semantic_validate: F,
    ) -> Result<LlmValidatedJsonResult<T>, LlmError>
    where
        F: Fn(Value) -> Result<T, LlmSemanticValidationError>,
    {
        let logger = Self::logger_for_context(context);
        let persist_artifacts = Self::should_persist_artifacts(context);

        if persist_artifacts
            && let Some(cache_hit) =
            read_cache_hit(context, &self.config, "", user_prompt, response_validator)
        {
            if let Some(validator) = response_validator {
                if let Err(err) = validator.validate(&cache_hit.json) {
                    if let Some(logger) = logger.as_ref() {
                        logger.event(
                            "llm.call",
                            Some(&json!({
                                "status": "cache_invalid_schema",
                                "error": err.message,
                                "phase": context.phase,
                                "llmId": request_id,
                            })),
                        );
                    }
                } else {
                    match semantic_validate(cache_hit.json.clone()) {
                        Ok(value) => {
                            if let Some(logger) = logger.as_ref() {
                                logger.event(
                                    "llm.call",
                                    Some(&json!({
                                        "status": "cache_hit",
                                        "model": self.config.model,
                                        "baseUrl": normalize_base_url(&self.config.base_url),
                                        "provider": LLM_PROVIDER,
                                        "transport": LLM_TRANSPORT,
                                        "phase": context.phase,
                                        "llmId": request_id,
                                    })),
                                );
                            }
                            return Ok(LlmValidatedJsonResult { value });
                        }
                        Err(LlmSemanticValidationError::Retryable(message)) => {
                            if let Some(logger) = logger.as_ref() {
                                logger.event(
                                    "llm.call",
                                    Some(&json!({
                                        "status": "cache_invalid_semantic",
                                        "error": message,
                                        "phase": context.phase,
                                        "llmId": request_id,
                                    })),
                                );
                            }
                        }
                    }
                }
            } else if let Ok(value) = semantic_validate(cache_hit.json.clone()) {
                if let Some(logger) = logger.as_ref() {
                    logger.event(
                        "llm.call",
                        Some(&json!({
                            "status": "cache_hit",
                            "model": self.config.model,
                            "baseUrl": normalize_base_url(&self.config.base_url),
                            "provider": LLM_PROVIDER,
                            "transport": LLM_TRANSPORT,
                            "phase": context.phase,
                            "llmId": request_id,
                        })),
                    );
                }
                return Ok(LlmValidatedJsonResult { value });
            }
        }

        let max_attempts = self.config.max_retries.saturating_add(1).max(1);
        let started = std::time::Instant::now();
        let mut last_error = String::new();
        let mut last_feedback: Option<RetryFeedback> = None;
        let mut validation_failures = 0u32;

        for attempt in 1..=max_attempts {
            let effective_user_prompt = augment_user_prompt_with_retry_feedback(
                user_prompt,
                attempt,
                max_attempts,
                last_feedback.as_ref(),
            );
            let base_payload = json!({
                "attempt": attempt,
                "maxAttempts": max_attempts,
                "model": self.config.model,
                "baseUrl": normalize_base_url(&self.config.base_url),
                "provider": LLM_PROVIDER,
                "transport": LLM_TRANSPORT,
                "phase": context.phase,
                "llmId": request_id,
                "request": {
                    "userPrompt": effective_user_prompt.clone(),
                }
            });

            match self.call_once(&effective_user_prompt).await {
                Ok((raw_text, usage)) => {
                    let mut parsed = match extract_and_repair_json_with_outcome(&raw_text) {
                        Ok(v) => v,
                        Err(err) => {
                            validation_failures += 1;
                            match self
                                .repair_json_response(
                                    context,
                                    request_id,
                                    user_prompt,
                                    response_validator,
                                    &raw_text,
                                    &err,
                                )
                                .await
                            {
                                Ok(repaired) => repaired,
                                Err(repair_err) => {
                                    let feedback = feedback_from_llm_error(&repair_err);
                                    last_error = feedback.detail.clone();
                                    last_feedback = Some(feedback.clone());
                                    if let Some(logger) = logger.as_ref() {
                                        logger.event(
                                            "llm.call",
                                            Some(&json!({
                                                "status": "invalid_json",
                                                "error": last_error,
                                                "errorKind": feedback.error_kind,
                                                "retryable": false,
                                                "retryHint": feedback.retry_hint,
                                                "repairMode": "llm_json_repair",
                                                "response": { "text": raw_text },
                                                "attempt": base_payload["attempt"],
                                                "maxAttempts": base_payload["maxAttempts"],
                                                "model": base_payload["model"],
                                                "baseUrl": base_payload["baseUrl"],
                                                "provider": base_payload["provider"],
                                                "transport": base_payload["transport"],
                                                "phase": base_payload["phase"],
                                                "llmId": base_payload["llmId"],
                                                "request": base_payload["request"],
                                            })),
                                        );
                                    }
                                    break;
                                }
                            }
                        }
                    };

                    if let Some(validator) = response_validator {
                        if let Err(err) = validator.validate(&parsed.value) {
                            validation_failures += 1;
                            match self
                                .repair_json_response(
                                    context,
                                    request_id,
                                    user_prompt,
                                    response_validator,
                                    &raw_text,
                                    &err,
                                )
                                .await
                            {
                                Ok(repaired) => parsed = repaired,
                                Err(repair_err) => {
                                    let feedback = feedback_from_llm_error(&repair_err);
                                    last_error = feedback.detail.clone();
                                    last_feedback = Some(feedback.clone());
                                    if let Some(logger) = logger.as_ref() {
                                        logger.event(
                                            "llm.call",
                                            Some(&json!({
                                                "status": "invalid_schema",
                                                "error": last_error,
                                                "errorKind": feedback.error_kind,
                                                "retryable": false,
                                                "retryHint": feedback.retry_hint,
                                                "repairMode": "llm_json_repair",
                                                "response": { "text": raw_text },
                                                "attempt": base_payload["attempt"],
                                                "maxAttempts": base_payload["maxAttempts"],
                                                "model": base_payload["model"],
                                                "baseUrl": base_payload["baseUrl"],
                                                "provider": base_payload["provider"],
                                                "transport": base_payload["transport"],
                                                "phase": base_payload["phase"],
                                                "llmId": base_payload["llmId"],
                                                "request": base_payload["request"],
                                            })),
                                        );
                                    }
                                    break;
                                }
                            }
                        }
                    }

                    match semantic_validate(parsed.value.clone()) {
                        Ok(value) => {
                            if persist_artifacts {
                                append_cache_entry(
                                    context,
                                    &self.config,
                                    "",
                                    user_prompt,
                                    response_validator,
                                    &raw_text,
                                    &parsed.value,
                                    &usage,
                                );
                            }
                            let elapsed_ms = started.elapsed().as_millis();
                            if let Some(logger) = logger.as_ref() {
                                logger.event(
                                    "llm.call",
                                    Some(&json!({
                                        "status": if validation_failures > 0 { "ok_after_repair" } else { "ok" },
                                        "repairMode": if validation_failures > 0 { Some("llm_json_repair") } else { None::<&str> },
                                        "validationFailures": validation_failures,
                                        "localRepairSource": parsed.source.as_str(),
                                        "attempt": base_payload["attempt"],
                                        "maxAttempts": base_payload["maxAttempts"],
                                        "model": base_payload["model"],
                                        "baseUrl": base_payload["baseUrl"],
                                        "provider": base_payload["provider"],
                                        "transport": base_payload["transport"],
                                        "phase": base_payload["phase"],
                                        "llmId": base_payload["llmId"],
                                        "request": base_payload["request"],
                                        "response": { "text": raw_text },
                                        "elapsedMs": elapsed_ms,
                                        "usage": {
                                            "promptTokens": usage.prompt_tokens,
                                            "completionTokens": usage.completion_tokens,
                                            "totalTokens": usage.total_tokens,
                                        }
                                    })),
                                );
                            }
                            if persist_artifacts {
                                record_llm_usage_best_effort(
                                    &context.task_id,
                                    &context.phase,
                                    TaskUsage {
                                        prompt_tokens: usage.prompt_tokens,
                                        completion_tokens: usage.completion_tokens,
                                        total_tokens: usage.total_tokens,
                                    },
                                );
                            }

                            return Ok(LlmValidatedJsonResult { value });
                        }
                        Err(LlmSemanticValidationError::Retryable(message)) => {
                            let feedback = feedback_for_semantic(message);
                            last_error = feedback.detail.clone();
                            last_feedback = Some(feedback.clone());
                            let backoff_ms = retry_backoff_ms(attempt, max_attempts);
                            if let Some(logger) = logger.as_ref() {
                                logger.event(
                                    "llm.call",
                                    Some(&json!({
                                        "status": "invalid_semantic",
                                        "error": last_error,
                                        "errorKind": feedback.error_kind,
                                        "retryable": feedback.retryable,
                                        "retryHint": feedback.retry_hint,
                                        "response": { "text": raw_text },
                                        "attempt": base_payload["attempt"],
                                        "maxAttempts": base_payload["maxAttempts"],
                                        "backoffMs": backoff_ms,
                                        "model": base_payload["model"],
                                        "baseUrl": base_payload["baseUrl"],
                                        "provider": base_payload["provider"],
                                        "transport": base_payload["transport"],
                                        "phase": base_payload["phase"],
                                        "llmId": base_payload["llmId"],
                                        "request": base_payload["request"],
                                    })),
                                );
                            }
                            if let Some(delay) = backoff_ms {
                                sleep(Duration::from_millis(delay)).await;
                            }
                            continue;
                        }
                    }
                }
                Err(err) => {
                    let feedback = feedback_from_llm_error(&err);
                    last_error = feedback.detail.clone();
                    last_feedback = Some(feedback.clone());
                    let backoff_ms = if feedback.retryable {
                        retry_backoff_ms(attempt, max_attempts)
                    } else {
                        None
                    };
                    if let Some(logger) = logger.as_ref() {
                        logger.event(
                            "llm.call",
                            Some(&json!({
                                "status": "http_error",
                                "error": last_error,
                                "errorKind": feedback.error_kind,
                                "retryable": feedback.retryable,
                                "retryHint": feedback.retry_hint,
                                "attempt": base_payload["attempt"],
                                "maxAttempts": base_payload["maxAttempts"],
                                "backoffMs": backoff_ms,
                                "model": base_payload["model"],
                                "baseUrl": base_payload["baseUrl"],
                                "provider": base_payload["provider"],
                                "transport": base_payload["transport"],
                                "phase": base_payload["phase"],
                                "llmId": base_payload["llmId"],
                                "request": base_payload["request"],
                            })),
                        );
                    }
                    if !feedback.retryable {
                        break;
                    }
                    if let Some(delay) = backoff_ms {
                        sleep(Duration::from_millis(delay)).await;
                    }
                }
            }
        }

        let exhausted_feedback = last_feedback.unwrap_or_else(|| RetryFeedback {
            error_kind: LlmErrorKind::InvalidSemantic.as_str().to_string(),
            retryable: true,
            retry_hint: None,
            detail: last_error.clone(),
        });

        let retry_hint_suffix = exhausted_feedback
            .retry_hint
            .as_ref()
            .filter(|hint| !hint.eq_ignore_ascii_case(exhausted_feedback.detail.as_str()))
            .map(|hint| format!("; retry_hint={hint}"))
            .unwrap_or_default();

        Err(LlmError::new(
            LlmErrorKind::InvalidSemantic,
            format!(
                "llm call failed after {} attempts: kind={}{}",
                max_attempts, exhausted_feedback.error_kind, retry_hint_suffix
            ),
        ))
    }
}

impl OpenAiCompatLlmClient {
    async fn repair_json_response(
        &self,
        context: &LlmCallContext,
        request_id: &str,
        original_prompt: &str,
        response_validator: Option<&JsonResponseValidator>,
        raw_text: &str,
        failure: &LlmError,
    ) -> Result<JsonRepairOutcome, LlmError> {
        let logger = Self::logger_for_context(context);

        let repair_prompt =
            build_json_repair_prompt(original_prompt, response_validator, raw_text, failure);
        if let Some(logger) = logger.as_ref() {
            logger.event(
                "llm.call",
                Some(&json!({
                    "status": "repair_requested",
                    "repairMode": "llm_json_repair",
                    "errorKind": failure.kind.as_str(),
                    "phase": context.phase,
                    "llmId": request_id,
                })),
            );
        }

        let (repair_raw_text, _) = self.call_once(&repair_prompt).await?;
        let repaired = extract_and_repair_json_with_outcome(&repair_raw_text)?;
        if let Some(validator) = response_validator {
            validator.validate(&repaired.value)?;
        }
        Ok(repaired)
    }
}

impl LlmPort for OpenAiCompatLlmClient {
    async fn call_json(
        &self,
        context: &LlmCallContext,
        request_id: &str,
        user_prompt: &str,
        response_validator: Option<&JsonResponseValidator>,
    ) -> Result<LlmJsonResult, LlmError> {
        let result = self
            .call_json_validated(
                context,
                request_id,
                user_prompt,
                response_validator,
                Ok::<Value, LlmSemanticValidationError>,
            )
            .await?;
        Ok(LlmJsonResult { json: result.value })
    }
}

fn retry_backoff_ms(attempt: u32, max_attempts: u32) -> Option<u64> {
    if attempt >= max_attempts {
        return None;
    }
    let exp = attempt.saturating_sub(1).min(2);
    let base_ms = 2_000u64;
    Some(base_ms.saturating_mul(1u64 << exp))
}

fn normalize_base_url(base_url: &str) -> String {
    base_url.trim().trim_end_matches('/').to_string()
}

fn chat_completions_endpoint(base_url: &str) -> String {
    let normalized = normalize_base_url(base_url);
    if normalized.ends_with("/chat/completions") {
        normalized
    } else {
        format!("{normalized}/chat/completions")
    }
}

fn feedback_for_semantic(message: String) -> RetryFeedback {
    let hint = compact_hint(&message, RETRY_HINT_MAX_CHARS);
    RetryFeedback {
        error_kind: LlmErrorKind::InvalidSemantic.as_str().to_string(),
        retryable: true,
        retry_hint: Some(hint),
        detail: message,
    }
}

fn feedback_from_llm_error(err: &LlmError) -> RetryFeedback {
    let retryable = !matches!(err.kind, LlmErrorKind::Config);
    let retry_hint = retry_hint_from_error(err.kind, &err.message);
    let detail = retry_hint
        .clone()
        .unwrap_or_else(|| compact_hint(&err.message, RETRY_HINT_MAX_CHARS));
    RetryFeedback {
        error_kind: err.kind.as_str().to_string(),
        retryable,
        retry_hint,
        detail,
    }
}

fn retry_hint_from_error(kind: LlmErrorKind, message: &str) -> Option<String> {
    let trimmed = message.trim();
    if trimmed.is_empty() {
        return None;
    }

    let hint = match kind {
        LlmErrorKind::InvalidSchema => {
            strip_prefix_case_insensitive(trimmed, "schema check failed:")
                .unwrap_or(trimmed)
                .trim()
        }
        LlmErrorKind::InvalidJson => {
            let detail = strip_prefix_case_insensitive(
                trimmed,
                "failed to extract valid json from llm response:",
            )
            .unwrap_or(trimmed)
            .trim();
            return Some(compact_invalid_json_hint(detail, RETRY_HINT_MAX_CHARS));
        }
        LlmErrorKind::InvalidSemantic => trimmed,
        _ => return None,
    };

    if hint.is_empty() {
        None
    } else {
        Some(compact_hint(hint, RETRY_HINT_MAX_CHARS))
    }
}

fn compact_invalid_json_hint(detail: &str, max_chars: usize) -> String {
    let mut reasons: Vec<String> = Vec::new();
    for part in detail.split('|') {
        let mut item = part.trim();
        if item.is_empty() {
            continue;
        }
        if let Some((head, _)) = item.split_once("; near:") {
            item = head.trim();
        }
        if let Some((head, _)) = item.split_once("; raw preview:") {
            item = head.trim();
        }
        item = strip_prefix_case_insensitive(item, "candidate parse failed:")
            .unwrap_or(item)
            .trim();
        item = strip_prefix_case_insensitive(item, "repaired candidate parse failed:")
            .unwrap_or(item)
            .trim();
        if item.is_empty() {
            continue;
        }
        let normalized = compact_hint(item, max_chars);
        if !reasons
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(&normalized))
        {
            reasons.push(normalized);
        }
    }

    if reasons.is_empty() {
        return compact_hint(detail, max_chars);
    }
    compact_hint(&reasons.join("; "), max_chars)
}

fn compact_hint(input: &str, max_chars: usize) -> String {
    let normalized = input
        .replace('\r', " ")
        .replace('\n', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    truncate_chars(&normalized, max_chars)
}

fn truncate_chars(input: &str, max_chars: usize) -> String {
    if input.chars().count() <= max_chars {
        return input.to_string();
    }
    let mut out = String::new();
    for (index, ch) in input.chars().enumerate() {
        if index >= max_chars {
            break;
        }
        out.push(ch);
    }
    out.push_str("...");
    out
}

fn strip_prefix_case_insensitive<'a>(input: &'a str, prefix: &str) -> Option<&'a str> {
    if input.len() < prefix.len() {
        return None;
    }
    let (head, tail) = input.split_at(prefix.len());
    if head.eq_ignore_ascii_case(prefix) {
        Some(tail)
    } else {
        None
    }
}

fn augment_user_prompt_with_retry_feedback(
    base_user_prompt: &str,
    attempt: u32,
    max_attempts: u32,
    last_feedback: Option<&RetryFeedback>,
) -> String {
    if attempt <= 1 {
        return base_user_prompt.to_string();
    }

    let Some(feedback) = last_feedback else {
        return base_user_prompt.to_string();
    };
    let Some(hint) = feedback.retry_hint.as_ref() else {
        return base_user_prompt.to_string();
    };
    if !feedback.retryable {
        return base_user_prompt.to_string();
    }

    format!(
        "{base_user_prompt}\n\n# Retry Constraint\nPrevious output validation failed ({attempt}/{max_attempts}): {hint}\nReturn a complete corrected JSON response only. Do not add any markdown, explanations, or extra text."
    )
}

fn build_json_repair_prompt(
    original_prompt: &str,
    response_validator: Option<&JsonResponseValidator>,
    raw_text: &str,
    failure: &LlmError,
) -> String {
    let schema_constraints = response_validator
        .map(|validator| validator.describe_constraints())
        .unwrap_or_else(|| "Return one valid JSON value.".to_string());
    let failure_hint = retry_hint_from_error(failure.kind, &failure.message)
        .unwrap_or_else(|| compact_hint(&failure.message, RETRY_HINT_MAX_CHARS));
    let original_prompt = truncate_chars(original_prompt.trim(), 2_000);
    let raw_text = truncate_chars(raw_text.trim(), REPAIR_RAW_TEXT_MAX_CHARS);

    format!(
        concat!(
            "You are a JSON repair tool.\n",
            "Your job is to repair the candidate response into valid JSON without redoing the task.\n",
            "Preserve the original intent and fields whenever possible.\n",
            "Do not add explanations, markdown fences, or commentary.\n",
            "If the candidate is partially valid, minimally fix it.\n",
            "If a field is missing but can be copied from the candidate, keep it.\n",
            "If something cannot be inferred, use the smallest safe JSON value instead of inventing extra content.\n\n",
            "Target constraints:\n",
            "{schema_constraints}\n\n",
            "Validation failure:\n",
            "{failure_hint}\n\n",
            "Original task prompt:\n",
            "{original_prompt}\n\n",
            "Candidate response to repair:\n",
            "{raw_text}\n\n",
            "Return repaired JSON only."
        ),
        schema_constraints = schema_constraints,
        failure_hint = failure_hint,
        original_prompt = original_prompt,
        raw_text = raw_text,
    )
}

#[derive(Debug, Serialize)]
struct ChatCompletionsRequest {
    model: String,
    messages: Vec<ChatMessageRequest>,
    temperature: f64,
    stream: bool,
}

#[derive(Debug, Serialize)]
struct ChatMessageRequest {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionsResponse {
    choices: Vec<ChatChoice>,
    usage: Option<ChatUsage>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatMessageResponse,
}

#[derive(Debug, Deserialize)]
struct ChatMessageResponse {
    content: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct ChatUsage {
    prompt_tokens: u64,
    completion_tokens: u64,
    total_tokens: u64,
}

fn extract_text_content(content: &serde_json::Value) -> Option<String> {
    if let Some(text) = content.as_str() {
        return Some(text.to_string());
    }
    let arr = content.as_array()?;
    let mut out = String::new();
    for part in arr {
        let maybe_text = part
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        if maybe_text.trim().is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(maybe_text);
    }
    if out.trim().is_empty() {
        None
    } else {
        Some(out)
    }
}
