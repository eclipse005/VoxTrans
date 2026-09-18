//! OpenAI Chat Completions wire transport (`POST {base}/v1/chat/completions`).
//!
//! Mirror image of `anthropic.rs`: that one converts the internal message
//! model into Anthropic content blocks, while this one serializes it as-is —
//! [`ChatMessage`]/[`ToolCall`] already are the OpenAI shape, so tool
//! definitions and tool-call history pass through untouched (no
//! `convert_tool_schemas` here; that exists only for the Messages direction).
//!
//! Differences from the Messages dialect that callers must not see:
//! - auth is `Authorization: Bearer` only (`x-api-key` means nothing here);
//! - structured output is `response_format.json_schema`, which many
//!   OSS/self-hosted endpoints reject — the client's existing rejection
//!   fallback drops it and lets the JSON repair chain take over;
//! - a streamed reply carries no usage unless `stream_options` asks for it.

use serde_json::{Value, json};

use super::base_url::normalize_base_url;
use super::error::{LlmError, LlmErrorKind};
use super::http::{drive_sse, json_u64};
use super::max_tokens::effective_max_tokens;
use super::port::{
    AGENT_TEMPERATURE, AssistantTurn, ChatMessage, LlmConfig, LlmTokenUsage, TRANSLATION_TEMPERATURE,
    ToolCall, ToolCallFunction,
};
use super::text::truncate_chars;

/// Name attached to the structured-output schema. OpenAI requires one;
/// nothing reads it back.
const RESULT_JSON_SCHEMA_NAME: &str = "result";

// ── Endpoint ────────────────────────────────────────────────────────────────

fn chat_completions_endpoint(base_url: &str) -> String {
    let normalized = normalize_base_url(base_url);
    if normalized.to_ascii_lowercase().ends_with("/chat/completions") {
        return normalized;
    }
    if normalized.ends_with("/v1") {
        format!("{normalized}/chat/completions")
    } else {
        format!("{normalized}/v1/chat/completions")
    }
}

// ── Request body ────────────────────────────────────────────────────────────

struct ChatCallParams<'a> {
    model: &'a str,
    /// Serialized [`ChatMessage`]s, passed through verbatim.
    messages: Vec<Value>,
    temperature: f64,
    stream: bool,
    /// Resolved output budget — callers pass [`effective_max_tokens`] so the
    /// adaptive degrade can rebuild the body with a lowered value.
    max_tokens: u32,
    tools: Option<&'a [Value]>,
    output_schema: Option<&'a Value>,
}

fn build_chat_body(params: &ChatCallParams<'_>) -> Value {
    let mut body = json!({
        "model": params.model,
        "messages": params.messages,
        "temperature": params.temperature,
        "stream": params.stream,
        "max_tokens": params.max_tokens,
    });
    if params.stream {
        // Without this the final chunk omits usage and the task silently
        // bills zero tokens. Endpoints that reject the field fail the request
        // outright, and the caller degrades to a non-stream call.
        body["stream_options"] = json!({"include_usage": true});
    }
    if let Some(tools) = params.tools.filter(|tools| !tools.is_empty()) {
        body["tools"] = Value::Array(tools.to_vec());
        body["tool_choice"] = json!("auto");
    }
    if let Some(schema) = params.output_schema {
        body["response_format"] = json!({
            "type": "json_schema",
            "json_schema": {
                "name": RESULT_JSON_SCHEMA_NAME,
                "schema": schema,
                // The schemas in this codebase were written for Anthropic's
                // `output_config`, which has no strict mode. `strict: true`
                // demands the full OpenAI subset (every property listed in
                // `required`, `additionalProperties: false` everywhere) and
                // would reject most of them, so ask for the schema without
                // the guarantee.
                "strict": false,
            }
        });
    }
    body
}

// ── HTTP plumbing ───────────────────────────────────────────────────────────

async fn post_chat_completions(
    http: &reqwest::Client,
    config: &LlmConfig,
    body: &Value,
) -> Result<reqwest::Response, LlmError> {
    let endpoint = chat_completions_endpoint(&config.base_url);
    let request = http
        .post(&endpoint)
        .bearer_auth(config.next_api_key());
    super::http::send_and_check(request.json(body)).await
}

/// [`post_chat_completions`] with the shared output-cap degrade strategy (see
/// [`super::http::degrade_max_tokens_after_rejection`]).
async fn post_chat_adaptive(
    http: &reqwest::Client,
    config: &LlmConfig,
    build_body: impl Fn(u32) -> Value,
) -> Result<reqwest::Response, LlmError> {
    let mut max_tokens = effective_max_tokens(&config.base_url, &config.model);
    loop {
        match post_chat_completions(http, config, &build_body(max_tokens)).await {
            Ok(response) => return Ok(response),
            Err(err) => {
                if let Some(lowered) =
                    super::http::degrade_max_tokens_after_rejection(&err, config, max_tokens)
                {
                    max_tokens = lowered;
                } else {
                    return Err(err);
                }
            }
        }
    }
}

// ── Usage ───────────────────────────────────────────────────────────────────

/// OpenAI usage → internal usage. `prompt_tokens` already counts cached
/// input, so the cache split rides along for logs only — same contract as
/// the Anthropic mapper.
fn usage_from_openai(usage: &Value) -> LlmTokenUsage {
    let prompt = json_u64(usage.get("prompt_tokens")).unwrap_or(0);
    let completion = json_u64(usage.get("completion_tokens")).unwrap_or(0);
    let total = json_u64(usage.get("total_tokens"))
        .filter(|total| *total > 0)
        .unwrap_or(prompt.saturating_add(completion));
    LlmTokenUsage {
        prompt_tokens: prompt,
        completion_tokens: completion,
        total_tokens: total,
        cache_creation_tokens: 0,
        cache_read_tokens: json_u64(usage.pointer("/prompt_tokens_details/cached_tokens"))
            .unwrap_or(0),
    }
}

// ── Response decoding ───────────────────────────────────────────────────────

/// Assistant text out of a `content` field. Vendors return either a plain
/// string or the multimodal parts array even for text-only replies.
fn text_from_content(content: Option<&Value>) -> Option<String> {
    let text = match content? {
        Value::String(text) => text.clone(),
        Value::Array(parts) => {
            let mut joined = String::new();
            for part in parts {
                if part.get("type").and_then(Value::as_str) == Some("text")
                    && let Some(text) = part.get("text").and_then(Value::as_str)
                    && !text.trim().is_empty()
                {
                    if !joined.is_empty() {
                        joined.push('\n');
                    }
                    joined.push_str(text);
                }
            }
            joined
        }
        _ => return None,
    };
    let trimmed = text.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

// ── Non-stream plain call (translation batches, connectivity probe) ────────

pub(super) async fn call_chat_completion(
    http: &reqwest::Client,
    config: &LlmConfig,
    user_prompt: &str,
    output_schema: Option<&Value>,
) -> Result<(String, LlmTokenUsage), LlmError> {
    let messages = vec![json!(ChatMessage::user(user_prompt))];
    let response = post_chat_adaptive(http, config, |max_tokens| {
        build_chat_body(&ChatCallParams {
            model: &config.model,
            messages: messages.clone(),
            temperature: TRANSLATION_TEMPERATURE,
            stream: false,
            max_tokens,
            tools: None,
            output_schema,
        })
    })
    .await?;
    let payload: Value = response.json().await.map_err(|err| {
        LlmError::new(
            LlmErrorKind::Http,
            format!("openai response decode failed: {err}"),
        )
    })?;
    let text = text_from_content(payload.pointer("/choices/0/message/content")).ok_or_else(|| {
        LlmError::new(LlmErrorKind::Http, "response missing assistant text content")
    })?;
    let usage = payload
        .get("usage")
        .map(usage_from_openai)
        .unwrap_or_default();
    Ok((text, usage))
}

// ── Streaming plain call (translation preview) ─────────────────────────────

pub(super) async fn call_chat_completion_stream(
    http: &reqwest::Client,
    config: &LlmConfig,
    user_prompt: &str,
    output_schema: Option<&Value>,
    mut on_delta: Option<&mut (dyn FnMut(&str) + Send)>,
) -> Result<(String, LlmTokenUsage), LlmError> {
    let messages = vec![json!(ChatMessage::user(user_prompt))];
    let response = post_chat_adaptive(http, config, |max_tokens| {
        build_chat_body(&ChatCallParams {
            model: &config.model,
            messages: messages.clone(),
            temperature: TRANSLATION_TEMPERATURE,
            stream: true,
            max_tokens,
            tools: None,
            output_schema,
        })
    })
    .await?;

    let mut acc = String::new();
    let mut usage: Option<Value> = None;
    drive_sse(response, |_event, data| {
        apply_chat_stream_frame(data, &mut acc, &mut usage, &mut on_delta)
    })
    .await?;
    finish_chat_stream(acc, usage, &mut on_delta)
}

/// Apply one decoded SSE frame. Returns `Ok(true)` on the `[DONE]` sentinel.
/// Mid-stream failures arrive as an ordinary chunk-shaped error envelope and
/// become hard errors so callers can fall back / retry.
fn apply_chat_stream_frame(
    data: &str,
    acc: &mut String,
    usage: &mut Option<Value>,
    on_delta: &mut Option<&mut (dyn FnMut(&str) + Send)>,
) -> Result<bool, LlmError> {
    let trimmed = data.trim();
    if trimmed.is_empty() {
        return Ok(false);
    }
    // The dialect's one non-JSON frame.
    if trimmed == "[DONE]" {
        return Ok(true);
    }
    let payload: Value = serde_json::from_str(trimmed).map_err(|err| {
        LlmError::new(
            LlmErrorKind::Http,
            format!("openai sse decode failed: {err}; raw={}", truncate_chars(trimmed, 200)),
        )
    })?;
    if let Some(error) = payload.get("error") {
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("stream error");
        return Err(LlmError::new(
            LlmErrorKind::Http,
            format!("openai stream error: {message}"),
        ));
    }
    // Only sent on the final chunk, and only because `stream_options` asked.
    if let Some(chunk_usage) = payload.get("usage").filter(|value| !value.is_null()) {
        *usage = Some(chunk_usage.clone());
    }
    match payload.pointer("/choices/0/delta/content") {
        // Appended raw, exactly like the Messages transport: providers split
        // prose at arbitrary points, and a whitespace-only chunk between two
        // words is content, not noise — trimming it would silently glue
        // "hello", " ", "world" into "helloworld".
        Some(Value::String(text)) if !text.is_empty() => push_delta(acc, text, on_delta),
        // The parts-array shape, which providers only send pre-assembled.
        Some(content) if !content.is_null() => {
            if let Some(text) = text_from_content(Some(content)) {
                push_delta(acc, &text, on_delta);
            }
        }
        _ => {}
    }
    Ok(false)
}

fn push_delta(acc: &mut String, text: &str, on_delta: &mut Option<&mut (dyn FnMut(&str) + Send)>) {
    acc.push_str(text);
    if let Some(cb) = on_delta.as_mut() {
        cb(acc);
    }
}

fn finish_chat_stream(
    acc: String,
    usage: Option<Value>,
    on_delta: &mut Option<&mut (dyn FnMut(&str) + Send)>,
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
    let Some(usage) = usage else {
        // Same contract as the Messages transport: surface missing usage
        // instead of silently billing zero.
        eprintln!(
            "[warn] openai stream finished without usage; token count for this call will be 0"
        );
        return Ok((acc, LlmTokenUsage::default()));
    };
    Ok((acc, usage_from_openai(&usage)))
}

// ── Tool-calling turn (terminology agent; always non-streaming) ────────────

pub(super) async fn call_chat_completion_tools(
    http: &reqwest::Client,
    config: &LlmConfig,
    messages: &[ChatMessage],
    tools: &[Value],
    temperature: Option<f64>,
) -> Result<AssistantTurn, LlmError> {
    let serialized: Vec<Value> = messages.iter().map(|message| json!(message)).collect();
    let response = post_chat_adaptive(http, config, |max_tokens| {
        build_chat_body(&ChatCallParams {
            model: &config.model,
            messages: serialized.clone(),
            temperature: temperature.unwrap_or(AGENT_TEMPERATURE),
            stream: false,
            max_tokens,
            tools: Some(tools),
            output_schema: None,
        })
    })
    .await?;
    let payload: Value = response.json().await.map_err(|err| {
        LlmError::new(
            LlmErrorKind::Http,
            format!("openai response decode failed: {err}"),
        )
    })?;
    parse_assistant_turn(&payload)
}

fn parse_assistant_turn(payload: &Value) -> Result<AssistantTurn, LlmError> {
    let choice = payload
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .ok_or_else(|| LlmError::new(LlmErrorKind::Http, "response missing choices[0]"))?;
    // Some gateways drop the `message` wrapper; treat the choice as the message.
    let message = choice.get("message").unwrap_or(choice);
    let content = text_from_content(message.get("content")).unwrap_or_default();
    // `reasoning_content` is DeepSeek/Qwen spelling, `reasoning` the gateway one.
    let reasoning = message
        .get("reasoning_content")
        .and_then(Value::as_str)
        .or_else(|| message.get("reasoning").and_then(Value::as_str))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let mut tool_calls = parse_tool_calls(message.get("tool_calls"));
    if tool_calls.is_empty()
        && let Some(legacy) = message.get("function_call")
        && let Some(call) = tool_call_from_function(legacy, "call_legacy")
    {
        tool_calls.push(call);
    }
    if content.trim().is_empty() && tool_calls.is_empty() {
        return Err(LlmError::new(
            LlmErrorKind::Http,
            "response missing assistant text and tool_calls",
        ));
    }
    // kimi-code FinishInfo: the raw provider finish reason rides along so the
    // harness can tell a clean end from a `length` truncation.
    let stop_reason = choice
        .get("finish_reason")
        .and_then(Value::as_str)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    Ok(AssistantTurn {
        content,
        tool_calls,
        reasoning_content: reasoning,
        usage: payload
            .get("usage")
            .map(usage_from_openai)
            .unwrap_or_default(),
        stop_reason,
    })
}

fn parse_tool_calls(value: Option<&Value>) -> Vec<ToolCall> {
    let Some(calls) = value.and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (index, call) in calls.iter().enumerate() {
        let id = call
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| format!("call_{index}"));
        let function = call.get("function").unwrap_or(call);
        if let Some(tool_call) = tool_call_from_function(function, &id) {
            out.push(tool_call);
        }
    }
    out
}

fn tool_call_from_function(function: &Value, id: &str) -> Option<ToolCall> {
    let name = function.get("name").and_then(Value::as_str)?.trim();
    if name.is_empty() {
        return None;
    }
    // `arguments` is a JSON *string* per spec, but gateways that pre-parse it
    // hand back an object; keep whatever we got rather than dropping the call.
    let arguments = match function.get("arguments") {
        Some(Value::String(raw)) => raw.clone(),
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

#[cfg(test)]
mod tests {
    use super::*;

    // ── endpoint ────────────────────────────────────────────────────────

    #[test]
    fn endpoint_join_rules() {
        assert_eq!(
            chat_completions_endpoint("https://api.openai.com"),
            "https://api.openai.com/v1/chat/completions"
        );
        assert_eq!(
            chat_completions_endpoint("https://api.commandcode.ai/provider/v1"),
            "https://api.commandcode.ai/provider/v1/chat/completions"
        );
        // A base already ending in `/v1/` must not double up.
        assert_eq!(
            chat_completions_endpoint("https://x.example.com/v1/"),
            "https://x.example.com/v1/chat/completions"
        );
        // A fully spelled endpoint is left alone.
        assert_eq!(
            chat_completions_endpoint("https://x.example.com/v1/chat/completions"),
            "https://x.example.com/v1/chat/completions"
        );
    }

    // ── request body ────────────────────────────────────────────────────

    fn body(
        stream: bool,
        tools: Option<&[Value]>,
        schema: Option<&Value>,
        max_tokens: u32,
    ) -> Value {
        build_chat_body(&ChatCallParams {
            model: "deepseek/deepseek-v4-flash-fast",
            messages: vec![json!(ChatMessage::user("hi"))],
            temperature: TRANSLATION_TEMPERATURE,
            stream,
            max_tokens,
            tools,
            output_schema: schema,
        })
    }

    #[test]
    fn body_uses_messages_shape_not_output_config() {
        let body = body(false, None, None, 12_800);
        assert_eq!(body["model"], json!("deepseek/deepseek-v4-flash-fast"));
        assert_eq!(body["messages"][0]["role"], json!("user"));
        assert_eq!(body["messages"][0]["content"], json!("hi"));
        assert_eq!(body["max_tokens"], json!(12_800));
        assert_eq!(body["stream"], json!(false));
        // Anthropic-only fields must never leak into this dialect.
        assert!(body.get("output_config").is_none());
        assert!(body.get("system").is_none());
        assert!(body.get("stream_options").is_none());
        assert!(body.get("response_format").is_none());
        assert!(body.get("tool_choice").is_none());
    }

    #[test]
    fn body_asks_for_usage_only_when_streaming() {
        assert!(body(false, None, None, 1024).get("stream_options").is_none());
        assert_eq!(
            body(true, None, None, 1024)["stream_options"],
            json!({"include_usage": true})
        );
    }

    #[test]
    fn body_wraps_schema_in_response_format() {
        let schema = json!({"type":"object","properties":{"ok":{"type":"boolean"}},"required":["ok"]});
        let body = body(false, None, Some(&schema), 1024);
        assert_eq!(body["response_format"]["type"], json!("json_schema"));
        assert_eq!(body["response_format"]["json_schema"]["schema"], schema);
        assert_eq!(body["response_format"]["json_schema"]["name"], json!("result"));
        // Our schemas are not written to OpenAI's strict subset.
        assert_eq!(body["response_format"]["json_schema"]["strict"], json!(false));
        // Still no Anthropic structured-output field.
        assert!(body.get("output_config").is_none());
    }

    #[test]
    fn body_passes_openai_tools_through_untouched() {
        let tools = vec![json!({
            "type": "function",
            "function": {
                "name": "search_transcript",
                "description": "find a pattern",
                "parameters": {"type":"object","properties":{"pattern":{"type":"string"}}}
            }
        })];
        // Verbatim: the Anthropic converter must NOT run on this path.
        let with_tools = body(false, Some(&tools), None, 1024);
        assert_eq!(with_tools["tools"], json!(tools));
        assert_eq!(with_tools["tool_choice"], json!("auto"));
        assert!(with_tools["tools"][0]["function"]["parameters"].is_object());
        assert!(with_tools["tools"][0].get("input_schema").is_none());

        // Empty tool list means no tools block at all.
        let without_tools = body(false, Some(&[]), None, 1024);
        assert!(without_tools.get("tools").is_none());
        assert!(without_tools.get("tool_choice").is_none());
    }

    // ── usage ───────────────────────────────────────────────────────────

    #[test]
    fn usage_maps_counts_and_cached_tokens() {
        let usage = usage_from_openai(&json!({
            "prompt_tokens": 120,
            "completion_tokens": 30,
            "total_tokens": 150,
            "prompt_tokens_details": {"cached_tokens": 64}
        }));
        assert_eq!(usage.prompt_tokens, 120);
        assert_eq!(usage.completion_tokens, 30);
        assert_eq!(usage.total_tokens, 150);
        assert_eq!(usage.cache_read_tokens, 64);
        assert_eq!(usage.cache_creation_tokens, 0);
    }

    #[test]
    fn usage_derives_total_when_the_gateway_omits_it() {
        let usage = usage_from_openai(&json!({"prompt_tokens": 7, "completion_tokens": 5}));
        assert_eq!(usage.total_tokens, 12);
        // A zero total is treated as "absent" rather than believed.
        let usage = usage_from_openai(&json!({
            "prompt_tokens": 7, "completion_tokens": 5, "total_tokens": 0
        }));
        assert_eq!(usage.total_tokens, 12);
    }

    // ── streaming frames ────────────────────────────────────────────────

    #[test]
    fn stream_frames_accumulate_text_and_stop_on_done() {
        let mut acc = String::new();
        let mut usage = None;
        let mut none: Option<&mut (dyn FnMut(&str) + Send)> = None;

        let first = json!({"choices":[{"delta":{"content":"你"}}]});
        assert!(!apply_chat_stream_frame(&first.to_string(), &mut acc, &mut usage, &mut none).unwrap());
        let second = json!({"choices":[{"delta":{"content":"好"}}]});
        assert!(!apply_chat_stream_frame(&second.to_string(), &mut acc, &mut usage, &mut none).unwrap());
        assert_eq!(acc, "你好");

        // Usage rides on a final chunk that has an empty choices list.
        let final_chunk = json!({
            "choices": [],
            "usage": {"prompt_tokens": 9, "completion_tokens": 2, "total_tokens": 11}
        });
        assert!(!apply_chat_stream_frame(&final_chunk.to_string(), &mut acc, &mut usage, &mut none).unwrap());
        assert_eq!(acc, "你好", "a usage-only chunk must not touch the text");

        assert!(apply_chat_stream_frame("[DONE]", &mut acc, &mut usage, &mut none).unwrap());
        let (text, usage) = finish_chat_stream(acc, usage, &mut none).unwrap();
        assert_eq!(text, "你好");
        assert_eq!(usage.total_tokens, 11);
    }

    #[test]
    fn stream_reasoning_deltas_are_not_shown_as_translation() {
        let mut acc = String::new();
        let mut usage = None;
        let mut none: Option<&mut (dyn FnMut(&str) + Send)> = None;
        let frame = json!({"choices":[{"delta":{"reasoning_content":"thinking…"}}]});
        apply_chat_stream_frame(&frame.to_string(), &mut acc, &mut usage, &mut none).unwrap();
        assert_eq!(acc, "");
    }

    #[test]
    fn stream_invokes_the_delta_callback_with_the_accumulated_text() {
        let mut acc = String::new();
        let mut usage = None;
        let seen = std::sync::Mutex::new(Vec::<String>::new());
        let mut on_delta: Option<&mut (dyn FnMut(&str) + Send)> =
            Some(&mut |text: &str| seen.lock().unwrap().push(text.to_string()));

        let frame = json!({"choices":[{"delta":{"content":"abc"}}]});
        apply_chat_stream_frame(&frame.to_string(), &mut acc, &mut usage, &mut on_delta).unwrap();
        let frame = json!({"choices":[{"delta":{"content":"def"}}]});
        apply_chat_stream_frame(&frame.to_string(), &mut acc, &mut usage, &mut on_delta).unwrap();

        assert_eq!(*seen.lock().unwrap(), vec!["abc".to_string(), "abcdef".to_string()]);
    }

    #[test]
    fn stream_preserves_whitespace_only_deltas() {
        // Providers split prose at arbitrary points, so "hello world" can
        // arrive as three chunks with a bare space in the middle. Trimming
        // deltas would silently produce "helloworld".
        let mut acc = String::new();
        let mut usage = None;
        let mut none: Option<&mut (dyn FnMut(&str) + Send)> = None;
        for part in ["hello", " ", "world"] {
            let frame = json!({"choices":[{"delta":{"content": part}}]});
            apply_chat_stream_frame(&frame.to_string(), &mut acc, &mut usage, &mut none).unwrap();
        }
        assert_eq!(acc, "hello world");

        // The assembled parts shape still works, and is trimmed like the
        // non-streaming decode.
        let parts = json!({"choices":[{"delta":{"content":[
            {"type":"text","text":"  padded  "}
        ]}}]});
        apply_chat_stream_frame(&parts.to_string(), &mut acc, &mut usage, &mut none).unwrap();
        assert_eq!(acc, "hello worldpadded");
    }

    #[test]
    fn stream_error_envelope_is_a_hard_error() {
        let mut acc = String::new();
        let mut usage = None;
        let mut none: Option<&mut (dyn FnMut(&str) + Send)> = None;
        let payload = json!({"error":{"message":"upstream overloaded","type":"server_error"}});
        let err = apply_chat_stream_frame(&payload.to_string(), &mut acc, &mut usage, &mut none)
            .expect_err("error envelope must fail the stream");
        assert!(err.message.contains("upstream overloaded"), "{}", err.message);
    }

    #[test]
    fn stream_ignores_blank_keepalive_frames() {
        let mut acc = String::new();
        let mut usage = None;
        let mut none: Option<&mut (dyn FnMut(&str) + Send)> = None;
        assert!(!apply_chat_stream_frame("", &mut acc, &mut usage, &mut none).unwrap());
        assert!(!apply_chat_stream_frame("   ", &mut acc, &mut usage, &mut none).unwrap());
    }

    #[test]
    fn empty_stream_is_an_error_not_an_empty_translation() {
        let mut none: Option<&mut (dyn FnMut(&str) + Send)> = None;
        let err = finish_chat_stream("   ".to_string(), None, &mut none)
            .expect_err("empty stream must fail");
        assert!(err.message.contains("empty assistant content"), "{}", err.message);
    }

    // ── tool turns ──────────────────────────────────────────────────────

    #[test]
    fn parse_assistant_turn_reads_tool_calls_and_string_arguments() {
        let payload = json!({
            "choices":[{
                "message":{
                    "role":"assistant",
                    "content":null,
                    "tool_calls":[{
                        "id":"call_1",
                        "type":"function",
                        "function":{"name":"search_transcript","arguments":"{\"pattern\":\"foo\"}"}
                    }]
                },
                "finish_reason":"tool_calls"
            }],
            "usage":{"prompt_tokens":10,"completion_tokens":4,"total_tokens":14}
        });
        let turn = parse_assistant_turn(&payload).unwrap();
        assert_eq!(turn.tool_calls.len(), 1);
        assert_eq!(turn.tool_calls[0].function.name, "search_transcript");
        assert!(turn.tool_calls[0].function.arguments.contains("foo"));
        assert_eq!(turn.usage.total_tokens, 14);
        assert_eq!(turn.stop_reason.as_deref(), Some("tool_calls"));
        assert!(!turn.is_truncated());
    }

    #[test]
    fn parse_assistant_turn_tolerates_object_arguments_and_legacy_calls() {
        let object_args = json!({
            "choices":[{"message":{"tool_calls":[
                {"function":{"name":"count_transcript","arguments":{"terms":["a"]}}}
            ]}}]
        });
        let turn = parse_assistant_turn(&object_args).unwrap();
        assert_eq!(turn.tool_calls[0].function.name, "count_transcript");
        assert_eq!(turn.tool_calls[0].id, "call_0", "missing id gets a stable slot id");
        assert!(turn.tool_calls[0].function.arguments.contains("terms"));

        let legacy = json!({
            "choices":[{"message":{"content":"","function_call":{"name":"submit_result","arguments":"{}"}}}]
        });
        let turn = parse_assistant_turn(&legacy).unwrap();
        assert_eq!(turn.tool_calls[0].function.name, "submit_result");
        assert_eq!(turn.tool_calls[0].id, "call_legacy");
    }

    #[test]
    fn parse_assistant_turn_reads_reasoning_and_normalizes_truncation() {
        let payload = json!({
            "choices":[{
                "message":{"content":"partial","reasoning_content":"  because  "},
                "finish_reason":"length"
            }]
        });
        let turn = parse_assistant_turn(&payload).unwrap();
        assert_eq!(turn.content, "partial");
        assert_eq!(turn.reasoning_content.as_deref(), Some("because"));
        // OpenAI spells the output-cap stop `length`; the harness reads both.
        assert!(turn.is_truncated());

        let gateway = json!({
            "choices":[{"message":{"content":"x","reasoning":" alt spelling "}}]
        });
        assert_eq!(
            parse_assistant_turn(&gateway).unwrap().reasoning_content.as_deref(),
            Some("alt spelling")
        );
    }

    #[test]
    fn parse_assistant_turn_rejects_empty_responses() {
        let payload = json!({"choices":[{"message":{"content":"   "}}]});
        let err = parse_assistant_turn(&payload).expect_err("no text and no tool calls");
        assert!(err.message.contains("missing assistant text"), "{}", err.message);

        let no_choices = json!({"error":{"message":"boom"}});
        let err = parse_assistant_turn(&no_choices).expect_err("no choices");
        assert!(err.message.contains("choices[0]"), "{}", err.message);
    }

    #[test]
    fn text_from_content_accepts_string_and_parts_shapes() {
        assert_eq!(
            text_from_content(Some(&json!("  hello  "))).as_deref(),
            Some("hello")
        );
        assert_eq!(
            text_from_content(Some(&json!([
                {"type":"text","text":"a"},
                {"type":"image_url","image_url":{"url":"x"}},
                {"type":"text","text":"b"}
            ])))
            .as_deref(),
            Some("a\nb")
        );
        assert_eq!(text_from_content(Some(&json!("   "))), None);
        assert_eq!(text_from_content(Some(&json!(null))), None);
        assert_eq!(text_from_content(None), None);
    }
}
