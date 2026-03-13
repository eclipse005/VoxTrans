use crate::prompt_builder::{LLM_CONNECT_TEST_SYSTEM_PROMPT, LLM_CONNECT_TEST_USER_PROMPT};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::SqlitePool;
use tauri::async_runtime::spawn_blocking;

pub mod json;

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

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LlmToolResult {
    pub tool_call_id: String,
    pub content: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
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

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "lowercase")]
pub enum LlmStage {
    Hotword,
    Punctuation,
    Summary,
    Translate,
    Qa,
    Unknown,
}

impl LlmStage {
    fn as_usage_stage(&self) -> &'static str {
        match self {
            Self::Hotword => "hotword",
            Self::Punctuation => "punctuation",
            Self::Summary => "summary",
            Self::Translate => "translate",
            Self::Qa => "qa",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LlmRuntimeContext {
    pub task_id: Option<String>,
    pub media_path: Option<String>,
    pub stage: Option<LlmStage>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum LlmValidationProfile {
    TranslateMap {
        expected_items: usize,
    },
    QaDetect {
        min_index: usize,
        max_index: usize,
    },
    QaPatch {
        max_index: usize,
        expected_work_id: Option<String>,
        expected_index: Option<String>,
        expected_change_modes: Option<Vec<String>>,
        expected_source_before: Option<String>,
        expected_translation_before: Option<String>,
    },
    QaReverify,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LlmCallEnvelope {
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
    pub context: Option<LlmRuntimeContext>,
    pub validation: Option<LlmValidationProfile>,
    #[serde(skip, default)]
    pub usage_pool: Option<SqlitePool>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmInteractResponse {
    pub status: String,
    pub message: Option<String>,
    pub tool_calls: Vec<LlmToolCall>,
    pub finish_reason: Option<String>,
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
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

fn normalize_messages(request: &LlmCallEnvelope) -> Vec<Value> {
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

fn build_payload(request: &LlmCallEnvelope) -> Result<Value, String> {
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

    let prompt_tokens = raw
        .get("usage")
        .and_then(|u| u.get("prompt_tokens"))
        .and_then(|v| v.as_u64());
    let completion_tokens = raw
        .get("usage")
        .and_then(|u| u.get("completion_tokens"))
        .and_then(|v| v.as_u64());
    let total_tokens = raw
        .get("usage")
        .and_then(|u| u.get("total_tokens"))
        .and_then(|v| v.as_u64());

    Ok(LlmInteractResponse {
        status: status.to_string(),
        message,
        tool_calls,
        finish_reason,
        prompt_tokens,
        completion_tokens,
        total_tokens,
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

fn extract_user_prompt(request: &LlmCallEnvelope) -> String {
    if let Some(prompt) = request
        .prompt
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return prompt.to_string();
    }
    if let Some(messages) = &request.messages {
        let user_parts = messages
            .iter()
            .filter(|m| m.role.eq_ignore_ascii_case("user"))
            .filter_map(|m| m.content.as_deref().map(str::trim))
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>();
        if !user_parts.is_empty() {
            return user_parts.join("\n");
        }
    }
    String::new()
}

fn maybe_log_llm(request: &LlmCallEnvelope, payload: Value) {
    let Some(context) = request.context.as_ref() else {
        return;
    };
    let Some(task_id) = context
        .task_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    else {
        return;
    };
    let Some(media_path) = context
        .media_path
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    else {
        return;
    };

    let message = format!(
        "llm.exchange\n{}",
        serde_json::to_string_pretty(&payload).unwrap_or_else(|_| "{}".to_string())
    );
    let _ = crate::services::logs::append_task_log(crate::services::logs::AppendTaskLogRequest {
        task_id: task_id.to_string(),
        media_path: media_path.to_string(),
        channel: "llm".to_string(),
        message,
    });
}

fn run_llm_call_blocking(request: LlmCallEnvelope) -> Result<LlmInteractResponse, String> {
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

    let system_prompt = request.system_prompt.clone().unwrap_or_default();
    let user_prompt = extract_user_prompt(&request);
    let stage = request
        .context
        .as_ref()
        .and_then(|ctx| ctx.stage.as_ref())
        .map(LlmStage::as_usage_stage)
        .unwrap_or("unknown");
    let request_log = json!({
        "stage": stage,
        "systemPromptLength": system_prompt.chars().count(),
        "systemPrompt": system_prompt,
        "userPromptLength": user_prompt.chars().count(),
        "userPrompt": user_prompt,
    });

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
                    let raw: Value = serde_json::from_str(&body_text).map_err(|e| {
                        format!("invalid llm response json: {e}; body: {body_text}")
                    })?;
                    let parsed = parse_llm_response(&raw)?;
                    if let Err(validation_error) = validate_response(&request, &parsed) {
                        let error = format!("llm response validation failed: {validation_error}");
                        if attempt > max_retries {
                            break error;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(retry_backoff_ms(
                            attempt,
                        )));
                        continue;
                    }
                    maybe_log_llm(
                        &request,
                        json!({
                            "request": request_log.clone(),
                            "response": {
                                "messageLength": parsed.message.as_deref().unwrap_or_default().chars().count(),
                                "message": parsed.message.clone().unwrap_or_default(),
                            }
                        }),
                    );
                    return Ok(parsed);
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
        let err = format!(
            "llm request exhausted after {attempt}/{total_attempts} attempts: {}",
            final_error
        );
        maybe_log_llm(
            &request,
            json!({
                "request": request_log.clone(),
                "response": {
                    "messageLength": err.chars().count(),
                    "message": err,
                }
            }),
        );
        return Err(err);
    }
    maybe_log_llm(
        &request,
        json!({
            "request": request_log,
            "response": {
                "messageLength": final_error.chars().count(),
                "message": final_error,
            }
        }),
    );
    Err(final_error)
}

pub async fn call(request: LlmCallEnvelope) -> Result<LlmInteractResponse, String> {
    let request_for_blocking = request.clone();
    let response = spawn_blocking(move || run_llm_call_blocking(request_for_blocking))
        .await
        .map_err(|e| e.to_string())??;

    maybe_record_llm_usage(&request, &response).await;
    Ok(response)
}

pub async fn llm_test_connection(
    request: LlmTestConnectionRequest,
) -> Result<LlmTestConnectionResponse, String> {
    let call_envelope = LlmCallEnvelope {
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
        context: None,
        validation: None,
        usage_pool: None,
    };

    let response = spawn_blocking(move || run_llm_call_blocking(call_envelope))
        .await
        .map_err(|e| e.to_string())??;

    Ok(LlmTestConnectionResponse {
        ok: true,
        message: response.message.unwrap_or_else(|| "OK".to_string()),
        finish_reason: response.finish_reason,
        model: request.model,
    })
}

async fn maybe_record_llm_usage(request: &LlmCallEnvelope, response: &LlmInteractResponse) {
    let Some(pool) = request.usage_pool.as_ref() else {
        return;
    };
    let Some(context) = request.context.as_ref() else {
        return;
    };
    let Some(task_id) = context
        .task_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    else {
        return;
    };
    let stage = context
        .stage
        .as_ref()
        .map(LlmStage::as_usage_stage)
        .unwrap_or("unknown");

    let prompt_tokens = response.prompt_tokens.unwrap_or(0) as i64;
    let completion_tokens = response.completion_tokens.unwrap_or(0) as i64;
    let total_tokens = response
        .total_tokens
        .unwrap_or((prompt_tokens + completion_tokens).max(0) as u64) as i64;
    if prompt_tokens <= 0 && completion_tokens <= 0 && total_tokens <= 0 {
        return;
    }

    let _ = crate::services::usage::record_task_llm_usage(
        pool,
        crate::services::usage::RecordTaskLlmUsageRequest {
            task_id: task_id.to_string(),
            stage: stage.to_string(),
            prompt_tokens,
            completion_tokens,
            total_tokens,
        },
    )
    .await;
}

fn validate_response(
    request: &LlmCallEnvelope,
    response: &LlmInteractResponse,
) -> Result<(), String> {
    let Some(profile) = request.validation.as_ref() else {
        return Ok(());
    };
    match profile {
        LlmValidationProfile::TranslateMap { expected_items } => validate_translate_map(
            response.message.as_deref().unwrap_or_default(),
            *expected_items,
        ),
        LlmValidationProfile::QaDetect {
            min_index,
            max_index,
        } => validate_qa_detect(
            response.message.as_deref().unwrap_or_default(),
            *min_index,
            *max_index,
        ),
        LlmValidationProfile::QaPatch {
            max_index,
            expected_work_id,
            expected_index,
            expected_change_modes,
            expected_source_before,
            expected_translation_before,
        } => validate_qa_patch(
            response.message.as_deref().unwrap_or_default(),
            *max_index,
            expected_work_id.as_deref(),
            expected_index.as_deref(),
            expected_change_modes.as_deref(),
            expected_source_before.as_deref(),
            expected_translation_before.as_deref(),
        ),
        LlmValidationProfile::QaReverify => {
            validate_qa_reverify(response.message.as_deref().unwrap_or_default())
        }
    }
}

fn validate_translate_map(message: &str, expected_items: usize) -> Result<(), String> {
    if expected_items == 0 {
        return Ok(());
    }
    let value = crate::services::llm::json::parse_llm_json_response(message)?;
    let obj = value
        .as_object()
        .ok_or_else(|| "translate response must be a JSON object".to_string())?;
    for idx in 1..=expected_items {
        let key = idx.to_string();
        let item = obj.get(&key).ok_or_else(|| format!("missing key: {key}"))?;
        let _item_obj = item
            .as_object()
            .ok_or_else(|| format!("item {key} must be an object"))?;
    }
    Ok(())
}

fn validate_qa_detect(message: &str, min_index: usize, max_index: usize) -> Result<(), String> {
    let value = crate::services::llm::json::parse_llm_json_response(message)?;
    let obj = value
        .as_object()
        .ok_or_else(|| "qa detect response must be a JSON object".to_string())?;
    let issues = obj
        .get("issues")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "qa detect response must contain issues[]".to_string())?;

    for (i, issue) in issues.iter().enumerate() {
        let issue_obj = issue
            .as_object()
            .ok_or_else(|| format!("issues[{i}] must be an object"))?;
        let index = issue_obj
            .get("index")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .ok_or_else(|| format!("issues[{i}].index missing"))?;
        let idx = index
            .parse::<usize>()
            .map_err(|_| format!("issues[{i}].index must be numeric string"))?;
        let allowed_min = min_index.max(1);
        let allowed_max = max_index.max(allowed_min);
        if idx < allowed_min || idx > allowed_max {
            return Err(format!(
                "issues[{i}].index out of range: {idx}, expected {}..={}",
                allowed_min, allowed_max
            ));
        }
        let _severity = issue_obj
            .get("severity")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .ok_or_else(|| format!("issues[{i}].severity missing"))?;
        let _message = issue_obj
            .get("message")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .ok_or_else(|| format!("issues[{i}].message missing"))?;
        let _issue_type = issue_obj
            .get("issueType")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .ok_or_else(|| format!("issues[{i}].issueType missing"))?;
        let _target_field = issue_obj
            .get("targetField")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .ok_or_else(|| format!("issues[{i}].targetField missing"))?;
        let _evidence = issue_obj
            .get("evidence")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .ok_or_else(|| format!("issues[{i}].evidence missing"))?;
        if issue_obj.get("fixable").and_then(|v| v.as_bool()).is_none() {
            return Err(format!("issues[{i}].fixable missing or not boolean"));
        }
        let confidence = issue_obj
            .get("confidence")
            .and_then(|v| v.as_f64())
            .ok_or_else(|| format!("issues[{i}].confidence missing or not number"))?;
        if !(0.0..=1.0).contains(&confidence) {
            return Err(format!("issues[{i}].confidence out of range"));
        }
    }

    Ok(())
}

fn validate_qa_patch(
    message: &str,
    max_index: usize,
    expected_work_id: Option<&str>,
    expected_index: Option<&str>,
    expected_change_modes: Option<&[String]>,
    expected_source_before: Option<&str>,
    expected_translation_before: Option<&str>,
) -> Result<(), String> {
    let value = crate::services::llm::json::parse_llm_json_response(message)?;
    let obj = value
        .as_object()
        .ok_or_else(|| "qa patch response must be a JSON object".to_string())?;
    let action = obj
        .get("action")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .ok_or_else(|| "qa patch response must contain action".to_string())?;
    if action != "apply" && action != "skip" {
        return Err("qa patch action must be apply or skip".to_string());
    }
    let patch_value = obj.get("patch");
    if action == "skip" {
        if let Some(value) = patch_value {
            if !value.is_null() {
                return Err("qa patch must be null when action=skip".to_string());
            }
        }
        return Ok(());
    }
    let patch = patch_value.and_then(|v| v.as_object()).ok_or_else(|| {
        "qa patch response must contain patch object when action=apply".to_string()
    })?;
    let _work_id = patch
        .get("workId")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| "patch.workId missing".to_string())?;
    if let Some(expected) = expected_work_id {
        if !expected.trim().is_empty() && _work_id != expected.trim() {
            return Err(format!(
                "patch.workId mismatch: got {}, expected {}",
                _work_id,
                expected.trim()
            ));
        }
    }
    let index = patch
        .get("index")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| "patch.index missing".to_string())?;
    let idx = index
        .parse::<usize>()
        .map_err(|_| "patch.index must be numeric string".to_string())?;
    if idx == 0 || idx > max_index.max(1) {
        return Err(format!(
            "patch.index out of range: {idx}, expected 1..={}",
            max_index.max(1)
        ));
    }
    if let Some(expected) = expected_index {
        let expected_trimmed = expected.trim();
        if !expected_trimmed.is_empty() {
            let expected_idx = expected_trimmed
                .parse::<usize>()
                .map_err(|_| "expected_index must be numeric string".to_string())?;
            if idx != expected_idx {
                return Err(format!(
                    "patch.index mismatch: got {idx}, expected {expected_idx}"
                ));
            }
        }
    }
    let _source_before = patch
        .get("sourceBefore")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "patch.sourceBefore missing".to_string())?;
    if let Some(expected) = expected_source_before {
        if _source_before.trim() != expected.trim() {
            return Err("patch.sourceBefore mismatch".to_string());
        }
    }
    let _source_after = patch
        .get("sourceAfter")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "patch.sourceAfter missing".to_string())?;
    let _translation_before = patch
        .get("translationBefore")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "patch.translationBefore missing".to_string())?;
    if let Some(expected) = expected_translation_before {
        if _translation_before.trim() != expected.trim() {
            return Err("patch.translationBefore mismatch".to_string());
        }
    }
    let _translation_after = patch
        .get("translationAfter")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "patch.translationAfter missing".to_string())?;
    let change_mode = patch
        .get("changeMode")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .ok_or_else(|| "patch.changeMode missing".to_string())?;
    if !matches!(
        change_mode,
        "translation_only" | "source_only" | "source_and_translation"
    ) {
        return Err("patch.changeMode invalid".to_string());
    }
    if let Some(allowed_modes) = expected_change_modes {
        let allowed = allowed_modes
            .iter()
            .map(|mode| mode.trim())
            .filter(|mode| !mode.is_empty())
            .collect::<Vec<_>>();
        if !allowed.is_empty() && !allowed.iter().any(|mode| *mode == change_mode) {
            return Err(format!(
                "patch.changeMode not allowed: {change_mode}, allowed={}",
                allowed.join(",")
            ));
        }
    }
    let _reason = patch
        .get("reason")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| "patch.reason missing".to_string())?;
    let confidence = patch
        .get("confidence")
        .and_then(|v| v.as_f64())
        .ok_or_else(|| "patch.confidence missing or not number".to_string())?;
    if !(0.0..=1.0).contains(&confidence) {
        return Err("patch.confidence out of range".to_string());
    }
    Ok(())
}

fn validate_qa_reverify(message: &str) -> Result<(), String> {
    let value = crate::services::llm::json::parse_llm_json_response(message)?;
    let obj = value
        .as_object()
        .ok_or_else(|| "qa reverify response must be a JSON object".to_string())?;
    let _work_id = obj
        .get("workId")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| "qa reverify workId missing".to_string())?;
    let status = obj
        .get("status")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .ok_or_else(|| "qa reverify status missing".to_string())?;
    if !matches!(status, "resolved" | "unresolved" | "regression") {
        return Err("qa reverify status invalid".to_string());
    }
    let _message = obj
        .get("message")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| "qa reverify message missing".to_string())?;
    let confidence = obj
        .get("confidence")
        .and_then(|v| v.as_f64())
        .ok_or_else(|| "qa reverify confidence missing or not number".to_string())?;
    if !(0.0..=1.0).contains(&confidence) {
        return Err("qa reverify confidence out of range".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_apply_patch_message(change_mode: &str) -> String {
        json!({
            "action": "apply",
            "patch": {
                "workId": "work-12",
                "index": "12",
                "sourceBefore": "hello world",
                "sourceAfter": "hello world",
                "translationBefore": "你好，世界",
                "translationAfter": "你好，世界！",
                "changeMode": change_mode,
                "reason": "improve fluency",
                "confidence": 0.9
            }
        })
        .to_string()
    }

    #[test]
    fn qa_patch_validation_enforces_expected_scope() {
        let message = sample_apply_patch_message("translation_only");
        let ok = validate_qa_patch(
            &message,
            120,
            Some("work-12"),
            Some("12"),
            Some(&["translation_only".to_string()]),
            Some("hello world"),
            Some("你好，世界"),
        );
        assert!(ok.is_ok());

        let err = validate_qa_patch(
            &message,
            120,
            Some("work-13"),
            Some("12"),
            Some(&["translation_only".to_string()]),
            Some("hello world"),
            Some("你好，世界"),
        )
        .unwrap_err();
        assert!(err.contains("workId mismatch"));
    }

    #[test]
    fn qa_patch_validation_enforces_allowed_modes() {
        let message = sample_apply_patch_message("source_and_translation");
        let err = validate_qa_patch(
            &message,
            120,
            Some("work-12"),
            Some("12"),
            Some(&["translation_only".to_string()]),
            Some("hello world"),
            Some("你好，世界"),
        )
        .unwrap_err();
        assert!(err.contains("changeMode not allowed"));
    }
}
