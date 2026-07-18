use futures_util::StreamExt;
use serde::{Deserialize, Serialize};

use super::base_url::normalize_base_url;
use super::error::{LlmError, LlmErrorKind};
use super::port::{LlmConfig, LlmTokenUsage};

/// Sampling temperature for translation/terminology requests. 0.2 is low
/// enough to keep translations deterministic-ish while still allowing the
/// model a small amount of flexibility for natural phrasing. Extracted
/// from a magic literal so it can be tuned in one place.
const TRANSLATION_TEMPERATURE: f64 = 0.2;

#[derive(Debug, Serialize)]
pub(super) struct ChatCompletionsRequest {
    pub(super) model: String,
    pub(super) messages: Vec<ChatMessageRequest>,
    pub(super) temperature: f64,
    pub(super) stream: bool,
    /// OpenAI-compatible: when streaming, ask the provider to attach final
    /// `usage` on the last SSE chunk. Without this, most providers omit
    /// token counts entirely and we would record 0.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) stream_options: Option<StreamOptions>,
}

#[derive(Debug, Serialize)]
pub(super) struct StreamOptions {
    pub(super) include_usage: bool,
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
        temperature: TRANSLATION_TEMPERATURE,
        stream: false,
        stream_options: None,
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

/// OpenAI-compatible streaming chat completion (`stream: true`, SSE body).
///
/// Invokes `on_delta` with the **accumulated** assistant text after each
/// content chunk (throttling is the caller's job). Falls through with an
/// HTTP error if the provider rejects streaming — callers may then use
/// [`call_chat_completion`].
pub(super) async fn call_chat_completion_stream(
    http: &reqwest::Client,
    config: &LlmConfig,
    user_prompt: &str,
    images: Option<&[String]>,
    mut on_delta: Option<&mut (dyn FnMut(&str) + Send)>,
) -> Result<(String, LlmTokenUsage), LlmError> {
    let content = match images {
        Some(imgs) if !imgs.is_empty() => {
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
        temperature: TRANSLATION_TEMPERATURE,
        stream: true,
        // Critical for token accounting: without this, OpenAI-compatible
        // streams usually never include `usage`, and we would persist 0.
        stream_options: Some(StreamOptions {
            include_usage: true,
        }),
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
    if !status.is_success() {
        let text = response.text().await.unwrap_or_default();
        return Err(LlmError::new(
            LlmErrorKind::Http,
            format!("http status {}: {}", status.as_u16(), text),
        ));
    }

    let mut byte_stream = response.bytes_stream();
    let mut line_buf = String::new();
    let mut acc = String::new();
    let mut usage = LlmTokenUsage::default();

    while let Some(item) = byte_stream.next().await {
        let chunk = item.map_err(|err| {
            LlmError::new(
                LlmErrorKind::Http,
                format!("http stream read failed: {err}"),
            )
        })?;
        line_buf.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(nl) = line_buf.find('\n') {
            // Borrow the completed line, then drop it from the buffer with
            // drain instead of re-allocating the remaining tail each time.
            let line = line_buf[..nl]
                .strip_suffix('\r')
                .unwrap_or(&line_buf[..nl]);
            apply_sse_line(line, &mut acc, &mut usage, &mut on_delta)?;
            let is_done = matches!(parse_sse_data_line(line), Some(SseData::Done));
            line_buf.drain(..nl + 1);
            if is_done {
                return finish_stream(acc, usage, on_delta);
            }
        }
    }

    // Flush a final incomplete line (no trailing newline).
    if !line_buf.trim().is_empty() {
        let line = line_buf.trim_end_matches('\r').to_string();
        apply_sse_line(&line, &mut acc, &mut usage, &mut on_delta)?;
    }

    finish_stream(acc, usage, on_delta)
}

fn apply_sse_line(
    line: &str,
    acc: &mut String,
    usage: &mut LlmTokenUsage,
    on_delta: &mut Option<&mut (dyn FnMut(&str) + Send)>,
) -> Result<(), LlmError> {
    let Some(event) = parse_sse_data_line(line) else {
        return Ok(());
    };
    match event {
        SseData::Done => Ok(()),
        SseData::Json(value) => {
            if let Some(piece) = extract_stream_delta_content(&value) {
                acc.push_str(&piece);
                if let Some(cb) = on_delta.as_mut() {
                    cb(acc);
                }
            }
            // Only adopt non-empty usage so intermediate chunks without
            // usage never wipe a previously captured final usage, and so
            // empty usage objects do not look like "we got usage".
            if let Some(u) = extract_stream_usage(&value) {
                if usage_has_counts(&u) {
                    *usage = u;
                }
            }
            Ok(())
        }
    }
}

fn finish_stream(
    acc: String,
    usage: LlmTokenUsage,
    on_delta: Option<&mut (dyn FnMut(&str) + Send)>,
) -> Result<(String, LlmTokenUsage), LlmError> {
    if let Some(cb) = on_delta {
        cb(&acc);
    }
    if acc.trim().is_empty() {
        return Err(LlmError::new(
            LlmErrorKind::Http,
            "stream ended with empty assistant content",
        ));
    }
    if !usage_has_counts(&usage) {
        // Provider ignored stream_options or never sent usage. Do not
        // silently claim success-with-zero for billing — surface in logs.
        // Caller still gets content; token total is left at 0 (same as a
        // non-stream response that omitted usage).
        eprintln!(
            "[warn] chat completion stream finished without usage; token count for this call will be 0 (provider may ignore stream_options.include_usage)"
        );
    }
    Ok((acc, usage))
}

fn usage_has_counts(usage: &LlmTokenUsage) -> bool {
    usage.total_tokens > 0
        || usage.prompt_tokens > 0
        || usage.completion_tokens > 0
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum SseData {
    Done,
    Json(serde_json::Value),
}

/// Parse one SSE line. Returns `None` for comments / empty / non-data lines.
pub(super) fn parse_sse_data_line(line: &str) -> Option<SseData> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with(':') {
        return None;
    }
    let payload = trimmed.strip_prefix("data:")?.trim();
    if payload.is_empty() {
        return None;
    }
    if payload == "[DONE]" {
        return Some(SseData::Done);
    }
    let value = serde_json::from_str(payload).ok()?;
    Some(SseData::Json(value))
}

pub(super) fn extract_stream_delta_content(chunk: &serde_json::Value) -> Option<String> {
    let choices = chunk.get("choices")?.as_array()?;
    let first = choices.first()?;
    // OpenAI: choices[0].delta.content
    if let Some(delta) = first.get("delta") {
        if let Some(text) = delta.get("content").and_then(|v| v.as_str()) {
            if !text.is_empty() {
                return Some(text.to_string());
            }
        }
        // Rare: delta.content as array of parts
        if let Some(arr) = delta.get("content").and_then(|v| v.as_array()) {
            let mut out = String::new();
            for part in arr {
                if let Some(t) = part.get("text").and_then(|v| v.as_str()) {
                    out.push_str(t);
                } else if let Some(t) = part.as_str() {
                    out.push_str(t);
                }
            }
            if !out.is_empty() {
                return Some(out);
            }
        }
    }
    // Some gateways: choices[0].text
    if let Some(text) = first.get("text").and_then(|v| v.as_str()) {
        if !text.is_empty() {
            return Some(text.to_string());
        }
    }
    None
}

/// Pull usage from an SSE JSON chunk. Returns `None` if the object is
/// missing or all counts are zero/absent (so callers can keep a previous
/// non-zero usage).
pub(super) fn extract_stream_usage(chunk: &serde_json::Value) -> Option<LlmTokenUsage> {
    let usage = chunk.get("usage")?;
    if usage.is_null() {
        return None;
    }
    let prompt_tokens = json_u64(usage.get("prompt_tokens")).unwrap_or(0);
    let completion_tokens = json_u64(usage.get("completion_tokens")).unwrap_or(0);
    let mut total_tokens = json_u64(usage.get("total_tokens")).unwrap_or(0);
    if total_tokens == 0 {
        total_tokens = prompt_tokens.saturating_add(completion_tokens);
    }
    let out = LlmTokenUsage {
        prompt_tokens,
        completion_tokens,
        total_tokens,
    };
    if usage_has_counts(&out) {
        Some(out)
    } else {
        None
    }
}

fn json_u64(v: Option<&serde_json::Value>) -> Option<u64> {
    let v = v?;
    if let Some(n) = v.as_u64() {
        return Some(n);
    }
    if let Some(n) = v.as_i64() {
        return Some(n.max(0) as u64);
    }
    if let Some(n) = v.as_f64() {
        return Some(n.max(0.0) as u64);
    }
    if let Some(s) = v.as_str() {
        return s.trim().parse().ok();
    }
    None
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

    #[test]
    fn parse_sse_data_line_handles_done_and_json() {
        assert_eq!(parse_sse_data_line(""), None);
        assert_eq!(parse_sse_data_line(": keepalive"), None);
        assert_eq!(parse_sse_data_line("data: [DONE]"), Some(SseData::Done));
        let event = parse_sse_data_line(
            r#"data: {"choices":[{"delta":{"content":"你好"}}]}"#,
        )
        .unwrap();
        match event {
            SseData::Json(v) => {
                assert_eq!(
                    extract_stream_delta_content(&v).as_deref(),
                    Some("你好")
                );
            }
            SseData::Done => panic!("expected json"),
        }
    }

    #[test]
    fn extract_stream_delta_content_reads_openai_shape() {
        let v: serde_json::Value = serde_json::from_str(
            r#"{"choices":[{"index":0,"delta":{"role":"assistant","content":"Hel"}}]}"#,
        )
        .unwrap();
        assert_eq!(extract_stream_delta_content(&v).as_deref(), Some("Hel"));
        let empty: serde_json::Value =
            serde_json::from_str(r#"{"choices":[{"delta":{}}]}"#).unwrap();
        assert_eq!(extract_stream_delta_content(&empty), None);
    }

    #[test]
    fn extract_stream_usage_reads_final_chunk_and_ignores_empty() {
        let with_usage: serde_json::Value = serde_json::from_str(
            r#"{"choices":[],"usage":{"prompt_tokens":100,"completion_tokens":50,"total_tokens":150}}"#,
        )
        .unwrap();
        let u = extract_stream_usage(&with_usage).unwrap();
        assert_eq!(u.prompt_tokens, 100);
        assert_eq!(u.completion_tokens, 50);
        assert_eq!(u.total_tokens, 150);

        // total omitted → sum of parts
        let no_total: serde_json::Value = serde_json::from_str(
            r#"{"usage":{"prompt_tokens":10,"completion_tokens":5}}"#,
        )
        .unwrap();
        let u2 = extract_stream_usage(&no_total).unwrap();
        assert_eq!(u2.total_tokens, 15);

        let empty: serde_json::Value =
            serde_json::from_str(r#"{"choices":[{"delta":{"content":"x"}}]}"#).unwrap();
        assert!(extract_stream_usage(&empty).is_none());

        let zero: serde_json::Value =
            serde_json::from_str(r#"{"usage":{"prompt_tokens":0,"completion_tokens":0}}"#)
                .unwrap();
        assert!(extract_stream_usage(&zero).is_none());
    }

    #[test]
    fn stream_request_serializes_include_usage() {
        let req = ChatCompletionsRequest {
            model: "m".into(),
            messages: vec![ChatMessageRequest {
                role: "user".into(),
                content: MessageContent::Text("hi".into()),
            }],
            temperature: 0.2,
            stream: true,
            stream_options: Some(StreamOptions {
                include_usage: true,
            }),
        };
        let v = serde_json::to_value(&req).unwrap();
        assert_eq!(v["stream"], true);
        assert_eq!(v["stream_options"]["include_usage"], true);

        let non_stream = ChatCompletionsRequest {
            model: "m".into(),
            messages: vec![ChatMessageRequest {
                role: "user".into(),
                content: MessageContent::Text("hi".into()),
            }],
            temperature: 0.2,
            stream: false,
            stream_options: None,
        };
        let v2 = serde_json::to_value(&non_stream).unwrap();
        assert!(v2.get("stream_options").is_none());
    }
}

