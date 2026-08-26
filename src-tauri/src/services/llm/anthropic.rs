//! Anthropic Messages wire transport (`POST {base}/v1/messages`).
//!
//! Protocol boundary only: the internal [`ChatMessage`]/[`ToolCall`] models
//! stay provider-neutral (the terminology agent keeps using them), and
//! conversion to Anthropic content blocks happens here — kimi-code
//! `convertMessage` strategy. Callers in `client.rs` keep their public API.

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::base_url::normalize_base_url;
use super::error::{LlmError, LlmErrorKind};
use super::port::{LlmConfig, LlmTokenUsage};

/// `anthropic-version` header required on every Messages API request.
pub(crate) const ANTHROPIC_VERSION: &str = "2023-06-01";
/// Sampling temperature for translation/connectivity requests (parity with
/// the previous OpenAI-compatible transport).
const TRANSLATION_TEMPERATURE: f64 = 0.2;
/// Terminology-agent tool turns run slightly hotter.
const AGENT_TEMPERATURE: f64 = 0.3;
/// Output budget for models we cannot classify as Claude. Third-party
/// Anthropic-compatible endpoints may reject large `max_tokens`, so this is
/// deliberately conservative — kimi-code falls back to 128000 because it only
/// talks to real Claude models; our users mostly point at compatible vendors.
const FALLBACK_MAX_TOKENS: u32 = 8_192;

// ── Internal message model (moved verbatim from the old OpenAI transport) ──

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

// ── Endpoints ────────────────────────────────────────────────────────────────

fn messages_endpoint(base_url: &str) -> String {
    let normalized = normalize_base_url(base_url);
    if normalized.ends_with("/v1") {
        format!("{normalized}/messages")
    } else {
        format!("{normalized}/v1/messages")
    }
}

/// `GET {base}/v1/models` for the settings model picker. Shares the join rule
/// with [`messages_endpoint`] so a base ending in `/v1` never doubles up.
pub fn models_endpoint(base_url: &str) -> String {
    let normalized = normalize_base_url(base_url);
    if normalized.ends_with("/v1") {
        format!("{normalized}/models")
    } else {
        format!("{normalized}/v1/models")
    }
}

// ── max_tokens resolution (kimi-code family table, conservative fallback) ──

/// Parse `(family, major, minor)` from a model name. Handles both
/// `claude-sonnet-4-5` and `claude-3-5-sonnet` orderings; 8+ digit tokens are
/// date stamps and never read as versions. Any family-word hit counts as a
/// Claude-shaped model (wire-path parity with kimi-code).
fn parse_claude_version(model: &str) -> Option<(&'static str, u32, Option<u32>)> {
    const FAMILIES: [&str; 5] = ["fable", "mythos", "opus", "sonnet", "haiku"];
    let normalized = model.to_ascii_lowercase();
    let tokens: Vec<&str> = normalized
        .split(['-', '_', '.'])
        .filter(|t| !t.is_empty())
        .collect();
    let fam_idx = tokens.iter().position(|t| FAMILIES.contains(t))?;
    let family: &'static str = match tokens[fam_idx] {
        "fable" => "fable",
        "mythos" => "mythos",
        "opus" => "opus",
        "sonnet" => "sonnet",
        _ => "haiku",
    };
    // 1–2 digit version; longer digit runs are dates like `-20250929`.
    let version = |t: &str| -> Option<u32> {
        if t.len() <= 2 && t.chars().all(|c| c.is_ascii_digit()) {
            t.parse().ok()
        } else {
            None
        }
    };
    // family-first: sonnet-4-5 / opus-4
    if let Some(major) = tokens.get(fam_idx + 1).copied().and_then(version) {
        let minor = tokens.get(fam_idx + 2).copied().and_then(version);
        return Some((family, major, minor));
    }
    // version-first: claude-3-5-sonnet / claude-3-sonnet
    if fam_idx >= 2
        && let Some(major) = version(tokens[fam_idx - 2])
    {
        let minor = version(tokens[fam_idx - 1]);
        return Some((family, major, minor));
    }
    // bare major: claude-3-opus / sonnet-3 (one version token before family)
    if fam_idx >= 1
        && let Some(major) = version(tokens[fam_idx - 1])
    {
        return Some((family, major, None));
    }
    None
}

/// Max-output ceiling per family/major/minor. Tables descend so an unknown
/// future minor inherits from its nearest known sibling, then the family-major
/// baseline (kimi-code `lookupClaudeCeiling` walk).
fn output_ceiling(family: &str, major: u32, minor: Option<u32>) -> Option<u32> {
    const K128: u32 = 128_000;
    const K64: u32 = 64_000;
    let walk = |minor: Option<u32>, table: &[(u32, u32)], baseline: u32| -> u32 {
        if let Some(m) = minor {
            for (floor, value) in table {
                if m >= *floor {
                    return *value;
                }
            }
        }
        baseline
    };
    match (family, major) {
        ("fable", 5) | ("mythos", 5) => Some(K128),
        ("opus", 4) => Some(walk(
            minor,
            &[(8, K128), (7, K128), (6, K128), (5, K64), (1, 32_000), (0, 32_000)],
            32_000,
        )),
        ("opus", 3) => Some(walk(minor, &[(7, 8_192), (5, 8_192), (1, 8_192), (0, 4_096)], 4_096)),
        ("sonnet", 5) => Some(K128),
        ("sonnet", 4) => Some(walk(
            minor,
            &[(6, K128), (5, K64), (4, K64), (3, K64), (2, K64), (1, K64), (0, K64)],
            K64,
        )),
        ("sonnet", 3) => Some(walk(minor, &[(7, 8_192), (5, 8_192), (1, 8_192), (0, 4_096)], 4_096)),
        // haiku-4-5 and haiku-4 share the same cap.
        ("haiku", 4) => Some(K64),
        ("haiku", 3) => Some(walk(minor, &[(5, 8_192), (0, 4_096)], 4_096)),
        _ => None,
    }
}

/// Resolve the mandatory `max_tokens` for a request: the family ceiling for
/// known Claude models, the conservative fallback otherwise (compatible
/// endpoints may reject large values).
pub(super) fn resolve_max_tokens(model: &str) -> u32 {
    parse_claude_version(model)
        .and_then(|(family, major, minor)| output_ceiling(family, major, minor))
        .unwrap_or(FALLBACK_MAX_TOKENS)
}

/// Claude-family detection for the thinking echo rules below. Unsigned
/// thinking must be dropped for real Claude models but preserved for
/// compatible endpoints that reject tool rounds without it.
fn is_claude_model(model: &str) -> bool {
    parse_claude_version(model).is_some()
}

// ── Tool-call id policy (64 chars, `[A-Za-z0-9_-]`) ────────────────────────

const TOOL_CALL_ID_MAX_CHARS: usize = 64;

fn sanitize_tool_call_id(id: &str) -> String {
    let cleaned: String = id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .take(TOOL_CALL_ID_MAX_CHARS)
        .collect();
    cleaned
}

/// Maps raw provider ids to wire-safe ids consistently across a whole history,
/// so a `tool_use.id` and its answering `tool_result.tool_use_id` always land
/// on the same sanitized value (with collision suffixes).
#[derive(Default)]
struct ToolIdMapper {
    map: std::collections::HashMap<String, String>,
    used: std::collections::HashSet<String>,
}

impl ToolIdMapper {
    fn map(&mut self, raw: &str) -> String {
        if let Some(existing) = self.map.get(raw) {
            return existing.clone();
        }
        // Truncate the base FIRST and reserve room for any collision suffix,
        // so two raw ids whose sanitized forms share a long prefix still map
        // to distinct wire ids (appending the suffix after truncation could
        // cut it off again and collapse them).
        let base: String = sanitize_tool_call_id(raw)
            .chars()
            .take(TOOL_CALL_ID_MAX_CHARS)
            .collect();
        let mut n = 0u32;
        let candidate = loop {
            let suffix = if n == 0 { String::new() } else { format!("_{n}") };
            // Sanitized output is ASCII, so byte indexing is char-safe.
            let body_end = TOOL_CALL_ID_MAX_CHARS
                .saturating_sub(suffix.len())
                .min(base.len());
            let candidate = format!("{}{suffix}", &base[..body_end]);
            n += 1;
            if self.used.insert(candidate.clone()) {
                break candidate;
            }
        };
        self.map.insert(raw.to_string(), candidate.clone());
        candidate
    }
}

// ── Message conversion ──────────────────────────────────────────────────────

struct ConvertedConversation {
    system: Option<String>,
    /// Wire messages `{role, content:[blocks]}` after the directional merge.
    messages: Vec<Value>,
}

/// Convert internal history into Anthropic wire form:
/// - leading `system` messages join into the top-level `system` param
///   (mid-history ones become `<system>`-wrapped user turns);
/// - `tool` rows become `tool_result` blocks inside user turns;
/// - consecutive user turns merge asymmetrically (`merge-user-messages.ts`
///   semantics): a tool-result-only turn absorbs whatever follows, a text
///   turn absorbs only text — `[text][tool_result]` stays split;
/// - empty assistant turns are dropped; unsigned thinking blocks are kept
///   only for non-Claude models (real Claude rejects them on replay).
fn convert_messages(messages: &[ChatMessage], model: &str) -> ConvertedConversation {
    let claude_model = is_claude_model(model);
    let mut ids = ToolIdMapper::default();
    let mut system_parts: Vec<String> = Vec::new();
    // (wire message, is_tool_result_only)
    let mut converted: Vec<(Value, bool)> = Vec::new();

    for m in messages {
        let content_text = m.content.as_deref().unwrap_or_default();
        match m.role.as_str() {
            "system" => {
                if content_text.trim().is_empty() {
                    continue;
                }
                if converted.is_empty() {
                    system_parts.push(content_text.to_string());
                } else {
                    converted.push((
                        json!({
                            "role": "user",
                            "content": [{"type":"text","text":format!("<system>{content_text}</system>")}],
                        }),
                        false,
                    ));
                }
            }
            "user" => {
                if content_text.trim().is_empty() {
                    continue;
                }
                converted.push((
                    json!({"role":"user","content":[{"type":"text","text":content_text}]}),
                    false,
                ));
            }
            "assistant" => {
                let mut blocks: Vec<Value> = Vec::new();
                if !claude_model && let Some(reasoning) = &m.reasoning_content && !reasoning.trim().is_empty() {
                    blocks.push(json!({"type":"thinking","thinking":reasoning}));
                }
                if !content_text.trim().is_empty() {
                    blocks.push(json!({"type":"text","text":content_text}));
                }
                for tc in m.tool_calls.iter().flatten() {
                    // Arguments round-trip as a JSON object; fall back to an
                    // empty object rather than failing the whole history.
                    let input = serde_json::from_str::<Value>(&tc.function.arguments)
                        .ok()
                        .filter(|v| v.is_object())
                        .unwrap_or_else(|| json!({}));
                    blocks.push(json!({
                        "type":"tool_use",
                        "id": ids.map(&tc.id),
                        "name": tc.function.name,
                        "input": input,
                    }));
                }
                if blocks.is_empty() {
                    continue;
                }
                converted.push((json!({"role":"assistant","content":blocks}), false));
            }
            "tool" => {
                let tool_use_id = m
                    .tool_call_id
                    .as_deref()
                    .map(|id| ids.map(id))
                    .unwrap_or_default();
                converted.push((
                    json!({
                        "role":"user",
                        "content":[{
                            "type":"tool_result",
                            "tool_use_id": tool_use_id,
                            "content":[{"type":"text","text":content_text}],
                        }],
                    }),
                    true,
                ));
            }
            _ => {}
        }
    }

    // Directional merge of consecutive user turns:
    // merge iff `last` is tool-result-only OR `next` is not tool-result-only.
    let mut merged: Vec<(Value, bool)> = Vec::new();
    for (message, tool_only) in converted {
        if let Some((last, last_tool_only)) = merged.last_mut()
            && last.get("role").and_then(Value::as_str) == Some("user")
            && message.get("role").and_then(Value::as_str) == Some("user")
            && (*last_tool_only || !tool_only)
        {
            let mut combined = last["content"].as_array().cloned().unwrap_or_default();
            combined.extend(message["content"].as_array().cloned().unwrap_or_default());
            *last = json!({"role":"user","content":combined});
            *last_tool_only = *last_tool_only && tool_only;
        } else {
            merged.push((message, tool_only));
        }
    }

    // Invariant repair: the Messages API requires strictly alternating
    // roles. The directional pass above keeps kimi-code's merge semantics;
    // any adjacent same-role turns it leaves behind (e.g. a plain user text
    // turn right before a tool_result turn) are merged here unconditionally
    // — all content is kept, only the wire shape is normalized.
    let mut messages: Vec<Value> = Vec::with_capacity(merged.len());
    for (message, _) in merged {
        if let Some(last) = messages.last_mut()
            && last.get("role") == message.get("role")
        {
            let mut combined = last["content"].as_array().cloned().unwrap_or_default();
            combined.extend(message["content"].as_array().cloned().unwrap_or_default());
            *last = json!({"role": message["role"].clone(), "content": combined});
        } else {
            messages.push(message);
        }
    }

    // The Messages API requires the first turn to be a user turn.
    while messages
        .first()
        .map(|m| m.get("role").and_then(Value::as_str) != Some("user"))
        .unwrap_or(false)
    {
        messages.remove(0);
    }

    ConvertedConversation {
        system: if system_parts.is_empty() {
            None
        } else {
            Some(system_parts.join("\n\n"))
        },
        messages,
    }
}

/// OpenAI function-tool JSON → Anthropic tool definitions. Already-Anthropic
/// shapes pass through untouched.
fn convert_tool_schemas(tools: &[Value]) -> Vec<Value> {
    tools
        .iter()
        .map(|tool| {
            let Some(function) = tool.get("function") else {
                return tool.clone();
            };
            json!({
                "name": function.get("name").cloned().unwrap_or(Value::Null),
                "description": function.get("description").cloned().unwrap_or(json!("")),
                "input_schema": function.get("parameters").cloned().unwrap_or(json!({"type":"object","properties":{}})),
            })
        })
        .collect()
}

// ── Request body ────────────────────────────────────────────────────────────

struct MessageCallParams<'a> {
    model: &'a str,
    system: Option<String>,
    messages: Vec<Value>,
    temperature: f64,
    stream: bool,
    tools: Option<Vec<Value>>,
    output_schema: Option<&'a Value>,
}

fn build_request_body(params: &MessageCallParams<'_>) -> Value {
    let mut body = json!({
        "model": params.model,
        // Mandatory on the Messages API.
        "max_tokens": resolve_max_tokens(params.model),
        "temperature": params.temperature,
        "stream": params.stream,
        "messages": params.messages,
    });
    // Cache breakpoints follow kimi-code's layout, applied conditionally by
    // economics: system/tools prefixes repeat across calls and pay off; a
    // last-message breakpoint only pays off when history actually grows
    // (agent tool loops). Stateless single-shot calls (translation batches)
    // would re-write the cache every batch with ~zero hits, so they get none.
    if let Some(system) = &params.system {
        body["system"] = json!([{
            "type":"text",
            "text": system,
            "cache_control":{"type":"ephemeral"},
        }]);
    }
    if let Some(tools) = &params.tools {
        let mut converted = convert_tool_schemas(tools);
        if let Some(last) = converted.last_mut() {
            last["cache_control"] = json!({"type":"ephemeral"});
        }
        body["tools"] = Value::Array(converted);
        body["tool_choice"] = json!({"type":"auto"});
    }
    if converted_messages_len(&body["messages"]) >= 2
        && let Some(last_message) = body["messages"].as_array_mut().and_then(|m| m.last_mut())
    {
        mark_last_block_cached(last_message);
    }
    if let Some(schema) = params.output_schema {
        body["output_config"] = json!({"format":{"type":"json_schema","schema":schema}});
    }
    body
}

fn converted_messages_len(messages: &Value) -> usize {
    messages.as_array().map(Vec::len).unwrap_or(0)
}

fn mark_last_block_cached(message: &mut Value) {
    if let Some(blocks) = message
        .get_mut("content")
        .and_then(Value::as_array_mut)
        && let Some(last) = blocks.last_mut()
    {
        last["cache_control"] = json!({"type":"ephemeral"});
    }
}

// ── HTTP plumbing ───────────────────────────────────────────────────────────

/// Auth for Anthropic-compatible endpoints: the key rides in BOTH schemes —
/// `x-api-key` (official Messages API shape) and `Authorization: Bearer`
/// (relay/gateway shape). Vendor `/anthropic` endpoints accept either and
/// ignore the other; one-api/new-api style relays only read `Authorization`.
pub(crate) fn add_auth_headers(
    builder: reqwest::RequestBuilder,
    api_key: &str,
) -> reqwest::RequestBuilder {
    if api_key.is_empty() {
        return builder;
    }
    builder
        .header("x-api-key", api_key)
        .header("Authorization", format!("Bearer {api_key}"))
}

async fn post_messages(
    http: &reqwest::Client,
    config: &LlmConfig,
    body: &Value,
) -> Result<reqwest::Response, LlmError> {
    let endpoint = messages_endpoint(&config.base_url);
    let request = add_auth_headers(
        http.post(&endpoint).header("anthropic-version", ANTHROPIC_VERSION),
        &config.next_api_key(),
    );
    let response = request.json(body).send().await.map_err(|err| {
        LlmError::new(LlmErrorKind::Http, format!("http request failed: {err}"))
    })?;
    let status = response.status();
    if !status.is_success() {
        let retry_after_ms = response
            .headers()
            .get("retry-after")
            .and_then(|value| value.to_str().ok())
            .and_then(|raw| raw.trim().parse::<u64>().ok())
            .map(|secs| secs.saturating_mul(1000));
        let text = response.text().await.unwrap_or_default();
        return Err(api_error(status.as_u16(), retry_after_ms, &text));
    }
    Ok(response)
}

/// Build an [`LlmError`] from a non-2xx response body, parsing the
/// `{type:"error", error:{type,message}}` envelope when present and carrying
/// the status code plus Retry-After override for the retry loop.
pub(super) fn api_error(status: u16, retry_after_ms: Option<u64>, body: &str) -> LlmError {
    let parsed: Option<Value> = serde_json::from_str(body).ok();
    let detail = parsed
        .as_ref()
        .filter(|value| {
            value.get("type").and_then(Value::as_str) == Some("error")
                || value.get("error").is_some_and(Value::is_object)
        })
        .and_then(|value| value.pointer("/error/message"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| truncate_chars(body.trim(), 300));
    LlmError {
        kind: LlmErrorKind::Http,
        message: format!("http status {status}: {detail}"),
        status: Some(status),
        retry_after_ms,
    }
}

fn truncate_chars(input: &str, max_chars: usize) -> String {
    if input.chars().count() <= max_chars {
        return input.to_string();
    }
    let out: String = input.chars().take(max_chars).collect();
    format!("{out}...")
}

// ── Usage ───────────────────────────────────────────────────────────────────

fn json_u64(value: Option<&Value>) -> Option<u64> {
    let value = value?;
    if let Some(n) = value.as_u64() {
        return Some(n);
    }
    if let Some(n) = value.as_i64() {
        return Some(n.max(0) as u64);
    }
    if let Some(n) = value.as_f64() {
        return Some(n.max(0.0) as u64);
    }
    if let Some(s) = value.as_str() {
        return s.trim().parse().ok();
    }
    None
}

/// Anthropic usage → internal usage. `prompt_tokens` keeps the historical
/// meaning of *all* input tokens (uncached + cache-write + cache-read); the
/// cache split rides along for logs only.
fn usage_from_anthropic(usage: &Value) -> LlmTokenUsage {
    let input = json_u64(usage.get("input_tokens")).unwrap_or(0);
    let cache_creation = json_u64(usage.get("cache_creation_input_tokens")).unwrap_or(0);
    let cache_read = json_u64(usage.get("cache_read_input_tokens")).unwrap_or(0);
    let completion = json_u64(usage.get("output_tokens")).unwrap_or(0);
    let prompt = input + cache_creation + cache_read;
    LlmTokenUsage {
        prompt_tokens: prompt,
        completion_tokens: completion,
        total_tokens: prompt + completion,
        cache_creation_tokens: cache_creation,
        cache_read_tokens: cache_read,
    }
}

// ── Non-stream plain call (translation batches, connectivity probe) ────────

pub(super) async fn call_message(
    http: &reqwest::Client,
    config: &LlmConfig,
    user_prompt: &str,
    output_schema: Option<&Value>,
) -> Result<(String, LlmTokenUsage), LlmError> {
    let conv = convert_messages(&[ChatMessage::user(user_prompt)], &config.model);
    let body = build_request_body(&MessageCallParams {
        model: &config.model,
        system: conv.system,
        messages: conv.messages,
        temperature: TRANSLATION_TEMPERATURE,
        stream: false,
        tools: None,
        output_schema,
    });
    let response = post_messages(http, config, &body).await?;
    let payload: Value = response.json().await.map_err(|err| {
        LlmError::new(
            LlmErrorKind::Http,
            format!("anthropic response decode failed: {err}"),
        )
    })?;
    let text = joined_text_blocks(payload.get("content"))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            LlmError::new(LlmErrorKind::Http, "response missing assistant text content")
        })?;
    let usage = payload
        .get("usage")
        .map(usage_from_anthropic)
        .unwrap_or_default();
    Ok((text, usage))
}

fn joined_text_blocks(content: Option<&Value>) -> Option<String> {
    let blocks = content?.as_array()?;
    let mut out = String::new();
    for block in blocks {
        if block.get("type").and_then(Value::as_str) != Some("text") {
            continue;
        }
        let text = block.get("text").and_then(Value::as_str).unwrap_or_default();
        if text.trim().is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(text);
    }
    if out.trim().is_empty() {
        None
    } else {
        Some(out)
    }
}

// ── Streaming plain call (translation preview) ─────────────────────────────

pub(super) async fn call_message_stream(
    http: &reqwest::Client,
    config: &LlmConfig,
    user_prompt: &str,
    output_schema: Option<&Value>,
    mut on_delta: Option<&mut (dyn FnMut(&str) + Send)>,
) -> Result<(String, LlmTokenUsage), LlmError> {
    let conv = convert_messages(&[ChatMessage::user(user_prompt)], &config.model);
    let body = build_request_body(&MessageCallParams {
        model: &config.model,
        system: conv.system,
        messages: conv.messages,
        temperature: TRANSLATION_TEMPERATURE,
        stream: true,
        tools: None,
        output_schema,
    });
    let response = post_messages(http, config, &body).await?;

    let mut byte_stream = response.bytes_stream();
    let mut line_buf = String::new();
    let mut frames = SseFrameParser::new();
    let mut acc = String::new();
    let mut parts = UsageParts::default();

    // Pull complete SSE lines out of the buffer, refilling from the network
    // until EOF (a trailing partial line still counts as one final line).
    loop {
        let next_line = line_buf.find('\n').map(|nl| {
            let line = line_buf[..nl].trim_end_matches('\r').to_string();
            line_buf.drain(..nl + 1);
            line
        });
        let Some(line) = next_line else {
            match byte_stream.next().await {
                Some(Ok(chunk)) => {
                    line_buf.push_str(&String::from_utf8_lossy(&chunk));
                    continue;
                }
                Some(Err(err)) => {
                    return Err(LlmError::new(
                        LlmErrorKind::Http,
                        format!("http stream read failed: {err}"),
                    ));
                }
                None => {
                    let rest = std::mem::take(&mut line_buf);
                    let rest = rest.trim_end_matches('\r');
                    if let Some((event, data)) = frames.feed_line(rest) {
                        apply_stream_frame(event.as_deref(), &data, &mut acc, &mut parts, &mut on_delta)?;
                    }
                    break;
                }
            }
        };
        if let Some((event, data)) = frames.feed_line(&line) {
            let stop = apply_stream_frame(event.as_deref(), &data, &mut acc, &mut parts, &mut on_delta)?;
            if stop {
                break;
            }
        }
    }
    finish_stream(acc, parts, &mut on_delta)
}

fn finish_stream(
    acc: String,
    parts: UsageParts,
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
    if !parts.has_counts() {
        // Same contract as the previous transport: surface missing usage
        // instead of silently billing zero.
        eprintln!(
            "[warn] anthropic stream finished without usage; token count for this call will be 0"
        );
    }
    Ok((acc, parts.finish()))
}

/// Accumulates SSE lines into named frames (`event:` … `data:` … blank line).
struct SseFrameParser {
    event: Option<String>,
    data: String,
}

impl SseFrameParser {
    fn new() -> Self {
        Self {
            event: None,
            data: String::new(),
        }
    }

    /// Feed one complete line; returns `(event, data)` when a frame closes on
    /// the blank separator line.
    fn feed_line(&mut self, line: &str) -> Option<(Option<String>, String)> {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if self.event.is_none() && self.data.is_empty() {
                return None;
            }
            return Some((self.event.take(), std::mem::take(&mut self.data)));
        }
        if let Some(rest) = trimmed.strip_prefix("event:") {
            self.event = Some(rest.trim().to_string());
        } else if let Some(rest) = trimmed.strip_prefix("data:") {
            if !self.data.is_empty() {
                self.data.push('\n');
            }
            self.data.push_str(rest.trim_start());
        }
        // `:` comments and unknown field lines are ignored.
        None
    }
}

/// Apply one decoded SSE frame. Returns `Ok(true)` when `message_stop` was
/// reached. Mid-stream `error` events become hard errors so callers can fall
/// back / retry — kimi-code ignores these; we must not.
fn apply_stream_frame(
    event: Option<&str>,
    data: &str,
    acc: &mut String,
    parts: &mut UsageParts,
    on_delta: &mut Option<&mut (dyn FnMut(&str) + Send)>,
) -> Result<bool, LlmError> {
    if data.trim().is_empty() {
        return Ok(false);
    }
    let payload: Value = serde_json::from_str(data).map_err(|err| {
        LlmError::new(
            LlmErrorKind::Http,
            format!("anthropic sse decode failed: {err}; raw={}", truncate_chars(data, 200)),
        )
    })?;
    let kind = event
        .map(|e| e.trim().to_ascii_lowercase())
        .or_else(|| payload.get("type").and_then(Value::as_str).map(str::to_ascii_lowercase))
        .unwrap_or_default();
    match kind.as_str() {
        "message_start" => {
            if let Some(usage) = payload.pointer("/message/usage") {
                parts.replace(usage);
            }
        }
        "content_block_delta" => {
            if payload.pointer("/delta/type").and_then(Value::as_str) == Some("text_delta")
                && let Some(text) = payload.pointer("/delta/text").and_then(Value::as_str)
            {
                acc.push_str(text);
                if let Some(cb) = on_delta.as_mut() {
                    cb(acc);
                }
            }
            // thinking_delta / input_json_delta / signature_delta: translation
            // previews show final text only, and tool calls never stream here.
        }
        "message_delta" => {
            if let Some(usage) = payload.get("usage") {
                parts.merge(usage);
            }
        }
        "error" => {
            let err_type = payload
                .pointer("/error/type")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let message = payload
                .pointer("/error/message")
                .and_then(Value::as_str)
                .unwrap_or("stream error");
            return Err(LlmError::new(
                LlmErrorKind::Http,
                format!("anthropic stream error ({err_type}): {message}"),
            ));
        }
        // ping / content_block_start / content_block_stop / message_stop /
        // anything unknown: nothing to extract (stop handled via return value).
        _ => {}
    }
    Ok(kind == "message_stop")
}

/// Usage accumulated from both sources (`message_start` replaces wholesale,
/// `message_delta` overwrites per-field) before final conversion.
#[derive(Default)]
struct UsageParts {
    seen: bool,
    input: u64,
    output: u64,
    cache_creation: u64,
    cache_read: u64,
}

impl UsageParts {
    fn replace(&mut self, usage: &Value) {
        *self = Self::default();
        self.merge(usage);
    }

    fn merge(&mut self, usage: &Value) {
        if let Some(n) = json_u64(usage.get("input_tokens")) {
            self.input = n;
            self.seen = true;
        }
        if let Some(n) = json_u64(usage.get("output_tokens")) {
            self.output = n;
            self.seen = true;
        }
        if let Some(n) = json_u64(usage.get("cache_creation_input_tokens")) {
            self.cache_creation = n;
        }
        if let Some(n) = json_u64(usage.get("cache_read_input_tokens")) {
            self.cache_read = n;
        }
    }

    fn has_counts(&self) -> bool {
        self.seen
            && (self.input > 0
                || self.output > 0
                || self.cache_creation > 0
                || self.cache_read > 0)
    }

    fn finish(&self) -> LlmTokenUsage {
        let prompt = self.input + self.cache_creation + self.cache_read;
        LlmTokenUsage {
            prompt_tokens: prompt,
            completion_tokens: self.output,
            total_tokens: prompt + self.output,
            cache_creation_tokens: self.cache_creation,
            cache_read_tokens: self.cache_read,
        }
    }
}

// ── Tool-calling turn (terminology agent; always non-streaming) ────────────

pub(super) async fn call_message_tools(
    http: &reqwest::Client,
    config: &LlmConfig,
    messages: &[ChatMessage],
    tools: &[Value],
    temperature: Option<f64>,
) -> Result<AssistantTurn, LlmError> {
    let conv = convert_messages(messages, &config.model);
    let body = build_request_body(&MessageCallParams {
        model: &config.model,
        system: conv.system,
        messages: conv.messages,
        temperature: temperature.unwrap_or(AGENT_TEMPERATURE),
        stream: false,
        tools: if tools.is_empty() { None } else { Some(tools.to_vec()) },
        output_schema: None,
    });
    let response = post_messages(http, config, &body).await?;
    let payload: Value = response.json().await.map_err(|err| {
        LlmError::new(
            LlmErrorKind::Http,
            format!("anthropic response decode failed: {err}"),
        )
    })?;
    parse_assistant_turn(&payload, &config.model)
}

fn parse_assistant_turn(payload: &Value, model: &str) -> Result<AssistantTurn, LlmError> {
    let blocks = payload.get("content").and_then(Value::as_array);
    let mut texts: Vec<&str> = Vec::new();
    let mut thinkings: Vec<&str> = Vec::new();
    let mut tool_calls: Vec<ToolCall> = Vec::new();
    for block in blocks.into_iter().flatten() {
        match block.get("type").and_then(Value::as_str) {
            Some("text") => {
                let text = block.get("text").and_then(Value::as_str).unwrap_or_default();
                if !text.trim().is_empty() {
                    texts.push(text);
                }
            }
            // Signed/unsigned distinction is moot here: requests never enable
            // thinking, so real-Claude responses carry none. Compatible-endpoint
            // reasoning is preserved for non-Claude models only — echoing it
            // back to a real Claude endpoint would be rejected.
            Some("thinking") => {
                if !is_claude_model(model) {
                    let text = block
                        .get("thinking")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    if !text.trim().is_empty() {
                        thinkings.push(text);
                    }
                }
            }
            Some("tool_use") => {
                let name = block
                    .get("name")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty());
                let Some(name) = name else { continue };
                let id = block
                    .get("id")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                    .unwrap_or("tool_use");
                let arguments = block
                    .get("input")
                    .cloned()
                    .unwrap_or_else(|| json!({}))
                    .to_string();
                tool_calls.push(ToolCall {
                    id: id.to_string(),
                    type_: "function".to_string(),
                    function: ToolCallFunction {
                        name: name.to_string(),
                        arguments,
                    },
                });
            }
            // redacted_thinking and anything unknown: not replayable, skip.
            _ => {}
        }
    }
    let content = texts.join("\n");
    if content.trim().is_empty() && tool_calls.is_empty() {
        return Err(LlmError::new(
            LlmErrorKind::Http,
            "response missing assistant text and tool_use",
        ));
    }
    let joined_thinking = thinkings.join("\n");
    Ok(AssistantTurn {
        content,
        tool_calls,
        reasoning_content: if joined_thinking.trim().is_empty() {
            None
        } else {
            Some(joined_thinking)
        },
        usage: payload
            .get("usage")
            .map(usage_from_anthropic)
            .unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── endpoints ───────────────────────────────────────────────────────

    #[test]
    fn endpoint_join_rules() {
        assert_eq!(
            messages_endpoint("https://api.anthropic.com"),
            "https://api.anthropic.com/v1/messages"
        );
        assert_eq!(
            messages_endpoint("https://api.deepseek.com/anthropic/"),
            "https://api.deepseek.com/anthropic/v1/messages"
        );
        assert_eq!(
            messages_endpoint("https://x.example.com/v1"),
            "https://x.example.com/v1/messages"
        );
        assert_eq!(models_endpoint("https://api.anthropic.com"), "https://api.anthropic.com/v1/models");
        assert_eq!(models_endpoint("https://x.example.com/v1"), "https://x.example.com/v1/models");
    }

    #[test]
    fn auth_headers_carry_both_schemes() {
        let client = reqwest::Client::new();
        let request = add_auth_headers(client.get("http://x.local"), "sk-test")
            .build()
            .unwrap();
        assert_eq!(request.headers().get("x-api-key").unwrap(), "sk-test");
        assert_eq!(
            request.headers().get("Authorization").unwrap(),
            "Bearer sk-test"
        );
        // Empty key: neither header is set.
        let request = add_auth_headers(client.get("http://x.local"), "")
            .build()
            .unwrap();
        assert!(request.headers().get("x-api-key").is_none());
        assert!(request.headers().get("Authorization").is_none());
    }

    // ── max_tokens resolution ───────────────────────────────────────────

    #[test]
    fn max_tokens_known_models() {
        let r = |m: &str| resolve_max_tokens(m);
        assert_eq!(r("claude-fable-5"), 128_000);
        assert_eq!(r("claude-mythos-5"), 128_000);
        assert_eq!(r("claude-opus-4-8"), 128_000);
        assert_eq!(r("claude-opus-4-5"), 64_000);
        assert_eq!(r("claude-opus-4-1"), 32_000);
        assert_eq!(r("claude-sonnet-5"), 128_000);
        assert_eq!(r("claude-sonnet-4-6"), 128_000);
        assert_eq!(r("claude-sonnet-4-5"), 64_000);
        assert_eq!(r("claude-haiku-4-5"), 64_000);
        assert_eq!(r("claude-3-7-sonnet-latest"), 8_192);
        assert_eq!(r("claude-3-5-haiku-20241022"), 8_192); // date token ignored
        assert_eq!(r("claude-3-opus"), 4_096);
    }

    #[test]
    fn max_tokens_unknown_and_non_claude_fallback() {
        // Compatible-endpoint model names get the conservative fallback.
        assert_eq!(resolve_max_tokens("deepseek-v4-flash"), FALLBACK_MAX_TOKENS);
        assert_eq!(resolve_max_tokens("kimi-k2-thinking"), FALLBACK_MAX_TOKENS);
        assert_eq!(resolve_max_tokens(""), FALLBACK_MAX_TOKENS);
        // Unknown claude-shaped name still parses as Claude but has no ceiling.
        assert_eq!(resolve_max_tokens("sonnet-latest"), FALLBACK_MAX_TOKENS);
        assert_eq!(resolve_max_tokens("glm-4.6"), FALLBACK_MAX_TOKENS);
    }

    #[test]
    fn version_parser_forms() {
        assert_eq!(
            parse_claude_version("Claude-Sonnet-4.5"),
            Some(("sonnet", 4, Some(5)))
        );
        assert_eq!(
            parse_claude_version("claude-3-5-sonnet-20241022"),
            Some(("sonnet", 3, Some(5)))
        );
        assert_eq!(parse_claude_version("opus-4"), Some(("opus", 4, None)));
        assert_eq!(parse_claude_version("kimi-k2-thinking"), None);
        // Bare family word carries no version at all.
        assert_eq!(parse_claude_version("haiku-latest"), None);
        // Bare major before the family.
        assert_eq!(parse_claude_version("claude-3-opus"), Some(("opus", 3, None)));
    }

    #[test]
    fn haiku_latest_inherits_baseline() {
        // haiku with no usable major falls back conservatively.
        assert_eq!(resolve_max_tokens("haiku-latest"), FALLBACK_MAX_TOKENS);
    }

    // ── message conversion ──────────────────────────────────────────────

    #[test]
    fn leading_systems_join_and_mid_system_wraps() {
        let msgs = vec![
            ChatMessage::system("first rules"),
            ChatMessage::user("hello"),
            ChatMessage::system("mid-stream note"),
        ];
        let conv = convert_messages(&msgs, "claude-sonnet-5");
        assert_eq!(conv.system.as_deref(), Some("first rules"));
        // The wrapped mid-history system merges into the preceding text turn.
        assert_eq!(conv.messages.len(), 1);
        assert_eq!(
            conv.messages[0]["content"][1]["text"].as_str(),
            Some("<system>mid-stream note</system>")
        );
    }

    #[test]
    fn merge_directional_rules() {
        let tool_only = ChatMessage::tool("t1", "result");
        // [text user][tool_result] → the directional pass keeps them split,
        // then the alternation repair merges them into one user turn
        // (text first, tool_result second).
        let conv = convert_messages(
            &[ChatMessage::user("question"), tool_only.clone()],
            "claude-sonnet-5",
        );
        assert_eq!(conv.messages.len(), 1);
        assert_eq!(conv.messages[0]["content"][0]["type"], json!("text"));
        assert_eq!(conv.messages[0]["content"][1]["type"], json!("tool_result"));

        // [tool_result][tool_result] → merged.
        let conv = convert_messages(
            &[tool_only.clone(), ChatMessage::tool("t2", "result2")],
            "claude-sonnet-5",
        );
        assert_eq!(conv.messages.len(), 1);
        assert_eq!(conv.messages[0]["content"].as_array().map(Vec::len), Some(2));

        // [tool_result][text] → merged into one turn ending in text.
        let conv = convert_messages(&[tool_only, ChatMessage::user("next question")], "claude-sonnet-5");
        assert_eq!(conv.messages.len(), 1);
        assert_eq!(
            conv.messages[0]["content"][1]["type"].as_str(),
            Some("text")
        );

        // [text][text] → merged.
        let conv = convert_messages(
            &[ChatMessage::user("a"), ChatMessage::user("b")],
            "claude-sonnet-5",
        );
        assert_eq!(conv.messages.len(), 1);
    }

    #[test]
    fn alternation_repair_merges_adjacent_same_role_turns() {
        // Defensive: mid-history consecutive assistant turns with no tool
        // round between them collapse into one assistant message instead of
        // an invalid wire form (a LEADING assistant would be dropped by the
        // first-turn rule instead).
        let msgs = vec![
            ChatMessage::user("start"),
            ChatMessage::assistant_text("part one"),
            ChatMessage::assistant_text("part two"),
        ];
        let conv = convert_messages(&msgs, "claude-sonnet-5");
        assert_eq!(conv.messages.len(), 2);
        assert_eq!(conv.messages[1]["role"], json!("assistant"));
        assert_eq!(conv.messages[1]["content"].as_array().map(Vec::len), Some(2));
    }

    #[test]
    fn empty_assistant_dropped_and_leading_assistant_removed() {
        let msgs = vec![
            ChatMessage::assistant_text("   "),
            ChatMessage::user("hi"),
        ];
        let conv = convert_messages(&msgs, "claude-sonnet-5");
        assert_eq!(conv.messages.len(), 1);

        // Defensive normalization: assistant cannot open the conversation.
        let msgs = vec![
            ChatMessage::assistant_text("orphan"),
            ChatMessage::user("hi"),
        ];
        let conv = convert_messages(&msgs, "claude-sonnet-5");
        assert_eq!(conv.messages.len(), 1);
        assert_eq!(conv.messages[0]["role"], json!("user"));
    }

    #[test]
    fn tool_id_mapper_suffix_survives_truncation() {
        let mut mapper = ToolIdMapper::default();
        let prefix = "a".repeat(70);
        let first = mapper.map(&format!("{prefix}_1"));
        let second = mapper.map(&format!("{prefix}_2"));
        // Both raw ids sanitize to the same 64-char base; the reserved suffix
        // room must keep them distinct instead of collapsing after truncate.
        assert_ne!(first, second);
        for id in [&first, &second] {
            assert!(id.chars().count() <= TOOL_CALL_ID_MAX_CHARS);
            assert!(id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'));
        }
        // The same raw id stays stable across calls.
        assert_eq!(mapper.map(&format!("{prefix}_1")), first);
    }

    #[test]
    fn tool_ids_sanitized_consistently() {
        let long_id = format!("toolu_{}", "x".repeat(100));
        let msgs = vec![
            ChatMessage::user("find terms"),
            ChatMessage {
                role: "assistant".into(),
                content: None,
                tool_calls: Some(vec![ToolCall {
                    id: long_id.clone(),
                    type_: "function".into(),
                    function: ToolCallFunction {
                        name: "search_terms".into(),
                        arguments: "{\"query\":\"glossary\"}".into(),
                    },
                }]),
                tool_call_id: None,
                reasoning_content: None,
            },
            ChatMessage::tool(long_id, "found"),
        ];
        let conv = convert_messages(&msgs, "claude-sonnet-5");
        let use_id = conv.messages[1]["content"][0]["id"].as_str().expect("tool_use id");
        assert!(use_id.chars().count() <= 64);
        assert!(use_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'));
        // The tool_result turn merges into the assistant? No — it follows an
        // assistant turn, so it stays a separate user turn.
        assert_eq!(conv.messages[2]["content"][0]["tool_use_id"].as_str(), Some(use_id));
        // Arguments parsed into an object.
        assert_eq!(
            conv.messages[1]["content"][0]["input"]["query"],
            json!("glossary")
        );
    }

    #[test]
    fn thinking_kept_for_non_claude_dropped_for_claude() {
        let msgs = vec![
            ChatMessage::user("q"),
            ChatMessage {
                role: "assistant".into(),
                content: Some("answer".into()),
                tool_calls: None,
                tool_call_id: None,
                reasoning_content: Some("private thoughts".into()),
            },
        ];
        let kimi = convert_messages(&msgs, "kimi-k2-thinking");
        assert_eq!(kimi.messages.len(), 2);
        assert_eq!(kimi.messages[1]["content"][0]["type"], json!("thinking"));
        let claude = convert_messages(&msgs, "claude-sonnet-5");
        assert_eq!(claude.messages[1]["content"][0]["type"], json!("text"));
    }

    // ── request body / cache breakpoints ────────────────────────────────

    fn body_with(messages: Vec<ChatMessage>, tools: Option<Vec<Value>>, schema: Option<&Value>) -> Value {
        let conv = convert_messages(&messages, "claude-sonnet-5");
        build_request_body(&MessageCallParams {
            model: "claude-sonnet-5",
            system: conv.system,
            messages: conv.messages,
            temperature: TRANSLATION_TEMPERATURE,
            stream: false,
            tools,
            output_schema: schema,
        })
    }

    #[test]
    fn single_message_gets_no_message_cache_control() {
        let body = body_with(vec![ChatMessage::user("batch")], None, None);
        // No system/tools/message-level cache markers at all.
        assert!(body.get("system").is_none());
        assert_eq!(body["max_tokens"], json!(128_000));
        let blocks = body["messages"][0]["content"].as_array().unwrap();
        for block in blocks {
            assert!(block.get("cache_control").is_none());
        }
    }

    #[test]
    fn system_and_tools_get_breakpoints() {
        let mut msgs = vec![ChatMessage::system("rules"), ChatMessage::user("q")];
        msgs.insert(0, ChatMessage::system("more"));
        let tools = vec![json!({
            "type":"function",
            "function":{"name":"a","description":"d","parameters":{"type":"object","properties":{}}}
        })];
        let body = body_with(msgs, Some(tools.clone()), None);
        let system = body["system"].as_array().unwrap();
        assert_eq!(system.last().unwrap()["cache_control"]["type"], json!("ephemeral"));
        let wire_tools = body["tools"].as_array().unwrap();
        assert_eq!(wire_tools.last().unwrap()["cache_control"]["type"], json!("ephemeral"));
        assert_eq!(wire_tools[0]["name"], json!("a"));
        assert_eq!(wire_tools[0]["input_schema"]["type"], json!("object"));
        assert_eq!(body["tool_choice"], json!({"type":"auto"}));
    }

    #[test]
    fn multi_turn_history_gets_last_block_breakpoint() {
        let msgs = vec![
            ChatMessage::user("q"),
            ChatMessage::assistant_tools(None, vec![ToolCall {
                id: "t1".into(),
                type_: "function".into(),
                function: ToolCallFunction { name: "a".into(), arguments: "{}".into() },
            }], None),
            ChatMessage::tool("t1", "res"),
        ];
        let body = body_with(msgs, None, None);
        let last_blocks = body["messages"]
            .as_array()
            .and_then(|messages| messages.last())
            .and_then(|message| message["content"].as_array())
            .expect("converted history");
        assert_eq!(last_blocks.last().unwrap()["cache_control"]["type"], json!("ephemeral"));
    }

    #[test]
    fn output_schema_becomes_output_config() {
        let schema = json!({"type":"object","properties":{}});
        let body = body_with(vec![ChatMessage::user("batch")], None, Some(&schema));
        assert_eq!(body["output_config"]["format"]["type"], json!("json_schema"));
        assert_eq!(body["output_config"]["format"]["schema"], schema);
    }

    // ── SSE parsing ─────────────────────────────────────────────────────

    #[test]
    fn sse_parser_cross_chunk_frames() {
        let mut parser = SseFrameParser::new();
        assert!(parser.feed_line("event: content_block_delta").is_none());
        assert!(parser.feed_line("data: {\"type\":\"content_block_delta\"}").is_none());
        let frame = parser.feed_line("").expect("frame closes on blank line");
        assert_eq!(frame.0.as_deref(), Some("content_block_delta"));
        assert_eq!(frame.1, "{\"type\":\"content_block_delta\"}");
        // Comments and stray fields are ignored.
        assert!(parser.feed_line(": keep-alive comment").is_none());
    }

    #[test]
    fn stream_frames_accumulate_text_and_usage() {
        let mut acc = String::new();
        let mut parts = UsageParts::default();
        let cb_invoked = std::sync::atomic::AtomicUsize::new(0);
        let mut on_delta: Option<&mut (dyn FnMut(&str) + Send)> =
            Some(&mut |_delta: &str| {
                cb_invoked.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            });

        let start = json!({
            "type":"message_start",
            "message":{"usage":{"input_tokens":10,"cache_read_input_tokens":5}}
        });
        apply_stream_frame(Some("message_start"), &start.to_string(), &mut acc, &mut parts, &mut on_delta).unwrap();

        let delta = json!({"type":"content_block_delta","delta":{"type":"text_delta","text":"你好"}});
        apply_stream_frame(Some("content_block_delta"), &delta.to_string(), &mut acc, &mut parts, &mut on_delta).unwrap();
        assert_eq!(acc, "你好");

        let end = json!({
            "type":"message_delta",
            "usage":{"output_tokens":7,"cache_creation_input_tokens":2}
        });
        let stop = apply_stream_frame(Some("message_delta"), &end.to_string(), &mut acc, &mut parts, &mut on_delta).unwrap();
        assert!(!stop);

        let usage_event = json!({"type":"message_stop"});
        let stop = apply_stream_frame(Some("message_stop"), &usage_event.to_string(), &mut acc, &mut parts, &mut on_delta).unwrap();
        assert!(stop);
        // Callback bookkeeping reads only after the callback borrow ends.
        assert_eq!(
            cb_invoked.load(std::sync::atomic::Ordering::SeqCst),
            1
        );
        let usage = parts.finish();
        assert_eq!(usage.prompt_tokens, 17); // 10 input + 5 read + 2 creation
        assert_eq!(usage.cache_read_tokens, 5);
        assert_eq!(usage.completion_tokens, 7);
        assert_eq!(usage.total_tokens, 24);
    }

    #[test]
    fn stream_error_event_is_hard_error() {
        let mut acc = String::new();
        let mut parts = UsageParts::default();
        let mut none: Option<&mut (dyn FnMut(&str) + Send)> = None;
        let payload = json!({"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}});
        let err = apply_stream_frame(Some("error"), &payload.to_string(), &mut acc, &mut parts, &mut none)
            .expect_err("error event must fail the stream");
        assert!(err.message.contains("overloaded_error"), "{}", err.message);
        assert!(err.message.contains("Overloaded"), "{}", err.message);
    }

    #[test]
    fn stream_ping_and_payload_type_fallback_ignored() {
        let mut acc = String::new();
        let mut parts = UsageParts::default();
        let mut none: Option<&mut (dyn FnMut(&str) + Send)> = None;
        // ping event
        apply_stream_frame(Some("ping"), "{\"type\":\"ping\"}", &mut acc, &mut parts, &mut none).unwrap();
        // No named event line → fall back to payload["type"].
        let stop = apply_stream_frame(
            None,
            "{\"type\":\"message_stop\"}",
            &mut acc,
            &mut parts,
            &mut none,
        )
        .unwrap();
        assert!(stop);
        assert!(acc.is_empty());
    }

    // ── tool response parse ─────────────────────────────────────────────

    #[test]
    fn assistant_turn_parses_blocks_per_model_family() {
        let payload = json!({
            "content":[
                {"type":"thinking","thinking":"reasoning trace"},
                {"type":"text","text":"using glossary"},
                {"type":"redacted_thinking","data":"xxx"},
                {"type":"tool_use","id":"tu_1","name":"save_terms","input":{"pairs":[{"en":"a","zh":"甲"}]}},
                {"type":"tool_use","id":"","name":"  ","input":{}}
            ],
            "usage":{"input_tokens":3,"output_tokens":9}
        });
        let kimi = parse_assistant_turn(&payload, "kimi-k2-thinking").unwrap();
        assert_eq!(kimi.content, "using glossary");
        assert_eq!(kimi.reasoning_content.as_deref(), Some("reasoning trace"));
        assert_eq!(kimi.tool_calls.len(), 1); // blank-name tool_use skipped
        assert_eq!(kimi.tool_calls[0].function.name, "save_terms");
        assert!(serde_json::from_str::<Value>(&kimi.tool_calls[0].function.arguments).unwrap()["pairs"]
            .is_array());

        // Real Claude drops unsigned thinking entirely.
        let claude = parse_assistant_turn(&payload, "claude-sonnet-5").unwrap();
        assert!(claude.reasoning_content.is_none());
        assert_eq!(claude.usage.prompt_tokens, 3);
    }

    #[test]
    fn assistant_turn_rejects_empty_response() {
        let err = parse_assistant_turn(&json!({"content":[]}), "claude-sonnet-5")
            .expect_err("empty content must error");
        assert!(err.message.contains("missing assistant text"), "{}", err.message);
    }

    // ── api_error ───────────────────────────────────────────────────────

    #[test]
    fn api_error_envelope_retry_after_and_truncation() {
        let err = api_error(429, Some(12_000), r#"{"type":"error","error":{"type":"rate_limit_error","message":"Slow down"}}"#);
        assert_eq!(err.kind, LlmErrorKind::Http);
        assert_eq!(err.status, Some(429));
        assert_eq!(err.retry_after_ms, Some(12_000));
        assert!(err.message.contains("http status 429"), "{}", err.message);
        assert!(err.message.contains("Slow down"), "{}", err.message);

        // Non-JSON bodies fall back to a truncated raw excerpt.
        let long_body = "x".repeat(500);
        let err = api_error(500, None, &long_body);
        assert_eq!(err.retry_after_ms, None);
        assert!(err.message.ends_with("..."));
        assert!(err.message.chars().count() < 400);
    }
}
