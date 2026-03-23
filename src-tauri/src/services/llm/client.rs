use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::time::Duration;
use tokio::time::sleep;

use crate::services::task_log::TaskLogger;
use crate::services::task_usage::{LlmTokenUsage as TaskUsage, record_llm_usage_best_effort};

use super::error::{LlmError, LlmErrorKind};
use super::json_guard::{JsonResponseValidator, extract_and_repair_json};
use super::port::{LlmCallContext, LlmConfig, LlmJsonResult, LlmPort, LlmTokenUsage};

const LLM_PROVIDER: &str = "openai_compatible_http";
const LLM_TRANSPORT: &str = "chat_completions";

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

impl OpenAiCompatLlmClient {
    pub fn new(config: LlmConfig) -> Result<Self, LlmError> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .map_err(|err| LlmError::new(LlmErrorKind::Config, format!("failed to create http client: {err}")))?;
        Ok(Self { config, http })
    }

    async fn call_once(
        &self,
        system_prompt: &str,
        user_prompt: &str,
    ) -> Result<(String, LlmTokenUsage), LlmError> {
        let request = ChatCompletionsRequest {
            model: self.config.model.clone(),
            messages: vec![
                ChatMessageRequest {
                    role: "system".to_string(),
                    content: system_prompt.to_string(),
                },
                ChatMessageRequest {
                    role: "user".to_string(),
                    content: user_prompt.to_string(),
                },
            ],
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
            .map_err(|err| LlmError::new(LlmErrorKind::Http, format!("http request failed: {err}")))?;
        let status = response.status();
        let text = response
            .text()
            .await
            .map_err(|err| LlmError::new(LlmErrorKind::Http, format!("http response read failed: {err}")))?;
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
            .ok_or_else(|| LlmError::new(LlmErrorKind::Http, "response missing assistant text content"))?;
        let usage = LlmTokenUsage {
            prompt_tokens: parsed.usage.as_ref().map(|u| u.prompt_tokens).unwrap_or(0),
            completion_tokens: parsed.usage.as_ref().map(|u| u.completion_tokens).unwrap_or(0),
            total_tokens: parsed.usage.as_ref().map(|u| u.total_tokens).unwrap_or(0),
        };
        Ok((content, usage))
    }

    pub async fn call_json_validated<T, F>(
        &self,
        context: &LlmCallContext,
        request_id: &str,
        system_prompt: &str,
        user_prompt: &str,
        response_validator: Option<&JsonResponseValidator>,
        semantic_validate: F,
    ) -> Result<LlmValidatedJsonResult<T>, LlmError>
    where
        F: Fn(Value) -> Result<T, LlmSemanticValidationError>,
    {
        let logger = match context.media_path.as_deref() {
            Some(path) if !path.trim().is_empty() => {
                TaskLogger::llm_with_media(context.task_id.clone(), path.to_string())
            }
            _ => TaskLogger::llm(context.task_id.clone()),
        };

        let max_attempts = self.config.max_retries.saturating_add(1).max(1);
        let started = std::time::Instant::now();
        let mut last_error = String::new();

        for attempt in 1..=max_attempts {
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
                    "systemPrompt": system_prompt,
                    "userPrompt": user_prompt,
                }
            });

            match self.call_once(system_prompt, user_prompt).await {
                Ok((raw_text, usage)) => {
                    let parsed = match extract_and_repair_json(&raw_text) {
                        Ok(v) => v,
                        Err(err) => {
                            last_error = err.message.clone();
                            let backoff_ms = retry_backoff_ms(attempt, max_attempts);
                            logger.event(
                                "llm.call",
                                Some(&json!({
                                    "status": "invalid_json",
                                    "error": last_error,
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
                            if let Some(delay) = backoff_ms {
                                sleep(Duration::from_millis(delay)).await;
                            }
                            continue;
                        }
                    };

                    if let Some(validator) = response_validator {
                        if let Err(err) = validator.validate(&parsed) {
                            last_error = err.message.clone();
                            let backoff_ms = retry_backoff_ms(attempt, max_attempts);
                            logger.event(
                                "llm.call",
                                Some(&json!({
                                    "status": "invalid_schema",
                                    "error": last_error,
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
                            if let Some(delay) = backoff_ms {
                                sleep(Duration::from_millis(delay)).await;
                            }
                            continue;
                        }
                    }

                    match semantic_validate(parsed) {
                        Ok(value) => {
                            let elapsed_ms = started.elapsed().as_millis();
                            logger.event(
                                "llm.call",
                                Some(&json!({
                                    "status": "ok",
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
                            record_llm_usage_best_effort(
                                &context.task_id,
                                &context.phase,
                                TaskUsage {
                                    prompt_tokens: usage.prompt_tokens,
                                    completion_tokens: usage.completion_tokens,
                                    total_tokens: usage.total_tokens,
                                },
                            );

                            return Ok(LlmValidatedJsonResult {
                                value,
                            });
                        }
                        Err(LlmSemanticValidationError::Retryable(message)) => {
                            last_error = message;
                            let backoff_ms = retry_backoff_ms(attempt, max_attempts);
                            logger.event(
                                "llm.call",
                                Some(&json!({
                                    "status": "invalid_semantic",
                                    "error": last_error,
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
                            if let Some(delay) = backoff_ms {
                                sleep(Duration::from_millis(delay)).await;
                            }
                            continue;
                        }
                    }
                }
                Err(err) => {
                    last_error = format!("{}: {}", err.kind.as_str(), err.message);
                    let backoff_ms = retry_backoff_ms(attempt, max_attempts);
                    logger.event(
                        "llm.call",
                        Some(&json!({
                            "status": "http_error",
                            "error": last_error,
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
                    if let Some(delay) = backoff_ms {
                        sleep(Duration::from_millis(delay)).await;
                    }
                }
            }
        }

        Err(LlmError::new(
            LlmErrorKind::InvalidSemantic,
            format!("llm call failed after {} attempts: {}", max_attempts, last_error),
        ))
    }
}

impl LlmPort for OpenAiCompatLlmClient {
    async fn call_json(
        &self,
        context: &LlmCallContext,
        request_id: &str,
        system_prompt: &str,
        user_prompt: &str,
        response_validator: Option<&JsonResponseValidator>,
    ) -> Result<LlmJsonResult, LlmError> {
        let result = self
            .call_json_validated(
                context,
                request_id,
                system_prompt,
                user_prompt,
                response_validator,
                Ok::<Value, LlmSemanticValidationError>,
            )
            .await?;
        Ok(LlmJsonResult {
            json: result.value,
        })
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
        let maybe_text = part.get("text").and_then(|v| v.as_str()).unwrap_or_default();
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
