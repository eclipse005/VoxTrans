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

/// `content` is either a plain string (text-only, OpenAI compatible) or an
/// array of parts (text + image_url) for vision requests. Using `untagged`
/// keeps the text-only path byte-equal to the pre-vision serialization.
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub(super) enum MessageContent {
    Text(String),
    Parts(Vec<ContentPart>),
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum ContentPart {
    Text { text: String },
    ImageUrl { image_url: ImageUrlSpec },
}

#[derive(Debug, Serialize)]
pub(super) struct ImageUrlSpec {
    url: String,
}

#[derive(Debug, Serialize)]
pub(super) struct ChatMessageRequest {
    pub(super) role: String,
    pub(super) content: MessageContent,
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
    images: Option<&[String]>,
) -> Result<(String, LlmTokenUsage), LlmError> {
    let content = match images {
        Some(imgs) if !imgs.is_empty() => {
            // Images first (StepFun docs: model attends to later prompt more
            // strongly, so text instruction goes last), text second.
            //
            // No `detail` field is sent: it is an OpenAI-private extension that
            // non-OpenAI-compatible providers ignore, and `detail:"low"` would
            // downscale frames to 512×512 on OpenAI endpoints — destroying the
            // text legibility that frame_extract keeps source resolution for.
            // Omitting it lets OpenAI pick `auto` and keeps the request
            // spec-compliant for every other backend.
            let mut parts: Vec<ContentPart> = imgs
                .iter()
                .map(|url| ContentPart::ImageUrl {
                    image_url: ImageUrlSpec { url: url.clone() },
                })
                .collect();
            parts.push(ContentPart::Text {
                text: user_prompt.to_string(),
            });
            MessageContent::Parts(parts)
        }
        _ => MessageContent::Text(user_prompt.to_string()),
    };
    let request = ChatCompletionsRequest {
        model: config.model.clone(),
        messages: vec![ChatMessageRequest {
            role: "user".to_string(),
            content,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_content_text_serializes_as_string() {
        // Pure-text path must serialize as a JSON string, byte-equal to the
        // pre-vision ChatMessageRequest. This is the OpenAI-compatible form.
        let msg = ChatMessageRequest {
            role: "user".to_string(),
            content: MessageContent::Text("hello".to_string()),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert_eq!(json, r#"{"role":"user","content":"hello"}"#);
    }

    #[test]
    fn message_content_parts_serializes_as_array() {
        // Vision path serializes content as an array of typed parts.
        let msg = ChatMessageRequest {
            role: "user".to_string(),
            content: MessageContent::Parts(vec![
                ContentPart::ImageUrl {
                    image_url: ImageUrlSpec {
                        url: "data:image/jpeg;base64,abc".to_string(),
                    },
                },
                ContentPart::Text {
                    text: "describe".to_string(),
                },
            ]),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["role"].as_str().unwrap(), "user");
        let arr = parsed["content"].as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["type"].as_str().unwrap(), "image_url");
        assert_eq!(arr[0]["image_url"]["url"].as_str().unwrap(), "data:image/jpeg;base64,abc");
        // No `detail` field: spec-compliant for non-OpenAI providers; OpenAI
        // defaults to `auto` instead of being forced to a low-res downscale.
        assert!(
            arr[0]["image_url"].get("detail").is_none(),
            "detail field should be absent"
        );
        assert_eq!(arr[1]["type"].as_str().unwrap(), "text");
        assert_eq!(arr[1]["text"].as_str().unwrap(), "describe");
    }
}

