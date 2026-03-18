use std::time::Duration;

use reqwest::Client;
use serde_json::{Value, json};

use crate::services::task_log::TaskLogger;

use super::json_repair::extract_and_repair_json;

#[derive(Debug, Clone)]
pub struct LlmConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub timeout_sec: u64,
    pub max_retries: u32,
}

impl LlmConfig {
    pub fn new(base_url: String, api_key: String, model: String) -> Self {
        Self {
            base_url,
            api_key,
            model,
            timeout_sec: 60,
            max_retries: 2,
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
pub struct LlmJsonResult {
    pub raw_text: String,
    pub json: Value,
    pub usage: TokenUsage,
    pub model: String,
    pub elapsed_ms: u128,
    pub attempts: u32,
    pub request_id: Option<String>,
}

pub struct LlmClient {
    config: LlmConfig,
    http: Client,
}

impl LlmClient {
    pub fn new(config: LlmConfig) -> Result<Self, String> {
        let http = Client::builder()
            .timeout(Duration::from_secs(config.timeout_sec.clamp(10, 300)))
            .build()
            .map_err(|err| format!("failed to create llm http client: {err}"))?;
        Ok(Self { config, http })
    }

    pub async fn call_json(
        &self,
        task_id: &str,
        media_path: Option<&str>,
        system_prompt: &str,
        user_prompt: &str,
        response_validator: Option<&JsonResponseValidator>,
    ) -> Result<LlmJsonResult, String> {
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
                "baseUrl": self.config.base_url,
                "request": {
                    "systemPrompt": system_prompt,
                    "userPrompt": user_prompt
                }
            });

            match self.call_once(system_prompt, user_prompt).await {
                Ok((raw_text, usage, request_id)) => {
                    let parsed = extract_and_repair_json(&raw_text);
                    let parsed = match parsed {
                        Ok(v) => v,
                        Err(err) => {
                            last_error = format!("json parse failed: {err}");
                            logger.event("llm.call", Some(&json!({
                                "status": "invalid_json",
                                "error": last_error,
                                "response": {
                                    "text": raw_text
                                },
                                "attempt": base_payload["attempt"],
                                "maxAttempts": base_payload["maxAttempts"],
                                "model": base_payload["model"],
                                "baseUrl": base_payload["baseUrl"],
                                "request": base_payload["request"]
                            })));
                            continue;
                        }
                    };

                    if let Some(validator) = response_validator {
                        if let Err(err) = validator.validate(&parsed) {
                            last_error = err;
                            logger.event("llm.call", Some(&json!({
                                "status": "invalid_schema",
                                "error": last_error,
                                "response": {
                                    "text": raw_text
                                },
                                "attempt": base_payload["attempt"],
                                "maxAttempts": base_payload["maxAttempts"],
                                "model": base_payload["model"],
                                "baseUrl": base_payload["baseUrl"],
                                "request": base_payload["request"]
                            })));
                            continue;
                        }
                    }

                    let elapsed_ms = started.elapsed().as_millis();
                    logger.event("llm.call", Some(&json!({
                        "status": "ok",
                        "attempt": base_payload["attempt"],
                        "maxAttempts": base_payload["maxAttempts"],
                        "model": base_payload["model"],
                        "baseUrl": base_payload["baseUrl"],
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
                    })));

                    return Ok(LlmJsonResult {
                        raw_text,
                        json: parsed,
                        usage,
                        model: self.config.model.clone(),
                        elapsed_ms,
                        attempts: attempt,
                        request_id,
                    });
                }
                Err(err) => {
                    last_error = err;
                    logger.event("llm.call", Some(&json!({
                        "status": "http_error",
                        "error": last_error,
                        "attempt": base_payload["attempt"],
                        "maxAttempts": base_payload["maxAttempts"],
                        "model": base_payload["model"],
                        "baseUrl": base_payload["baseUrl"],
                        "request": base_payload["request"]
                    })));
                }
            }
        }

        Err(format!(
            "llm call failed after {} attempts: {}",
            max_attempts, last_error
        ))
    }

    async fn call_once(
        &self,
        system_prompt: &str,
        user_prompt: &str,
    ) -> Result<(String, TokenUsage, Option<String>), String> {
        let url = format!(
            "{}/chat/completions",
            self.config.base_url.trim_end_matches('/')
        );

        let payload = json!({
            "model": self.config.model,
            "temperature": 0.2,
            "response_format": { "type": "json_object" },
            "messages": [
                { "role": "system", "content": system_prompt },
                { "role": "user", "content": user_prompt }
            ]
        });

        let response = self
            .http
            .post(url)
            .bearer_auth(&self.config.api_key)
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await
            .map_err(|err| format!("llm http request failed: {err}"))?;

        let request_id = response
            .headers()
            .get("x-request-id")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|err| format!("llm response read failed: {err}"))?;
        if !status.is_success() {
            return Err(format!("llm returned {}: {}", status, body));
        }

        let value: Value = serde_json::from_str(&body)
            .map_err(|err| format!("llm response json decode failed: {err}"))?;

        let content = value
            .get("choices")
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .and_then(|choice| choice.get("message"))
            .and_then(|msg| msg.get("content"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| "llm response missing choices[0].message.content".to_string())?
            .to_string();

        let usage = parse_usage(&value, system_prompt, user_prompt, &content);
        Ok((content, usage, request_id))
    }
}

fn parse_usage(response: &Value, system_prompt: &str, user_prompt: &str, output: &str) -> TokenUsage {
    let usage = response.get("usage");
    let prompt_tokens = usage
        .and_then(|v| v.get("prompt_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or_else(|| estimate_tokens(system_prompt) + estimate_tokens(user_prompt));
    let completion_tokens = usage
        .and_then(|v| v.get("completion_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or_else(|| estimate_tokens(output));
    let total_tokens = usage
        .and_then(|v| v.get("total_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(prompt_tokens + completion_tokens);

    TokenUsage {
        prompt_tokens,
        completion_tokens,
        total_tokens,
    }
}

fn estimate_tokens(text: &str) -> u64 {
    let chars = text.chars().count() as u64;
    (chars / 4).max(1)
}
