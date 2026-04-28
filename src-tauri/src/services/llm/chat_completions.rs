use serde::{Deserialize, Serialize};

use super::base_url::normalize_base_url;
use super::error::{LlmError, LlmErrorKind};
use super::port::{LlmConfig, LlmTokenUsage};

#[derive(Debug, Serialize)]
pub(super) struct ChatCompletionsRequest {
    pub(super) model: String,
    pub(super) messages: Vec<ChatMessageRequest>,
    pub(super) temperature: f64,
    pub(super) stream: bool,
}

#[derive(Debug, Serialize)]
pub(super) struct ChatMessageRequest {
    pub(super) role: String,
    pub(super) content: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct ChatCompletionsResponse {
    pub(super) choices: Vec<ChatChoice>,
    pub(super) usage: Option<ChatUsage>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ChatChoice {
    pub(super) message: ChatMessageResponse,
}

#[derive(Debug, Deserialize)]
pub(super) struct ChatMessageResponse {
    pub(super) content: serde_json::Value,
}

#[derive(Debug, Deserialize)]
pub(super) struct ChatUsage {
    pub(super) prompt_tokens: u64,
    pub(super) completion_tokens: u64,
    pub(super) total_tokens: u64,
}

pub(super) fn chat_completions_endpoint(base_url: &str) -> String {
    let normalized = normalize_base_url(base_url);
    if normalized.ends_with("/chat/completions") {
        normalized
    } else {
        format!("{normalized}/chat/completions")
    }
}

pub(super) async fn call_chat_completion(
    http: &reqwest::Client,
    config: &LlmConfig,
    user_prompt: &str,
) -> Result<(String, LlmTokenUsage), LlmError> {
    let request = ChatCompletionsRequest {
        model: config.model.clone(),
        messages: vec![ChatMessageRequest {
            role: "user".to_string(),
            content: user_prompt.to_string(),
        }],
        temperature: 0.2,
        stream: false,
    };
    let endpoint = chat_completions_endpoint(&config.base_url);
    let response = http
        .post(&endpoint)
        .bearer_auth(config.api_key.trim())
        .json(&request)
        .send()
        .await
        .map_err(|err| LlmError::new(LlmErrorKind::Http, format!("http request failed: {err}")))?;
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

pub(super) fn extract_text_content(content: &serde_json::Value) -> Option<String> {
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
