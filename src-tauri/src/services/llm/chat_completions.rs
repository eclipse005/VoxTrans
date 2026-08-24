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
        temperature: TRANSLATION_TEMPERATURE,
        stream: false,
        stream_options: None,
    };
    let endpoint = chat_completions_endpoint(&config.base_url);
    let response = http
        .post(&endpoint)
        .bearer_auth(config.next_api_key())
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
    mut on_delta: Option<&mut (dyn FnMut(&str) + Send)>,
) -> Result<(String, LlmTokenUsage), LlmError> {
    let request = ChatCompletionsRequest {
        model: config.model.clone(),
        messages: vec![ChatMessageRequest {
            role: "user".to_string(),
            content: user_prompt.to_string(),
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
        .bearer_auth(config.next_api_key())
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

// ── Tool-calling (terminology agent). Translation still uses the string-only
// request above. Kept separate so providers that reject `tools` do not affect
// the batched translation path.

const AGENT_TEMPERATURE: f64 = 0.3;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".to_string(),
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: None,
            reasoning_content: None,
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: None,
            reasoning_content: None,
        }
    }

    pub fn assistant_text(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".to_string(),
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: None,
            reasoning_content: None,
        }
    }

    pub fn assistant_tools(
        content: Option<String>,
        tool_calls: Vec<ToolCall>,
        reasoning_content: Option<String>,
    ) -> Self {
        Self {
            role: "assistant".to_string(),
            content: content.filter(|s| !s.trim().is_empty()),
            tool_calls: Some(tool_calls),
            tool_call_id: None,
            reasoning_content,
        }
    }

    pub fn tool(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: "tool".to_string(),
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: Some(tool_call_id.into()),
            reasoning_content: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub type_: String,
    pub function: ToolCallFunction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallFunction {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone)]
pub struct AssistantTurn {
    pub content: String,
    pub tool_calls: Vec<ToolCall>,
    pub reasoning_content: Option<String>,
    pub usage: LlmTokenUsage,
}

#[derive(Debug, Serialize)]
struct ChatCompletionsToolsRequest {
    model: String,
    messages: Vec<ChatMessage>,
    temperature: f64,
    stream: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<String>,
}

pub(super) async fn call_chat_completion_tools(
    http: &reqwest::Client,
    config: &LlmConfig,
    messages: &[ChatMessage],
    tools: &[serde_json::Value],
    temperature: Option<f64>,
) -> Result<AssistantTurn, LlmError> {
    let request = ChatCompletionsToolsRequest {
        model: config.model.clone(),
        messages: messages.to_vec(),
        temperature: temperature.unwrap_or(AGENT_TEMPERATURE),
        stream: false,
        tools: tools.to_vec(),
        tool_choice: if tools.is_empty() {
            None
        } else {
            Some("auto".to_string())
        },
    };
    let endpoint = chat_completions_endpoint(&config.base_url);
    let response = http
        .post(&endpoint)
        .bearer_auth(config.next_api_key())
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
    let parsed: serde_json::Value = serde_json::from_str(&text).map_err(|err| {
        LlmError::new(
            LlmErrorKind::Http,
            format!("chat completion decode failed: {err}; raw={text}"),
        )
    })?;
    parse_assistant_turn(&parsed)
}

pub(super) fn parse_assistant_turn(parsed: &serde_json::Value) -> Result<AssistantTurn, LlmError> {
    let choice = parsed
        .get("choices")
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.first())
        .ok_or_else(|| {
            LlmError::new(LlmErrorKind::Http, "response missing choices[0]")
        })?;
    let message = choice.get("message").unwrap_or(choice);
    let content = message
        .get("content")
        .and_then(extract_text_content)
        .unwrap_or_default();
    let reasoning = message
        .get("reasoning_content")
        .and_then(|v| v.as_str())
        .or_else(|| message.get("reasoning").and_then(|v| v.as_str()))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let mut tool_calls = parse_tool_calls(message.get("tool_calls"));
    if tool_calls.is_empty() {
        if let Some(legacy) = message.get("function_call") {
            if let Some(tc) = tool_call_from_function(legacy, "call_legacy") {
                tool_calls.push(tc);
            }
        }
    }
    if content.trim().is_empty() && tool_calls.is_empty() {
        return Err(LlmError::new(
            LlmErrorKind::Http,
            "response missing assistant text and tool_calls",
        ));
    }
    let usage = parsed.get("usage").map(usage_from_value).unwrap_or_default();
    Ok(AssistantTurn {
        content,
        tool_calls,
        reasoning_content: reasoning,
        usage,
    })
}

fn parse_tool_calls(value: Option<&serde_json::Value>) -> Vec<ToolCall> {
    let Some(arr) = value.and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (i, item) in arr.iter().enumerate() {
        let id = item
            .get("id")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| format!("call_{i}"));
        let fn_obj = item.get("function").unwrap_or(item);
        if let Some(tc) = tool_call_from_function(fn_obj, &id) {
            out.push(tc);
        }
    }
    out
}

fn tool_call_from_function(fn_obj: &serde_json::Value, id: &str) -> Option<ToolCall> {
    let name = fn_obj.get("name").and_then(|v| v.as_str())?.trim();
    if name.is_empty() {
        return None;
    }
    let arguments = match fn_obj.get("arguments") {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
        None => "{}".to_string(),
    };
    Some(ToolCall {
        id: id.to_string(),
        type_: "function".to_string(),
        function: ToolCallFunction {
            name: name.to_string(),
            arguments,
        },
    })
}

fn usage_from_value(usage: &serde_json::Value) -> LlmTokenUsage {
    let prompt_tokens = json_u64(usage.get("prompt_tokens")).unwrap_or(0);
    let completion_tokens = json_u64(usage.get("completion_tokens")).unwrap_or(0);
    let mut total_tokens = json_u64(usage.get("total_tokens")).unwrap_or(0);
    if total_tokens == 0 {
        total_tokens = prompt_tokens.saturating_add(completion_tokens);
    }
    LlmTokenUsage {
        prompt_tokens,
        completion_tokens,
        total_tokens,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_content_text_serializes_as_string() {
        let msg = ChatMessageRequest {
            role: "user".to_string(),
            content: "hello".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert_eq!(json, r#"{"role":"user","content":"hello"}"#);
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
                content: "hi".into(),
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
                content: "hi".into(),
            }],
            temperature: 0.2,
            stream: false,
            stream_options: None,
        };
        let v2 = serde_json::to_value(&non_stream).unwrap();
        assert!(v2.get("stream_options").is_none());
    }

    #[test]
    fn assistant_tools_omits_empty_content() {
        let msg = ChatMessage::assistant_tools(
            None,
            vec![ToolCall {
                id: "c1".into(),
                type_: "function".into(),
                function: ToolCallFunction {
                    name: "search_transcript".into(),
                    arguments: "{}".into(),
                },
            }],
            None,
        );
        let v = serde_json::to_value(&msg).unwrap();
        assert!(v.get("content").is_none());
        assert_eq!(v["role"], "assistant");
        assert_eq!(v["tool_calls"][0]["id"], "c1");

        let blank = ChatMessage::assistant_tools(Some("  ".into()), vec![], None);
        let v2 = serde_json::to_value(&blank).unwrap();
        assert!(v2.get("content").is_none());
    }

    #[test]
    fn parse_assistant_turn_reads_tool_calls_and_string_arguments() {
        let v: serde_json::Value = serde_json::from_str(
            r#"{
              "choices":[{
                "message":{
                  "role":"assistant",
                  "content":null,
                  "tool_calls":[{
                    "id":"call_1",
                    "type":"function",
                    "function":{"name":"search_transcript","arguments":"{\"pattern\":\"foo\"}"}
                  }]
                }
              }],
              "usage":{"prompt_tokens":10,"completion_tokens":4,"total_tokens":14}
            }"#,
        )
        .unwrap();
        let turn = parse_assistant_turn(&v).unwrap();
        assert_eq!(turn.tool_calls.len(), 1);
        assert_eq!(turn.tool_calls[0].function.name, "search_transcript");
        assert!(turn.tool_calls[0].function.arguments.contains("foo"));
        assert_eq!(turn.usage.total_tokens, 14);
    }

    #[test]
    fn parse_assistant_turn_accepts_object_arguments_and_legacy_function_call() {
        let obj_args: serde_json::Value = serde_json::from_str(
            r#"{"choices":[{"message":{"tool_calls":[{"function":{"name":"count_transcript","arguments":{"terms":["a"]}}}]}}]}"#,
        )
        .unwrap();
        let turn = parse_assistant_turn(&obj_args).unwrap();
        assert_eq!(turn.tool_calls[0].function.name, "count_transcript");
        assert!(turn.tool_calls[0].function.arguments.contains("terms"));

        let legacy: serde_json::Value = serde_json::from_str(
            r#"{"choices":[{"message":{"content":"","function_call":{"name":"submit_result","arguments":"{}"}}}]}"#,
        )
        .unwrap();
        let turn2 = parse_assistant_turn(&legacy).unwrap();
        assert_eq!(turn2.tool_calls[0].function.name, "submit_result");
    }
}

