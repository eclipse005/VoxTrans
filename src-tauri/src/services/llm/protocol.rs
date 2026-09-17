//! Which wire protocol an endpoint speaks, and how we learn it.
//!
//! Two dialects exist in the wild for the same job:
//! - Anthropic Messages (`POST {base}/v1/messages`)
//! - OpenAI Chat Completions (`POST {base}/v1/chat/completions`)
//!
//! The protocol is not a user preference — it is an endpoint capability. So
//! it is resolved at call time from `(base_url, model)` and remembered
//! in-process, exactly like the other capability memos
//! (`STRUCTURED_OUTPUT_UNSUPPORTED` in client.rs, `MAX_TOKENS_OVERRIDE` in
//! anthropic.rs). Nothing about it is persisted, so settings and the DB
//! schema stay untouched.
//!
//! Aggregators exist that serve BOTH dialects on one host but different model
//! catalogs per dialect (e.g. `api.commandcode.ai/provider/v1`: `/messages`
//! takes Claude models, `/chat/completions` takes OSS models). That is why
//! the memo key carries the model too — switching model on the same URL
//! re-resolves the protocol.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use super::base_url::normalize_base_url;
use super::error::{LlmError, LlmErrorKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LlmProtocol {
    AnthropicMessages,
    OpenAiChatCompletions,
}

impl LlmProtocol {
    pub fn other(self) -> Self {
        match self {
            Self::AnthropicMessages => Self::OpenAiChatCompletions,
            Self::OpenAiChatCompletions => Self::AnthropicMessages,
        }
    }

    /// Log/telemetry name (`llm.call` `provider` field).
    pub fn provider_id(self) -> &'static str {
        match self {
            Self::AnthropicMessages => "anthropic",
            Self::OpenAiChatCompletions => "openai",
        }
    }

    /// Log/telemetry name (`llm.call` `transport` field).
    pub fn transport_id(self) -> &'static str {
        match self {
            Self::AnthropicMessages => "messages",
            Self::OpenAiChatCompletions => "chat_completions",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::AnthropicMessages => "Anthropic Messages",
            Self::OpenAiChatCompletions => "OpenAI Chat Completions",
        }
    }
}

/// Endpoints whose protocol we already learned, keyed by
/// `{normalized base_url}\u{1}{model}` (same key shape as `MAX_TOKENS_OVERRIDE`).
static PROTOCOL_BY_ENDPOINT: OnceLock<Mutex<HashMap<String, LlmProtocol>>> = OnceLock::new();

fn memo_key(base_url: &str, model: &str) -> String {
    format!("{}\u{1}{}", normalize_base_url(base_url), model.trim())
}

fn remembered(base_url: &str, model: &str) -> Option<LlmProtocol> {
    PROTOCOL_BY_ENDPOINT
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .ok()
        .and_then(|map| map.get(&memo_key(base_url, model)).copied())
}

/// Remember which dialect this endpoint+model actually answered on, so later
/// calls skip the probe entirely.
pub fn remember_protocol(base_url: &str, model: &str, protocol: LlmProtocol) {
    if let Ok(mut map) = PROTOCOL_BY_ENDPOINT
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
    {
        map.insert(memo_key(base_url, model), protocol);
    }
}

/// The protocol to try first for this endpoint+model.
///
/// Order of truth: what this endpoint already answered on → what the URL
/// says → Anthropic. The last default is deliberate: every configuration
/// that works today speaks Messages, so an unproven endpoint keeps its
/// current behavior and only leaves it when the endpoint itself objects.
pub fn resolve_protocol(base_url: &str, model: &str) -> LlmProtocol {
    if let Some(known) = remembered(base_url, model) {
        return known;
    }
    heuristic(base_url)
}

/// URL-only guess. A vendor that publishes an `/anthropic` path is telling
/// us outright; a URL already carrying the OpenAI route is equally explicit.
fn heuristic(base_url: &str) -> LlmProtocol {
    let normalized = normalize_base_url(base_url).to_ascii_lowercase();
    if normalized.contains("/anthropic") {
        return LlmProtocol::AnthropicMessages;
    }
    if normalized.ends_with("/chat/completions") {
        return LlmProtocol::OpenAiChatCompletions;
    }
    LlmProtocol::AnthropicMessages
}

/// Does this error mean "you knocked on the wrong door" — i.e. the endpoint
/// is alive and authenticated, it just does not serve this route?
///
/// Only statuses that can plausibly mean a misrouted path qualify, and the
/// match is deliberately narrow so genuine failures keep failing loudly
/// instead of being retried on the other dialect (a false positive costs one
/// extra request and then reports the second error).
pub fn is_protocol_mismatch(err: &LlmError, attempted: LlmProtocol) -> bool {
    if !matches!(err.status, Some(400) | Some(404) | Some(405) | Some(422)) {
        return false;
    }
    // Routing statuses: the request never reached a handler, and on a base URL
    // we composed ourselves the other dialect's route is the likeliest reason.
    if matches!(err.status, Some(404) | Some(405)) {
        return true;
    }
    // Payload statuses: only a signal that names the other door qualifies, so
    // genuine bad requests keep failing loudly instead of being retried on the
    // other dialect. A false positive costs one extra request before the
    // second error is reported.
    let message = err.message.to_ascii_lowercase();
    // The endpoint naming the route we should have used — strongest signal,
    // and what aggregators actually return (`... Use /provider/v1/chat/completions
    // for OpenAI and OSS models.`).
    let other_route = match attempted {
        LlmProtocol::AnthropicMessages => "chat/completions",
        LlmProtocol::OpenAiChatCompletions => "/messages",
    };
    if message.contains(other_route) {
        return true;
    }
    ["not supported on this endpoint", "unsupported endpoint", "wrong endpoint"]
        .iter()
        .any(|phrase| message.contains(phrase))
}

/// Error surfaced when both dialects were tried and neither was accepted.
/// Names the endpoint once and keeps the second failure verbatim — at that
/// point the URL or the key is wrong, not the protocol.
pub fn both_protocols_rejected(
    base_url: &str,
    first: LlmProtocol,
    second: LlmProtocol,
    second_error: &LlmError,
) -> LlmError {
    LlmError::new(
        LlmErrorKind::Config,
        format!(
            "endpoint {} rejected both the {} and {} wire formats; last error: {}",
            normalize_base_url(base_url),
            first.display_name(),
            second.display_name(),
            second_error.message,
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn http_error(status: u16, detail: &str) -> LlmError {
        LlmError {
            kind: LlmErrorKind::Http,
            message: format!("http status {status}: {detail}"),
            status: Some(status),
            retry_after_ms: None,
        }
    }

    #[test]
    fn heuristic_reads_the_url_when_nothing_is_remembered() {
        assert_eq!(
            heuristic("https://api.deepseek.com/anthropic"),
            LlmProtocol::AnthropicMessages
        );
        assert_eq!(
            heuristic("https://api.deepseek.com/anthropic/"),
            LlmProtocol::AnthropicMessages
        );
        assert_eq!(
            heuristic("https://api.openai.com/v1/chat/completions"),
            LlmProtocol::OpenAiChatCompletions
        );
        // Ambiguous hosts keep today's behavior: Messages first.
        assert_eq!(
            heuristic("https://api.commandcode.ai/provider/v1"),
            LlmProtocol::AnthropicMessages
        );
        assert_eq!(heuristic(""), LlmProtocol::AnthropicMessages);
    }

    #[test]
    fn remembered_protocol_beats_the_url_guess() {
        let base = "https://probe.example/v1";
        assert_eq!(resolve_protocol(base, "m"), LlmProtocol::AnthropicMessages);

        remember_protocol(base, "m", LlmProtocol::OpenAiChatCompletions);
        assert_eq!(resolve_protocol(base, "m"), LlmProtocol::OpenAiChatCompletions);
    }

    #[test]
    fn memory_is_per_model_so_switching_model_re_resolves() {
        let base = "https://probe-model.example/v1";
        remember_protocol(base, "oss-model", LlmProtocol::OpenAiChatCompletions);

        // Same URL, Claude id: the aggregator serves that one over Messages.
        assert_eq!(resolve_protocol(base, "claude-sonnet-5"), LlmProtocol::AnthropicMessages);
        assert_eq!(resolve_protocol(base, "oss-model"), LlmProtocol::OpenAiChatCompletions);
    }

    #[test]
    fn memory_normalizes_trailing_slash_and_model_whitespace() {
        remember_protocol("https://norm.example/v1/", " m ", LlmProtocol::OpenAiChatCompletions);
        assert_eq!(
            resolve_protocol("https://norm.example/v1", "m"),
            LlmProtocol::OpenAiChatCompletions
        );
    }

    #[test]
    fn mismatch_matches_the_real_aggregator_rejection() {
        // Verbatim from https://api.commandcode.ai/provider/v1/messages.
        let err = http_error(
            400,
            r#"{"type":"error","error":{"type":"invalid_request_error","message":"Model \"deepseek/deepseek-v4-flash-fast\" is not supported on this endpoint. Use /provider/v1/chat/completions for OpenAI and OSS models."}}"#,
        );
        assert!(is_protocol_mismatch(&err, LlmProtocol::AnthropicMessages));
    }

    #[test]
    fn mismatch_reads_the_route_named_by_the_endpoint() {
        // Told to use /messages while we were on the OpenAI route.
        let err = http_error(400, "model not available here, use /v1/messages instead");
        assert!(is_protocol_mismatch(&err, LlmProtocol::OpenAiChatCompletions));

        // The same text must NOT trigger when we were already on Messages.
        assert!(!is_protocol_mismatch(&err, LlmProtocol::AnthropicMessages));
    }

    #[test]
    fn mismatch_covers_bare_404_and_unsupported_endpoint_wording() {
        assert!(is_protocol_mismatch(&http_error(404, ""), LlmProtocol::AnthropicMessages));
        assert!(is_protocol_mismatch(
            &http_error(404, "Cannot POST /v1/messages"),
            LlmProtocol::AnthropicMessages
        ));
        assert!(is_protocol_mismatch(
            &http_error(400, "unsupported endpoint"),
            LlmProtocol::OpenAiChatCompletions
        ));
        assert!(is_protocol_mismatch(
            &http_error(405, "method not allowed"),
            LlmProtocol::OpenAiChatCompletions
        ));
    }

    #[test]
    fn mismatch_ignores_unrelated_failures() {
        // Auth, quota and schema problems must fail fast on the first dialect.
        assert!(!is_protocol_mismatch(
            &http_error(401, "invalid api key"),
            LlmProtocol::AnthropicMessages
        ));
        assert!(!is_protocol_mismatch(
            &http_error(403, "MODEL_NOT_IN_PLAN: Claude Sonnet 5 available in Pro and above plans"),
            LlmProtocol::AnthropicMessages
        ));
        assert!(!is_protocol_mismatch(
            &http_error(400, "output_config is not supported"),
            LlmProtocol::AnthropicMessages
        ));
        assert!(!is_protocol_mismatch(
            &http_error(429, "rate limit exceeded"),
            LlmProtocol::AnthropicMessages
        ));
        assert!(!is_protocol_mismatch(
            &LlmError::new(LlmErrorKind::Http, "http request failed: dns error"),
            LlmProtocol::AnthropicMessages
        ));
    }

    #[test]
    fn both_rejected_error_names_the_endpoint_and_the_last_cause() {
        let second = http_error(401, "invalid api key");
        let err = both_protocols_rejected(
            "https://x.example/v1",
            LlmProtocol::AnthropicMessages,
            LlmProtocol::OpenAiChatCompletions,
            &second,
        );
        assert_eq!(err.kind, LlmErrorKind::Config);
        assert!(err.message.contains("https://x.example/v1"));
        assert!(err.message.contains("Anthropic Messages"));
        assert!(err.message.contains("OpenAI Chat Completions"));
        assert!(err.message.contains("invalid api key"));
    }
}
