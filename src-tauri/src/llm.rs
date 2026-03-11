use crate::prompts::{LLM_CONNECT_TEST_SYSTEM_PROMPT, LLM_CONNECT_TEST_USER_PROMPT};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tauri::async_runtime::spawn_blocking;

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LlmTool {
    pub r#type: String,
    pub function: LlmToolFunction,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LlmToolFunction {
    pub name: String,
    pub description: Option<String>,
    pub parameters: Value,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LlmToolResult {
    pub tool_call_id: String,
    pub content: String,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LlmMessageInput {
    pub role: String,
    pub content: Option<String>,
    pub tool_call_id: Option<String>,
    pub tool_calls: Option<Vec<LlmToolCall>>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LlmToolCall {
    pub id: String,
    pub r#type: String,
    pub function: LlmToolCallFunction,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LlmToolCallFunction {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LlmInteractRequest {
    pub api_key: String,
    pub model: String,
    pub base_url: Option<String>,
    pub system_prompt: Option<String>,
    pub prompt: Option<String>,
    pub messages: Option<Vec<LlmMessageInput>>,
    pub mode: Option<String>,
    pub tools: Option<Vec<LlmTool>>,
    pub tool_results: Option<Vec<LlmToolResult>>,
    pub tool_choice: Option<Value>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub timeout_secs: Option<u64>,
    pub max_retries: Option<u32>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmInteractResponse {
    pub status: String,
    pub message: Option<String>,
    pub tool_calls: Vec<LlmToolCall>,
    pub finish_reason: Option<String>,
    pub raw: Value,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LlmTestConnectionRequest {
    pub api_key: String,
    pub base_url: Option<String>,
    pub model: String,
    pub timeout_secs: Option<u64>,
    pub max_retries: Option<u32>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmTestConnectionResponse {
    pub ok: bool,
    pub message: String,
    pub finish_reason: Option<String>,
    pub model: String,
}

const DEFAULT_MAX_RETRIES: u32 = 3;
const MAX_ALLOWED_RETRIES: u32 = 8;
const BASE_RETRY_BACKOFF_MS: u64 = 500;
const MAX_RETRY_BACKOFF_MS: u64 = 4000;

fn normalize_base_url(base_url: Option<String>) -> String {
    let fallback = "https://api.openai.com/v1".to_string();
    let Some(url) = base_url else {
        return fallback;
    };
    let trimmed = url.trim().trim_end_matches('/').to_string();
    if trimmed.is_empty() {
        return fallback;
    }
    trimmed
}

fn completion_endpoint(base_url: Option<String>) -> String {
    format!("{}/chat/completions", normalize_base_url(base_url))
}

fn normalize_messages(request: &LlmInteractRequest) -> Vec<Value> {
    let mut out = Vec::new();
    if let Some(system_prompt) = &request.system_prompt {
        let system = system_prompt.trim();
        if !system.is_empty() {
            out.push(json!({ "role": "system", "content": system }));
        }
    }

    if let Some(messages) = &request.messages {
        out.extend(messages.iter().map(|m| {
            let mut obj = json!({
                "role": m.role,
                "content": m.content.clone().unwrap_or_default(),
            });
            if let Some(tool_call_id) = &m.tool_call_id {
                obj["tool_call_id"] = json!(tool_call_id);
            }
            if let Some(tool_calls) = &m.tool_calls {
                obj["tool_calls"] = serde_json::to_value(tool_calls).unwrap_or_else(|_| json!([]));
            }
            obj
        }));
    } else if let Some(prompt) = &request.prompt {
        out.push(json!({ "role": "user", "content": prompt }));
    }

    if let Some(tool_results) = &request.tool_results {
        for result in tool_results {
            out.push(json!({
                "role": "tool",
                "tool_call_id": result.tool_call_id,
                "content": result.content,
            }));
        }
    }

    out
}

fn build_payload(request: &LlmInteractRequest) -> Result<Value, String> {
    let messages = normalize_messages(request);
    if messages.is_empty() {
        return Err("llm_interact requires prompt or messages".to_string());
    }

    let mut payload = json!({
        "model": request.model,
        "messages": messages,
    });

    if let Some(temperature) = request.temperature {
        payload["temperature"] = json!(temperature);
    }
    if let Some(max_tokens) = request.max_tokens {
        payload["max_tokens"] = json!(max_tokens);
    }

    let mode = request
        .mode
        .as_deref()
        .unwrap_or("chat")
        .to_ascii_lowercase();
    let tools = request.tools.clone().unwrap_or_default();
    if mode == "tool" || !tools.is_empty() {
        payload["tools"] = serde_json::to_value(tools).map_err(|e| e.to_string())?;
        if let Some(tool_choice) = &request.tool_choice {
            payload["tool_choice"] = tool_choice.clone();
        } else {
            payload["tool_choice"] = json!("auto");
        }
    }

    Ok(payload)
}

fn parse_llm_response(raw: &Value) -> Result<LlmInteractResponse, String> {
    let choice = raw
        .get("choices")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .ok_or_else(|| "invalid llm response: choices[0] missing".to_string())?;

    let finish_reason = choice
        .get("finish_reason")
        .and_then(|v| v.as_str())
        .map(str::to_string);

    let message_obj = choice
        .get("message")
        .and_then(|v| v.as_object())
        .ok_or_else(|| "invalid llm response: choices[0].message missing".to_string())?;

    let message = message_obj
        .get("content")
        .and_then(|v| v.as_str())
        .map(str::to_string);

    let tool_calls: Vec<LlmToolCall> = message_obj
        .get("tool_calls")
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map_err(|e| format!("invalid tool_calls in llm response: {e}"))?
        .unwrap_or_default();

    let status = if !tool_calls.is_empty() {
        "requires_tool"
    } else {
        "completed"
    };

    Ok(LlmInteractResponse {
        status: status.to_string(),
        message,
        tool_calls,
        finish_reason,
        raw: raw.clone(),
    })
}

fn should_retry_status(status: StatusCode) -> bool {
    matches!(
        status,
        StatusCode::TOO_MANY_REQUESTS
            | StatusCode::BAD_GATEWAY
            | StatusCode::SERVICE_UNAVAILABLE
            | StatusCode::GATEWAY_TIMEOUT
    )
}

fn should_retry_transport_error(err: &reqwest::Error) -> bool {
    err.is_timeout() || err.is_connect() || err.is_request() || err.is_body()
}

fn retry_backoff_ms(attempt: u32) -> u64 {
    let shift = attempt.saturating_sub(1).min(4);
    (BASE_RETRY_BACKOFF_MS << shift).min(MAX_RETRY_BACKOFF_MS)
}

fn run_llm_interact_blocking(request: LlmInteractRequest) -> Result<LlmInteractResponse, String> {
    if request.api_key.trim().is_empty() {
        return Err("apiKey is required".to_string());
    }
    if request.model.trim().is_empty() {
        return Err("model is required".to_string());
    }

    let url = completion_endpoint(request.base_url.clone());
    let payload = build_payload(&request)?;
    let timeout_secs = request.timeout_secs.unwrap_or(300).max(5);
    let max_retries = request
        .max_retries
        .unwrap_or(DEFAULT_MAX_RETRIES)
        .min(MAX_ALLOWED_RETRIES);
    let total_attempts = max_retries + 1;

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .build()
        .map_err(|e| format!("failed to build llm client: {e}"))?;

    let mut attempt: u32 = 0;
    let final_error = loop {
        attempt += 1;
        let send_result = client
            .post(&url)
            .bearer_auth(request.api_key.trim())
            .header("Content-Type", "application/json")
            .json(&payload)
            .send();

        match send_result {
            Ok(response) => {
                let status_code = response.status();
                let body_text = response
                    .text()
                    .map_err(|e| format!("failed to read llm response body: {e}"))?;

                if status_code.is_success() {
                    let raw: Value = serde_json::from_str(&body_text)
                        .map_err(|e| format!("invalid llm response json: {e}; body: {body_text}"))?;
                    return parse_llm_response(&raw);
                }

                let error = match serde_json::from_str::<Value>(&body_text) {
                    Ok(raw) => format!("llm api error ({status_code}) @ {url}: {raw}"),
                    Err(_) => format!("llm api error ({status_code}) @ {url}: {body_text}"),
                };

                if attempt > max_retries || !should_retry_status(status_code) {
                    break error;
                }
            }
            Err(err) => {
                let error = format!("llm request failed: {err}");
                if attempt > max_retries || !should_retry_transport_error(&err) {
                    break error;
                }
            }
        }

        std::thread::sleep(std::time::Duration::from_millis(retry_backoff_ms(attempt)));
    };

    if attempt > 1 {
        return Err(format!(
            "llm request exhausted after {attempt}/{total_attempts} attempts: {}",
            final_error
        ));
    }
    Err(final_error)
}

#[tauri::command]
pub async fn llm_interact(request: LlmInteractRequest) -> Result<LlmInteractResponse, String> {
    spawn_blocking(move || run_llm_interact_blocking(request))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn llm_test_connection(
    request: LlmTestConnectionRequest,
) -> Result<LlmTestConnectionResponse, String> {
    let interact_request = LlmInteractRequest {
        api_key: request.api_key,
        model: request.model.clone(),
        base_url: request.base_url,
        system_prompt: Some(LLM_CONNECT_TEST_SYSTEM_PROMPT.to_string()),
        prompt: Some(LLM_CONNECT_TEST_USER_PROMPT.to_string()),
        messages: None,
        mode: Some("chat".to_string()),
        tools: None,
        tool_results: None,
        tool_choice: None,
        temperature: None,
        max_tokens: None,
        timeout_secs: request.timeout_secs.or(Some(30)),
        max_retries: request.max_retries.or(Some(1)),
    };

    let response = spawn_blocking(move || run_llm_interact_blocking(interact_request))
        .await
        .map_err(|e| e.to_string())??;

    Ok(LlmTestConnectionResponse {
        ok: true,
        message: response.message.unwrap_or_else(|| "OK".to_string()),
        finish_reason: response.finish_reason,
        model: request.model,
    })
}



