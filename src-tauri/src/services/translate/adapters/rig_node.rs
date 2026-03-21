use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tokio::time::sleep;
use rig::client::CompletionClient;
use rig::completion::{AssistantContent, CompletionModel};
use rig::providers::openai;

use crate::services::task_log::TaskLogger;

use super::rig_client::{
    RIG_PROVIDER, RIG_TRANSPORT_CHAT_COMPLETIONS, build_openai_completions_client,
    normalize_base_url,
};
use super::json_repair::extract_and_repair_json;

#[derive(Debug, Clone)]
pub struct RigNodeConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub max_retries: u32,
}

impl RigNodeConfig {
    pub fn new(base_url: String, api_key: String, model: String) -> Self {
        Self {
            base_url,
            api_key,
            model,
            max_retries: 3,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct TokenUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

#[derive(Debug, Clone)]
pub struct JsonResponseValidator {
    pub required_top_level_keys: Vec<String>,
}

impl JsonResponseValidator {
    pub fn with_required_keys(keys: &[&str]) -> Self {
        Self {
            required_top_level_keys: keys.iter().map(|s| (*s).to_string()).collect(),
        }
    }

    fn validate(&self, value: &Value) -> Result<(), String> {
        let obj = value
            .as_object()
            .ok_or_else(|| "schema check failed: root JSON is not object".to_string())?;
        for key in &self.required_top_level_keys {
            if !obj.contains_key(key) {
                return Err(format!("schema check failed: missing key `{key}`"));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct RigNodeJsonTask {
    pub id: usize,
    pub system_prompt: String,
    pub user_prompt: String,
    pub response_validator: Option<JsonResponseValidator>,
}

#[derive(Debug, Clone)]
pub struct RigNodeJsonError {
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct RigNodeJsonResult {
    pub json: Value,
    pub model: String,
}

#[derive(Clone)]
pub struct RigNodeClient {
    config: RigNodeConfig,
    completions_client: openai::CompletionsClient,
}

impl RigNodeClient {
    pub fn new(config: RigNodeConfig) -> Result<Self, String> {
        let client = build_openai_completions_client(&config.api_key, &config.base_url)?;

        Ok(Self {
            config,
            completions_client: client,
        })
    }

    pub async fn call(
        &self,
        task_id: &str,
        media_path: Option<&str>,
        system_prompt: &str,
        user_prompt: &str,
        response_validator: Option<&JsonResponseValidator>,
    ) -> Result<RigNodeJsonResult, RigNodeJsonError> {
        let logger = match media_path {
            Some(path) if !path.trim().is_empty() => {
                TaskLogger::llm_with_media(task_id.to_string(), path.to_string())
            }
            _ => TaskLogger::llm(task_id.to_string()),
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
                "provider": RIG_PROVIDER,
                "transport": RIG_TRANSPORT_CHAT_COMPLETIONS,
                "request": {
                    "systemPrompt": system_prompt,
                    "userPrompt": user_prompt
                }
            });

            match self.call_once(system_prompt, user_prompt).await {
                Ok((raw_text, usage)) => {
                    let parsed = extract_and_repair_json(&raw_text);
                    let parsed = match parsed {
                        Ok(v) => v,
                        Err(err) => {
                            last_error = format!("json parse failed: {err}");
                            let backoff_ms = retry_backoff_ms(attempt, max_attempts);
                            logger.event(
                                "llm.call",
                                Some(&json!({
                                    "status": "invalid_json",
                                    "error": last_error,
                                    "response": {
                                        "text": raw_text
                                    },
                                    "attempt": base_payload["attempt"],
                                    "maxAttempts": base_payload["maxAttempts"],
                                    "backoffMs": backoff_ms,
                                    "model": base_payload["model"],
                                    "baseUrl": base_payload["baseUrl"],
                                    "provider": base_payload["provider"],
                                    "transport": base_payload["transport"],
                                    "request": base_payload["request"]
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
                            last_error = err;
                            let backoff_ms = retry_backoff_ms(attempt, max_attempts);
                            logger.event(
                                "llm.call",
                                Some(&json!({
                                    "status": "invalid_schema",
                                    "error": last_error,
                                    "response": {
                                        "text": raw_text
                                    },
                                    "attempt": base_payload["attempt"],
                                    "maxAttempts": base_payload["maxAttempts"],
                                    "backoffMs": backoff_ms,
                                    "model": base_payload["model"],
                                    "baseUrl": base_payload["baseUrl"],
                                    "provider": base_payload["provider"],
                                    "transport": base_payload["transport"],
                                    "request": base_payload["request"]
                                })),
                            );
                            if let Some(delay) = backoff_ms {
                                sleep(Duration::from_millis(delay)).await;
                            }
                            continue;
                        }
                    }

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
                            "request": base_payload["request"],
                            "response": {
                                "text": raw_text
                            },
                            "elapsedMs": elapsed_ms,
                            "usage": {
                                "promptTokens": usage.prompt_tokens,
                                "completionTokens": usage.completion_tokens,
                                "totalTokens": usage.total_tokens
                            }
                        })),
                    );

                    return Ok(RigNodeJsonResult {
                        json: parsed,
                        model: self.config.model.clone(),
                    });
                }
                Err(err) => {
                    last_error = err;
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
                            "request": base_payload["request"]
                        })),
                    );
                    if let Some(delay) = backoff_ms {
                        sleep(Duration::from_millis(delay)).await;
                    }
                }
            }
        }

        Err(RigNodeJsonError {
            message: format!(
                "llm call failed after {} attempts: {}",
                max_attempts, last_error
            ),
        })
    }

    pub async fn call_batch(
        &self,
        task_id: &str,
        media_path: Option<&str>,
        tasks: Vec<RigNodeJsonTask>,
        concurrency: usize,
    ) -> Vec<(usize, Result<RigNodeJsonResult, RigNodeJsonError>)> {
        if tasks.is_empty() {
            return Vec::new();
        }

        let concurrency = concurrency.clamp(1, 64);
        let semaphore = Arc::new(Semaphore::new(concurrency));
        let mut join_set: JoinSet<(usize, Result<RigNodeJsonResult, RigNodeJsonError>)> =
            JoinSet::new();
        let rig_node = self.clone();
        let task_id = task_id.to_string();
        let media_path = media_path.map(|s| s.to_string());

        for item in tasks {
            let semaphore = Arc::clone(&semaphore);
            let rig_node = rig_node.clone();
            let task_id = task_id.clone();
            let media_path = media_path.clone();
            join_set.spawn(async move {
                let permit = semaphore.acquire_owned().await;
                let _permit = match permit {
                    Ok(v) => v,
                    Err(err) => {
                        return (
                            item.id,
                            Err(RigNodeJsonError {
                                message: format!("semaphore acquire failed: {err}"),
                            }),
                        );
                    }
                };
                let result = rig_node
                    .call(
                        &task_id,
                        media_path.as_deref(),
                        &item.system_prompt,
                        &item.user_prompt,
                        item.response_validator.as_ref(),
                    )
                    .await;
                (item.id, result)
            });
        }

        let mut out: Vec<(usize, Result<RigNodeJsonResult, RigNodeJsonError>)> = Vec::new();
        while let Some(joined) = join_set.join_next().await {
            match joined {
                Ok(v) => out.push(v),
                Err(err) => out.push((
                    usize::MAX,
                    Err(RigNodeJsonError {
                        message: format!("task join error: {err}"),
                    }),
                )),
            }
        }
        out.sort_by_key(|(id, _)| *id);
        out
    }

    async fn call_once(
        &self,
        system_prompt: &str,
        user_prompt: &str,
    ) -> Result<(String, TokenUsage), String> {
        let model = self
            .completions_client
            .completion_model(self.config.model.clone());

        let request = model
            .completion_request(user_prompt.to_string())
            .preamble(system_prompt.to_string())
            .temperature(0.2)
            .build();

        let response = model
            .completion(request)
            .await
            .map_err(|err| format!("rig completion request failed: {err}"))?;

        let content = extract_text_from_choice(&response.choice)
            .ok_or_else(|| "rig response missing assistant text content".to_string())?;

        let usage = TokenUsage {
            prompt_tokens: response.usage.input_tokens,
            completion_tokens: response.usage.output_tokens,
            total_tokens: response.usage.total_tokens,
        };

        Ok((content, usage))
    }
}

fn retry_backoff_ms(attempt: u32, max_attempts: u32) -> Option<u64> {
    if attempt >= max_attempts {
        return None;
    }
    let exp = attempt.saturating_sub(1).min(6);
    let base_ms = 300u64;
    let delay = base_ms.saturating_mul(1u64 << exp);
    Some(delay.min(3_000))
}

fn extract_text_from_choice(choice: &rig::OneOrMany<AssistantContent>) -> Option<String> {
    let mut out = String::new();

    for content in choice.iter() {
        if let AssistantContent::Text(text) = content {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(text.text());
        }
    }

    let trimmed = out.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}
