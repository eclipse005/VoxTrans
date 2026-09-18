//! Transport plumbing shared by both wire dialects.
//!
//! Everything here is protocol-neutral: building error values from a
//! response, reading `Retry-After`, joining the model-list URL, and driving
//! an SSE byte stream into frames. The dialect-specific modules
//! (`anthropic.rs`, `openai.rs`) own their own request bodies and frame
//! semantics and only borrow these helpers.

use futures_util::StreamExt;
use serde_json::Value;

use super::base_url::normalize_base_url;
use super::error::{LlmError, LlmErrorKind};
use super::max_tokens::{halve_max_tokens, is_max_tokens_rejection, record_max_tokens_override};
use super::port::LlmConfig;
use super::text::truncate_chars;

/// reqwest's Display stops at "error sending request for url (...)" and hides
/// the actual root cause — dns failure, connection refused, TLS, timeout —
/// deeper in the `source()` chain. That root cause is exactly what tells a
/// local network blip from a dead endpoint, so append it to the message.
pub(super) fn transport_error_message(err: &reqwest::Error) -> String {
    let mut message = format!("http request failed: {err}");
    let mut source = std::error::Error::source(err);
    let mut root = None;
    while let Some(cause) = source {
        root = Some(cause.to_string());
        source = cause.source();
    }
    if let Some(cause) = root {
        message.push_str(&format!(" [{cause}]"));
    }
    message
}

/// Server-advised wait (`Retry-After`, seconds → ms) when the response sent
/// one. Shared so both dialects honor it identically.
pub(super) fn retry_after_ms(response: &reqwest::Response) -> Option<u64> {
    response
        .headers()
        .get("retry-after")
        .and_then(|value| value.to_str().ok())
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .map(|secs| secs.saturating_mul(1000))
}

/// Build an [`LlmError`] from a non-2xx response body.
///
/// Both dialects wrap failures in an `{error:{message}}` envelope (Anthropic
/// as `{type:"error", error:{...}}`, OpenAI as `{error:{message,type}}`), so
/// one parser covers them; anything else falls back to the truncated raw
/// body. The status code and Retry-After override ride along for the retry
/// loop's classification.
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

/// Send a fully-built request and normalize the response: transport failures
/// become an `Http` error carrying the root cause; a non-2xx status becomes an
/// `api_error` with the server's `Retry-After` (if any). Both wire dialects
/// share this tail — they differ only in the endpoint and auth headers they
/// bake into the builder before handing it here.
pub(super) async fn send_and_check(
    request: reqwest::RequestBuilder,
) -> Result<reqwest::Response, LlmError> {
    let response = request
        .send()
        .await
        .map_err(|err| LlmError::new(LlmErrorKind::Http, transport_error_message(&err)))?;
    let status = response.status();
    if !status.is_success() {
        let retry_after = retry_after_ms(&response);
        let text = response.text().await.unwrap_or_default();
        return Err(api_error(status.as_u16(), retry_after, &text));
    }
    Ok(response)
}

/// The output-cap degrade decision, shared by both dialects' retry loops. A
/// `max_tokens` rejection is deterministic (not transient), so the budget
/// halves and the caller rebuilds the request immediately — no backoff. The
/// lowered value is memoized per (base_url, model), so the halving is paid once
/// per endpoint+model, ever. Returns the new budget to retry with, or `None` to
/// stop — either the error isn't a `max_tokens` rejection or the budget is
/// already at the floor.
pub(super) fn degrade_max_tokens_after_rejection(
    err: &LlmError,
    config: &LlmConfig,
    current: u32,
) -> Option<u32> {
    if !is_max_tokens_rejection(err) {
        return None;
    }
    let lowered = halve_max_tokens(current)?;
    eprintln!("[warn] endpoint rejected max_tokens={current} ({err}); retrying with {lowered}");
    record_max_tokens_override(&config.base_url, &config.model, lowered);
    Some(lowered)
}

/// `GET {base}/v1/models` for the settings model picker. Both dialects serve
/// the same route (OpenAI's native list, and the one every Messages-compatible
/// vendor also exposes), so the join rule is shared and a base ending in
/// `/v1` never doubles up.
pub fn models_endpoint(base_url: &str) -> String {
    let normalized = normalize_base_url(base_url);
    if normalized.ends_with("/v1") {
        format!("{normalized}/models")
    } else {
        format!("{normalized}/v1/models")
    }
}

/// Read a token-count field that vendors spell as an integer, a float, or a
/// string depending on the gateway in front of them.
pub(super) fn json_u64(value: Option<&Value>) -> Option<u64> {
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

/// Accumulates SSE lines into frames (`event:` … `data:` … blank line).
///
/// Both dialects use the same framing; OpenAI simply omits the `event:` line
/// and carries its type inside the JSON, which is why callers get `event` as
/// an `Option`.
pub(super) struct SseFrameParser {
    event: Option<String>,
    data: String,
}

impl SseFrameParser {
    pub(super) fn new() -> Self {
        Self {
            event: None,
            data: String::new(),
        }
    }

    /// Feed one complete line; returns `(event, data)` when a frame closes on
    /// the blank separator line.
    pub(super) fn feed_line(&mut self, line: &str) -> Option<(Option<String>, String)> {
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

/// Pull complete SSE frames out of `response`, handing each to `apply`.
///
/// `apply` returns `Ok(true)` to stop early — each dialect has its own
/// terminator (`message_stop` / `[DONE]`). A trailing partial line at EOF
/// still counts as one final line, so a stream that ends without the blank
/// separator does not silently drop its last frame.
pub(super) async fn drive_sse<F>(response: reqwest::Response, mut apply: F) -> Result<(), LlmError>
where
    F: FnMut(Option<&str>, &str) -> Result<bool, LlmError>,
{
    let mut byte_stream = response.bytes_stream();
    let mut line_buf = String::new();
    let mut frames = SseFrameParser::new();

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
                        apply(event.as_deref(), &data)?;
                    }
                    break;
                }
            }
        };
        if let Some((event, data)) = frames.feed_line(&line)
            && apply(event.as_deref(), &data)?
        {
            break;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn models_endpoint_never_doubles_v1() {
        assert_eq!(
            models_endpoint("https://api.anthropic.com"),
            "https://api.anthropic.com/v1/models"
        );
        assert_eq!(
            models_endpoint("https://x.example.com/v1"),
            "https://x.example.com/v1/models"
        );
        assert_eq!(
            models_endpoint("https://x.example.com/v1/"),
            "https://x.example.com/v1/models"
        );
    }

    #[test]
    fn api_error_reads_both_error_envelopes() {
        let anthropic = api_error(
            400,
            None,
            r#"{"type":"error","error":{"type":"invalid_request_error","message":"bad model"}}"#,
        );
        assert_eq!(anthropic.message, "http status 400: bad model");
        assert_eq!(anthropic.status, Some(400));

        let openai = api_error(401, Some(1500), r#"{"error":{"message":"invalid api key"}}"#);
        assert_eq!(openai.message, "http status 401: invalid api key");
        assert_eq!(openai.retry_after_ms, Some(1500));
    }

    #[test]
    fn api_error_falls_back_to_the_raw_body() {
        let plain = api_error(502, None, "  <html>upstream exploded</html>  ");
        assert_eq!(plain.message, "http status 502: <html>upstream exploded</html>");
        assert_eq!(plain.status, Some(502));
    }

    #[test]
    fn sse_parser_closes_frames_on_the_blank_line() {
        let mut parser = SseFrameParser::new();
        assert_eq!(parser.feed_line("event: message_start"), None);
        assert_eq!(parser.feed_line(r#"data: {"a":1}"#), None);
        let (event, data) = parser.feed_line("").expect("frame closes");
        assert_eq!(event.as_deref(), Some("message_start"));
        assert_eq!(data, r#"{"a":1}"#);

        // OpenAI shape: no event line at all.
        assert_eq!(parser.feed_line(r#"data: {"b":2}"#), None);
        let (event, data) = parser.feed_line("").expect("frame closes");
        assert_eq!(event, None);
        assert_eq!(data, r#"{"b":2}"#);
    }

    #[test]
    fn sse_parser_ignores_comments_and_empty_keepalives() {
        let mut parser = SseFrameParser::new();
        assert_eq!(parser.feed_line(": keep-alive"), None);
        assert_eq!(parser.feed_line(""), None);
        assert_eq!(parser.feed_line("event: ping"), None);
        assert_eq!(parser.feed_line("data: {}"), None);
        let (event, data) = parser.feed_line("").expect("frame closes");
        assert_eq!(event.as_deref(), Some("ping"));
        assert_eq!(data, "{}");
    }

    #[test]
    fn json_u64_accepts_every_spelling_vendors_use() {
        assert_eq!(json_u64(Some(&json!(12))), Some(12));
        assert_eq!(json_u64(Some(&json!(-3))), Some(0));
        assert_eq!(json_u64(Some(&json!(7.0))), Some(7));
        assert_eq!(json_u64(Some(&json!("42"))), Some(42));
        assert_eq!(json_u64(Some(&json!(null))), None);
        assert_eq!(json_u64(Some(&json!("x"))), None);
        assert_eq!(json_u64(None), None);
    }
}
