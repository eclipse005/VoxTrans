use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::db::store::TaskStore;
use super::error::LlmError;
use super::json_guard::JsonResponseValidator;

static LLM_REQUEST_SEQ: AtomicU64 = AtomicU64::new(1);

/// Sampling temperature for translation/connectivity requests. Shared by both
/// wire dialects so a protocol switch never changes translation behavior.
pub(super) const TRANSLATION_TEMPERATURE: f64 = 0.2;
/// Terminology-agent tool turns run slightly hotter.
pub(super) const AGENT_TEMPERATURE: f64 = 0.3;

#[derive(Debug, Clone)]
pub struct LlmConfig {
    pub base_url: String,
    api_keys: Arc<[String]>,
    key_cursor: Arc<AtomicUsize>,
    pub model: String,
    pub max_retries: u32,
}

impl LlmConfig {
    pub fn new(base_url: String, api_key: String, model: String) -> Self {
        Self {
            base_url,
            api_keys: parse_api_keys(&api_key),
            key_cursor: Arc::new(AtomicUsize::new(0)),
            model,
            max_retries: 3,
        }
    }

    /// Override the retry budget. Connectivity probes pass `0` so a failed
    /// test returns immediately instead of waiting out retries.
    pub fn with_max_retries(mut self, max_retries: u32) -> Self {
        self.max_retries = max_retries;
        self
    }

    /// Next key in round-robin order. Concurrent calls share the cursor so
    /// `aaa|bbb|ccc` is spread across in-flight requests, not pinned to the
    /// first key.
    pub fn next_api_key(&self) -> &str {
        match self.api_keys.as_ref() {
            [] => "",
            [only] => only.as_str(),
            keys => {
                let i = self.key_cursor.fetch_add(1, Ordering::Relaxed);
                keys[i % keys.len()].as_str()
            }
        }
    }
}

/// Split `aaa|bbb|ccc` into trimmed non-empty keys. A single key (no `|`) is
/// unchanged. Empty slots (`a||b`) are dropped.
pub fn parse_api_keys(raw: &str) -> Arc<[String]> {
    raw.split('|')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>()
        .into()
}

#[derive(Debug, Clone, Default)]
pub struct LlmTokenUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    /// Anthropic prompt-cache accounting (log-only; DB schema untouched).
    /// Both are also included in `prompt_tokens`.
    pub cache_creation_tokens: u64,
    pub cache_read_tokens: u64,
}

// ── Internal message model ──────────────────────────────────────────────────
//
// Provider-neutral: this is the OpenAI-shaped conversation the agent harness
// and the translation pipeline both speak. `anthropic.rs` converts it to
// content blocks; `openai.rs` serializes it as-is. Kept here (rather than in
// either wire module) so neither dialect owns it.

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
    /// Raw provider `stop_reason` (`max_tokens` / `end_turn` / `tool_use` /
    /// `stop_sequence` / vendor-specific), `None` when the endpoint omitted
    /// it. kimi-code FinishReason: the harness needs to know a turn was cut
    /// by the output cap — truncated prose without tool calls is a
    /// recoverable condition, not a silent protocol failure.
    pub stop_reason: Option<String>,
}

impl AssistantTurn {
    /// Did this turn end because the output budget ran out? Normalizes the
    /// Anthropic (`max_tokens`) and OpenAI-style (`length`) spellings.
    pub fn is_truncated(&self) -> bool {
        matches!(
            self.stop_reason.as_deref().map(str::trim),
            Some("max_tokens") | Some("length")
        )
    }
}

#[derive(Debug, Clone)]
pub struct LlmCallContext {
    pub task_id: String,
    pub media_path: Option<String>,
    pub phase: String,
    pub store: Option<TaskStore>,
    /// JSON Schema forwarded as Anthropic structured output
    /// (`output_config.format`). Endpoints that reject it fall back to
    /// plain-JSON generation with the existing repair chain.
    pub output_schema: Option<Value>,
}

#[derive(Debug, Clone)]
pub struct LlmJsonTask {
    pub id: usize,
    pub request_id: String,
    pub user_prompt: Arc<str>,
    pub response_validator: Option<JsonResponseValidator>,
}

#[derive(Debug, Clone)]
pub struct LlmJsonResult {
    pub json: Value,
}

pub fn next_llm_request_id() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    let seq = LLM_REQUEST_SEQ.fetch_add(1, Ordering::Relaxed);
    format!("{millis}-{:04x}", seq & 0xffff)
}

pub trait LlmPort {
    #[allow(async_fn_in_trait)]
    async fn call_json(
        &self,
        context: &LlmCallContext,
        request_id: &str,
        user_prompt: &str,
        response_validator: Option<&JsonResponseValidator>,
    ) -> Result<LlmJsonResult, LlmError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;
    use std::thread;

    #[test]
    fn parse_api_keys_splits_on_pipe_and_drops_empty() {
        assert_eq!(parse_api_keys("aaa").as_ref(), &["aaa".to_string()]);
        assert_eq!(
            parse_api_keys("aaa|bbb|ccc").as_ref(),
            &["aaa".to_string(), "bbb".to_string(), "ccc".to_string()]
        );
        assert_eq!(
            parse_api_keys(" aaa | bbb | ").as_ref(),
            &["aaa".to_string(), "bbb".to_string()]
        );
        assert!(parse_api_keys("").is_empty());
        assert!(parse_api_keys("|||").is_empty());
    }

    #[test]
    fn next_api_key_rotates_round_robin() {
        let cfg = LlmConfig::new("u".into(), "aaa|bbb|ccc".into(), "m".into());
        assert_eq!(cfg.next_api_key(), "aaa");
        assert_eq!(cfg.next_api_key(), "bbb");
        assert_eq!(cfg.next_api_key(), "ccc");
        assert_eq!(cfg.next_api_key(), "aaa");
    }

    #[test]
    fn next_api_key_single_key_never_moves() {
        let cfg = LlmConfig::new("u".into(), "only".into(), "m".into());
        assert_eq!(cfg.next_api_key(), "only");
        assert_eq!(cfg.next_api_key(), "only");
    }

    #[test]
    fn cloned_config_shares_cursor() {
        let cfg = LlmConfig::new("u".into(), "a|b".into(), "m".into());
        let clone = cfg.clone();
        assert_eq!(cfg.next_api_key(), "a");
        assert_eq!(clone.next_api_key(), "b");
        assert_eq!(cfg.next_api_key(), "a");
    }

    #[test]
    fn round_robin_is_balanced_under_concurrency() {
        let cfg = Arc::new(LlmConfig::new("u".into(), "a|b|c".into(), "m".into()));
        let counts = Arc::new(Mutex::new(HashMap::<String, usize>::new()));
        let mut handles = Vec::new();
        for _ in 0..30 {
            let cfg = cfg.clone();
            let counts = counts.clone();
            handles.push(thread::spawn(move || {
                let key = cfg.next_api_key().to_string();
                *counts.lock().unwrap().entry(key).or_insert(0) += 1;
            }));
        }
        for handle in handles {
            handle.join().expect("thread");
        }
        let counts = counts.lock().unwrap();
        assert_eq!(counts.get("a"), Some(&10));
        assert_eq!(counts.get("b"), Some(&10));
        assert_eq!(counts.get("c"), Some(&10));
    }
}
