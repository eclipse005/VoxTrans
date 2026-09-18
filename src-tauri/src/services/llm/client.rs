use async_trait::async_trait;
use serde_json::{Value, json};
use std::collections::HashSet;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use tokio::time::sleep;

use crate::db::store::TaskStore;
use crate::services::task_log::TaskLogger;
use crate::services::task_usage::{LlmTokenUsage as TaskUsage, record_llm_usage_best_effort};

use super::anthropic::{call_message, call_message_stream, call_message_tools};
use super::base_url::normalize_base_url;
use super::error::{LlmError, LlmErrorKind};
use super::event_payload::{
    attempt_base_payload, failed_attempt_payload, http_error_attempt_payload,
    invalid_semantic_attempt_payload, log_llm_call, logger_for_context, success_attempt_payload,
};
use super::json_guard::{JsonResponseValidator, extract_and_repair_json_with_outcome};
use super::openai::{
    call_chat_completion, call_chat_completion_stream, call_chat_completion_tools,
};
use super::port::{
    AssistantTurn, ChatMessage, LlmCallContext, LlmConfig, LlmJsonResult, LlmPort, LlmTokenUsage,
};
use super::protocol::{
    LlmProtocol, both_protocols_rejected, is_protocol_mismatch, remember_protocol, resolve_protocol,
};
use super::retry::{
    RetryFeedback, augment_user_prompt_with_retry_feedback, effective_backoff_ms,
    feedback_from_llm_error, retry_backoff_ms,
};

/// One raw Messages call without retries or JSON validation, plus the
/// per-attempt hooks the retry loop delegates to. This is the seam unit
/// tests use: production goes through [`HttpCallOnce`], tests script
/// responses, backoff and usage recording through a fake.
#[async_trait]
trait LlmCallOnce: Send + Sync {
    async fn call_once(
        &self,
        http: &reqwest::Client,
        config: &LlmConfig,
        logger: Option<&TaskLogger>,
        user_prompt: &str,
        on_partial: Option<&(dyn Fn(String) + Send + Sync)>,
        output_schema: Option<&Value>,
    ) -> Result<(String, LlmTokenUsage), LlmError>;

    /// Delay (ms) before the next attempt, or `None` to proceed
    /// immediately — the last attempt, or no backoff wanted.
    fn backoff_ms(&self, attempt: u32, max_attempts: u32) -> Option<u64> {
        retry_backoff_ms(attempt, max_attempts)
    }

    /// Best-effort token accounting for one completed response. Called at
    /// the moment a response arrives — before validation — so failed
    /// retry rounds still count, exactly once per response.
    fn record_usage(
        &self,
        task_id: &str,
        phase: &str,
        usage: TaskUsage,
        store: Option<TaskStore>,
    ) {
        record_llm_usage_best_effort(task_id, phase, usage, store);
    }
}

/// Endpoints that already rejected structured output once — remembered per
/// (protocol, normalized base_url) so later batches skip the schema instead of
/// eating a 400 every round (project's existing global-mutex pattern). The
/// protocol is part of the key because one host can serve both dialects with
/// different capabilities behind each.
static STRUCTURED_OUTPUT_UNSUPPORTED: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

fn structured_output_key(protocol: LlmProtocol, base_url: &str) -> String {
    format!("{}\u{1}{}", protocol.provider_id(), normalize_base_url(base_url))
}

fn is_structured_output_unsupported(protocol: LlmProtocol, base_url: &str) -> bool {
    let key = structured_output_key(protocol, base_url);
    STRUCTURED_OUTPUT_UNSUPPORTED
        .get_or_init(|| Mutex::new(HashSet::new()))
        .lock()
        .map(|set| set.contains(&key))
        .unwrap_or(false)
}

fn mark_structured_output_unsupported(protocol: LlmProtocol, base_url: &str) {
    let key = structured_output_key(protocol, base_url);
    if let Ok(mut set) = STRUCTURED_OUTPUT_UNSUPPORTED
        .get_or_init(|| Mutex::new(HashSet::new()))
        .lock()
    {
        set.insert(key);
    }
}

/// Does this error mean "endpoint does not support structured output"? Only
/// schema-related 400/422s trigger the fallback; unrelated bad requests keep
/// failing loudly.
fn is_structured_output_rejection(err: &LlmError) -> bool {
    if !matches!(err.status, Some(400) | Some(422)) {
        return false;
    }
    let message = err.message.to_ascii_lowercase();
    let named = message.contains("output_config")
        || message.contains("json_schema")
        || message.contains("structured output")
        // Constrained-decoding backends (vLLM/xgrammar family) surface schema
        // trouble as grammar errors, e.g. SenseNova's `guided_grammar …
        // compile_grammar_error: No module named 'xgrammar'`.
        || message.contains("grammar")
        || message.contains("guided")
        // SenseNova rejects ANY `output_config` with a generic 400 body
        // "inference request is invalid" — verified 2026-09 against
        // token.sensenova.cn: flat and nested schemas both 400 while the
        // identical request without output_config returns 200. Treating this
        // as a structured-output rejection is safe: the fallback retries
        // without the schema, and a genuinely broken request fails again
        // either way (the memo then only skips a schema the endpoint hates).
        || message.contains("inference request");
    let format_shaped = message.contains("format")
        && ["unsupported", "not supported", "unknown", "unexpected", "invalid"]
            .iter()
            .any(|word| message.contains(word));
    named || format_shaped
}

/// One dialect's wire calls, behind a seam so the protocol reroute can be
/// scripted without a network. Production pairs [`AnthropicDialect`] with
/// [`OpenAiDialect`]; tests substitute fakes and assert the reroute.
#[async_trait]
trait Dialect: Send + Sync {
    async fn call(
        &self,
        http: &reqwest::Client,
        config: &LlmConfig,
        user_prompt: &str,
        output_schema: Option<&Value>,
    ) -> Result<(String, LlmTokenUsage), LlmError>;

    async fn call_stream<'a>(
        &'a self,
        http: &'a reqwest::Client,
        config: &'a LlmConfig,
        user_prompt: &'a str,
        output_schema: Option<&'a Value>,
        on_delta: Option<&'a mut (dyn for<'b> FnMut(&'b str) + Send + 'a)>,
    ) -> Result<(String, LlmTokenUsage), LlmError>;

    async fn call_tools(
        &self,
        http: &reqwest::Client,
        config: &LlmConfig,
        messages: &[ChatMessage],
        tools: &[Value],
        temperature: Option<f64>,
    ) -> Result<AssistantTurn, LlmError>;
}

struct AnthropicDialect;

#[async_trait]
impl Dialect for AnthropicDialect {
    async fn call(
        &self,
        http: &reqwest::Client,
        config: &LlmConfig,
        user_prompt: &str,
        output_schema: Option<&Value>,
    ) -> Result<(String, LlmTokenUsage), LlmError> {
        call_message(http, config, user_prompt, output_schema).await
    }

    async fn call_stream<'a>(
        &'a self,
        http: &'a reqwest::Client,
        config: &'a LlmConfig,
        user_prompt: &'a str,
        output_schema: Option<&'a Value>,
        on_delta: Option<&'a mut (dyn for<'b> FnMut(&'b str) + Send + 'a)>,
    ) -> Result<(String, LlmTokenUsage), LlmError> {
        call_message_stream(http, config, user_prompt, output_schema, on_delta).await
    }

    async fn call_tools(
        &self,
        http: &reqwest::Client,
        config: &LlmConfig,
        messages: &[ChatMessage],
        tools: &[Value],
        temperature: Option<f64>,
    ) -> Result<AssistantTurn, LlmError> {
        call_message_tools(http, config, messages, tools, temperature).await
    }
}

struct OpenAiDialect;

#[async_trait]
impl Dialect for OpenAiDialect {
    async fn call(
        &self,
        http: &reqwest::Client,
        config: &LlmConfig,
        user_prompt: &str,
        output_schema: Option<&Value>,
    ) -> Result<(String, LlmTokenUsage), LlmError> {
        call_chat_completion(http, config, user_prompt, output_schema).await
    }

    async fn call_stream<'a>(
        &'a self,
        http: &'a reqwest::Client,
        config: &'a LlmConfig,
        user_prompt: &'a str,
        output_schema: Option<&'a Value>,
        on_delta: Option<&'a mut (dyn for<'b> FnMut(&'b str) + Send + 'a)>,
    ) -> Result<(String, LlmTokenUsage), LlmError> {
        call_chat_completion_stream(http, config, user_prompt, output_schema, on_delta).await
    }

    async fn call_tools(
        &self,
        http: &reqwest::Client,
        config: &LlmConfig,
        messages: &[ChatMessage],
        tools: &[Value],
        temperature: Option<f64>,
    ) -> Result<AssistantTurn, LlmError> {
        call_chat_completion_tools(http, config, messages, tools, temperature).await
    }
}

/// Production transport: resolves which dialect the endpoint speaks, streams
/// when a callback is given (falling back to a plain completion when the
/// provider rejects streaming), falls back to unstructured generation when
/// the endpoint rejects the structured-output field, and reroutes once to the
/// other dialect when the endpoint says we knocked on the wrong door.
struct HttpCallOnce {
    anthropic: Arc<dyn Dialect>,
    openai: Arc<dyn Dialect>,
}

impl HttpCallOnce {
    fn new() -> Self {
        Self {
            anthropic: Arc::new(AnthropicDialect),
            openai: Arc::new(OpenAiDialect),
        }
    }

    fn dialect(&self, protocol: LlmProtocol) -> &Arc<dyn Dialect> {
        match protocol {
            LlmProtocol::AnthropicMessages => &self.anthropic,
            LlmProtocol::OpenAiChatCompletions => &self.openai,
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn attempt(
        &self,
        protocol: LlmProtocol,
        http: &reqwest::Client,
        config: &LlmConfig,
        logger: Option<&TaskLogger>,
        user_prompt: &str,
        on_partial: Option<&(dyn Fn(String) + Send + Sync)>,
        output_schema: Option<&Value>,
    ) -> Result<(String, LlmTokenUsage), LlmError> {
        let dialect = self.dialect(protocol);
        if let Some(cb) = on_partial {
            let mut delta_cb = |acc: &str| {
                cb(acc.to_string());
            };
            match dialect
                .call_stream(http, config, user_prompt, output_schema, Some(&mut delta_cb))
                .await
            {
                Ok(result) => return Ok(result),
                Err(err) => match logger {
                    // Provider may reject stream:true — fall back so translation
                    // still works. Record the fallback on the task's llm log
                    // when a logger is available.
                    Some(logger) => log_llm_call(
                        Some(logger),
                        json!({
                            "status": "stream_fallback",
                            "protocol": protocol.provider_id(),
                            "error": err.message,
                        }),
                    ),
                    None => eprintln!(
                        "[warn] {} stream failed ({err}); falling back to non-stream",
                        protocol.provider_id()
                    ),
                },
            }
        }
        dialect.call(http, config, user_prompt, output_schema).await
    }

    /// One methodical attempt on ONE dialect: the structured-output degrade
    /// lives here, so it applies identically to whichever dialect is running.
    #[allow(clippy::too_many_arguments)]
    async fn attempt_with_schema_fallback(
        &self,
        protocol: LlmProtocol,
        http: &reqwest::Client,
        config: &LlmConfig,
        logger: Option<&TaskLogger>,
        user_prompt: &str,
        on_partial: Option<&(dyn Fn(String) + Send + Sync)>,
        output_schema: Option<&Value>,
    ) -> Result<(String, LlmTokenUsage), LlmError> {
        let effective_schema = match output_schema {
            Some(schema) if !is_structured_output_unsupported(protocol, &config.base_url) => {
                Some(schema)
            }
            _ => None,
        };
        if let Some(schema) = effective_schema {
            return match self
                .attempt(protocol, http, config, logger, user_prompt, on_partial, Some(schema))
                .await
            {
                Ok(result) => Ok(result),
                Err(err) if is_structured_output_rejection(&err) => {
                    mark_structured_output_unsupported(protocol, &config.base_url);
                    match logger {
                        Some(logger) => log_llm_call(
                            Some(logger),
                            json!({
                                "status": "structured_output_fallback",
                                "protocol": protocol.provider_id(),
                                "error": err.message,
                            }),
                        ),
                        None => eprintln!(
                            "[warn] endpoint rejected structured output ({err}); falling back to plain JSON"
                        ),
                    }
                    self.attempt(protocol, http, config, logger, user_prompt, on_partial, None)
                        .await
                }
                Err(err) => Err(err),
            };
        }
        self.attempt(protocol, http, config, logger, user_prompt, on_partial, None)
            .await
    }
}

#[async_trait]
impl LlmCallOnce for HttpCallOnce {
    async fn call_once(
        &self,
        http: &reqwest::Client,
        config: &LlmConfig,
        logger: Option<&TaskLogger>,
        user_prompt: &str,
        on_partial: Option<&(dyn Fn(String) + Send + Sync)>,
        output_schema: Option<&Value>,
    ) -> Result<(String, LlmTokenUsage), LlmError> {
        let first = resolve_protocol(&config.base_url, &config.model);
        match self
            .attempt_with_schema_fallback(
                first,
                http,
                config,
                logger,
                user_prompt,
                on_partial,
                output_schema,
            )
            .await
        {
            Ok(result) => Ok(result),
            Err(err) if is_protocol_mismatch(&err, first) => {
                let second = first.other();
                match logger {
                    Some(logger) => log_llm_call(
                        Some(logger),
                        json!({
                            "status": "protocol_fallback",
                            "from": first.provider_id(),
                            "to": second.provider_id(),
                            "error": err.message,
                        }),
                    ),
                    None => eprintln!(
                        "[warn] {} rejected this endpoint ({err}); retrying as {}",
                        first.provider_id(),
                        second.provider_id()
                    ),
                }
                match self
                    .attempt_with_schema_fallback(
                        second,
                        http,
                        config,
                        logger,
                        user_prompt,
                        on_partial,
                        output_schema,
                    )
                    .await
                {
                    Ok(result) => {
                        remember_protocol(&config.base_url, &config.model, second);
                        Ok(result)
                    }
                    Err(retry_err) if is_protocol_mismatch(&retry_err, second) => {
                        Err(both_protocols_rejected(
                            &config.base_url,
                            first,
                            second,
                            &retry_err,
                        ))
                    }
                    Err(retry_err) => Err(retry_err),
                }
            }
            Err(err) => Err(err),
        }
    }
}

#[derive(Clone)]
pub struct LlmClient {
    config: LlmConfig,
    http: reqwest::Client,
    transport: Arc<dyn LlmCallOnce>,
    /// Wire seam for tool turns; the retry loop lives in the
    /// [`ToolTurnCaller`] impl below.
    tool_transport: Arc<dyn ToolTurnOnce>,
}

/// One tool-calling turn against the wire, no retries — the seam the retry
/// loop in [`LlmClient`]'s [`ToolTurnCaller`] impl and tests delegate to.
#[async_trait]
trait ToolTurnOnce: Send + Sync {
    async fn call_tools_once(
        &self,
        http: &reqwest::Client,
        config: &LlmConfig,
        messages: &[ChatMessage],
        tools: &[Value],
        temperature: Option<f64>,
    ) -> Result<AssistantTurn, LlmError>;

    /// Delay (ms) before the next attempt, or `None` to proceed
    /// immediately — the last attempt, or no backoff wanted.
    fn backoff_ms(&self, attempt: u32, max_attempts: u32) -> Option<u64> {
        retry_backoff_ms(attempt, max_attempts)
    }
}

struct HttpToolTurn {
    anthropic: Arc<dyn Dialect>,
    openai: Arc<dyn Dialect>,
}

impl HttpToolTurn {
    fn new() -> Self {
        Self {
            anthropic: Arc::new(AnthropicDialect),
            openai: Arc::new(OpenAiDialect),
        }
    }

    fn dialect(&self, protocol: LlmProtocol) -> &Arc<dyn Dialect> {
        match protocol {
            LlmProtocol::AnthropicMessages => &self.anthropic,
            LlmProtocol::OpenAiChatCompletions => &self.openai,
        }
    }
}

#[async_trait]
impl ToolTurnOnce for HttpToolTurn {
    async fn call_tools_once(
        &self,
        http: &reqwest::Client,
        config: &LlmConfig,
        messages: &[ChatMessage],
        tools: &[Value],
        temperature: Option<f64>,
    ) -> Result<AssistantTurn, LlmError> {
        // Same protocol resolution and one-shot reroute as the JSON path: an
        // agent turn must not fail just because the endpoint speaks the other
        // dialect.
        let first = resolve_protocol(&config.base_url, &config.model);
        let attempt = |protocol: LlmProtocol| async move {
            self.dialect(protocol)
                .call_tools(http, config, messages, tools, temperature)
                .await
        };
        match attempt(first).await {
            Ok(turn) => Ok(turn),
            Err(err) if is_protocol_mismatch(&err, first) => {
                let second = first.other();
                eprintln!(
                    "[warn] {} rejected this endpoint for the agent turn ({err}); retrying as {}",
                    first.provider_id(),
                    second.provider_id()
                );
                match attempt(second).await {
                    Ok(turn) => {
                        remember_protocol(&config.base_url, &config.model, second);
                        Ok(turn)
                    }
                    Err(retry_err) if is_protocol_mismatch(&retry_err, second) => {
                        Err(both_protocols_rejected(
                            &config.base_url,
                            first,
                            second,
                            &retry_err,
                        ))
                    }
                    Err(retry_err) => Err(retry_err),
                }
            }
            Err(err) => Err(err),
        }
    }
}

/// The agent-facing tool-calling port (kimi-code requester contract): the
/// caller gets ONE logical turn; retryable failures — 408/409/429/5xx,
/// transport errors, and empty responses — back off and re-request inside
/// the port instead of consuming an agent round. Non-retryable statuses
/// (400 bad request, 401/403 auth, 404 model) fail fast. Server-advised
/// `Retry-After` wins over the default backoff, capped as usual.
#[async_trait]
pub trait ToolTurnCaller: Send + Sync {
    async fn call_turn(
        &self,
        messages: &[ChatMessage],
        tools: &[Value],
        temperature: Option<f64>,
    ) -> Result<AssistantTurn, LlmError>;
}

#[async_trait]
impl ToolTurnCaller for LlmClient {
    async fn call_turn(
        &self,
        messages: &[ChatMessage],
        tools: &[Value],
        temperature: Option<f64>,
    ) -> Result<AssistantTurn, LlmError> {
        let max_attempts = self.config.max_retries.saturating_add(1).max(1);
        let mut last_err: Option<LlmError> = None;
        for attempt in 1..=max_attempts {
            match self
                .tool_transport
                .call_tools_once(&self.http, &self.config, messages, tools, temperature)
                .await
            {
                Ok(turn) => return Ok(turn),
                Err(err) => {
                    let feedback = feedback_from_llm_error(&err);
                    if !feedback.retryable {
                        return Err(err);
                    }
                    let last_attempt = attempt == max_attempts;
                    eprintln!(
                        "[terminology-llm] tools turn attempt {attempt}/{max_attempts} failed ({err}){}",
                        if last_attempt { "; giving up" } else { "; retrying" }
                    );
                    if let Some(delay) = self
                        .tool_transport
                        .backoff_ms(attempt, max_attempts)
                        .map(|default| effective_backoff_ms(default, err.retry_after_ms))
                    {
                        sleep(Duration::from_millis(delay)).await;
                    }
                    last_err = Some(err);
                }
            }
        }
        Err(last_err.unwrap_or_else(|| {
            LlmError::new(LlmErrorKind::Http, "tools turn failed with no recorded error")
        }))
    }
}

#[derive(Debug, Clone)]
pub enum LlmSemanticValidationError {
    Retryable(String),
}

impl LlmSemanticValidationError {
    pub fn retryable(message: impl Into<String>) -> Self {
        Self::Retryable(message.into())
    }
}

#[derive(Debug, Clone)]
pub struct LlmValidatedJsonResult<T> {
    pub value: T,
}

impl LlmClient {
    pub fn new(config: LlmConfig) -> Result<Self, LlmError> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .map_err(|err| {
                LlmError::new(
                    LlmErrorKind::Config,
                    format!("failed to create http client: {err}"),
                )
            })?;
        Ok(Self {
            config,
            http,
            transport: Arc::new(HttpCallOnce::new()),
            tool_transport: Arc::new(HttpToolTurn::new()),
        })
    }

    async fn call_once(
        &self,
        logger: Option<&TaskLogger>,
        context: &LlmCallContext,
        user_prompt: &str,
        on_partial: Option<&(dyn Fn(String) + Send + Sync)>,
    ) -> Result<(String, LlmTokenUsage), LlmError> {
        self.transport
            .call_once(
                &self.http,
                &self.config,
                logger,
                user_prompt,
                on_partial,
                context.output_schema.as_ref(),
            )
            .await
    }

    /// Shared failure bookkeeping for one retry attempt: fold `err` into the
    /// loop state (`last_error` / `last_feedback`) and compute the inter-attempt
    /// backoff. A server `Retry-After` wins over the default step but is capped
    /// (see `effective_backoff_ms`); JSON/schema/semantic errors carry none, so
    /// every failure source shares one exponential sequence. Returns the derived
    /// feedback plus the backoff — `None` on the final attempt or a non-retryable
    /// failure, which is how callers decide `break` vs `sleep`.
    fn register_failure(
        &self,
        last_error: &mut String,
        last_feedback: &mut Option<RetryFeedback>,
        err: &LlmError,
        attempt: u32,
        max_attempts: u32,
    ) -> (RetryFeedback, Option<u64>) {
        let feedback = feedback_from_llm_error(err);
        let backoff_ms = feedback
            .retryable
            .then(|| {
                self.transport
                    .backoff_ms(attempt, max_attempts)
                    .map(|default| effective_backoff_ms(default, err.retry_after_ms))
            })
            .flatten();
        *last_error = feedback.detail.clone();
        *last_feedback = Some(feedback.clone());
        (feedback, backoff_ms)
    }

    /// JSON-validated call for translation batches (retry loop inside).
    pub async fn call_json_validated<T, F>(
        &self,
        context: &LlmCallContext,
        request_id: &str,
        user_prompt: &str,
        response_validator: Option<&JsonResponseValidator>,
        semantic_validate: F,
    ) -> Result<LlmValidatedJsonResult<T>, LlmError>
    where
        F: Fn(Value) -> Result<T, LlmSemanticValidationError>,
    {
        self.call_json_validated_inner(
            context,
            request_id,
            user_prompt,
            response_validator,
            None,
            semantic_validate,
        )
        .await
    }

    /// Like [`Self::call_json_validated`], but streams tokens and invokes
    /// `on_partial` with the accumulated raw assistant text (caller should
    /// throttle UI side-effects).
    pub async fn call_json_validated_streaming<T, F>(
        &self,
        context: &LlmCallContext,
        request_id: &str,
        user_prompt: &str,
        response_validator: Option<&JsonResponseValidator>,
        on_partial: Arc<dyn Fn(String) + Send + Sync>,
        semantic_validate: F,
    ) -> Result<LlmValidatedJsonResult<T>, LlmError>
    where
        F: Fn(Value) -> Result<T, LlmSemanticValidationError>,
    {
        self.call_json_validated_inner(
            context,
            request_id,
            user_prompt,
            response_validator,
            Some(on_partial),
            semantic_validate,
        )
        .await
    }

    async fn call_json_validated_inner<T, F>(
        &self,
        context: &LlmCallContext,
        request_id: &str,
        user_prompt: &str,
        response_validator: Option<&JsonResponseValidator>,
        on_partial: Option<Arc<dyn Fn(String) + Send + Sync>>,
        semantic_validate: F,
    ) -> Result<LlmValidatedJsonResult<T>, LlmError>
    where
        F: Fn(Value) -> Result<T, LlmSemanticValidationError>,
    {
        let logger = logger_for_context(context);

        let max_attempts = self.config.max_retries.saturating_add(1).max(1);
        let started = std::time::Instant::now();
        let mut last_error = String::new();
        let mut last_feedback: Option<RetryFeedback> = None;
        let mut attempts_made = 0u32;

        // JSON/schema/semantic failures retry with the compacted failure
        // hint appended to the prompt. Every retryable failure — HTTP and
        // JSON/schema/semantic alike — shares the same backoff sequence
        // between attempts, overridden by the server's Retry-After when it
        // sent one. Whatever fails on the final attempt exits the loop and
        // is reported as `last_error` in the error below.
        for attempt in 1..=max_attempts {
            attempts_made = attempt;
            let effective_user_prompt = augment_user_prompt_with_retry_feedback(
                user_prompt,
                attempt,
                last_feedback.as_ref(),
            );
            let base_payload = attempt_base_payload(
                &self.config,
                context,
                request_id,
                attempt,
                max_attempts,
                &effective_user_prompt,
            );

            let partial_ref = on_partial.as_ref().map(|a| a.as_ref());
            match self
                .call_once(logger.as_ref(), context, &effective_user_prompt, partial_ref)
                .await
            {
                Ok((raw_text, usage)) => {
                    // Count tokens the moment a response arrives — before
                    // validation — so failed retry rounds are still recorded,
                    // exactly once per response.
                    self.transport.record_usage(
                        &context.task_id,
                        &context.phase,
                        TaskUsage {
                            prompt_tokens: usage.prompt_tokens,
                            completion_tokens: usage.completion_tokens,
                            total_tokens: usage.total_tokens,
                        },
                        context.store.clone(),
                    );
                    let parsed = match extract_and_repair_json_with_outcome(&raw_text) {
                        Ok(v) => v,
                        Err(err) => {
                            let (feedback, backoff_ms) = self.register_failure(
                                &mut last_error,
                                &mut last_feedback,
                                &err,
                                attempt,
                                max_attempts,
                            );
                            log_llm_call(
                                logger.as_ref(),
                                failed_attempt_payload(
                                    &base_payload,
                                    "invalid_json",
                                    &last_error,
                                    feedback.error_kind.as_str(),
                                    feedback.retryable,
                                    feedback.retry_hint.as_deref(),
                                    Some(&raw_text),
                                    backoff_ms,
                                ),
                            );
                            if !feedback.retryable {
                                break;
                            }
                            if let Some(delay) = backoff_ms {
                                sleep(Duration::from_millis(delay)).await;
                            }
                            continue;
                        }
                    };

                    if let Some(validator) = response_validator
                        && let Err(err) = validator.validate(&parsed.value)
                    {
                        let (feedback, backoff_ms) = self.register_failure(
                            &mut last_error,
                            &mut last_feedback,
                            &err,
                            attempt,
                            max_attempts,
                        );
                        log_llm_call(
                            logger.as_ref(),
                            failed_attempt_payload(
                                &base_payload,
                                "invalid_schema",
                                &last_error,
                                feedback.error_kind.as_str(),
                                feedback.retryable,
                                feedback.retry_hint.as_deref(),
                                Some(&raw_text),
                                backoff_ms,
                            ),
                        );
                        if !feedback.retryable {
                            break;
                        }
                        if let Some(delay) = backoff_ms {
                            sleep(Duration::from_millis(delay)).await;
                        }
                        continue;
                    }

                    match semantic_validate(parsed.value.clone()) {
                        Ok(value) => {
                            let elapsed_ms = started.elapsed().as_millis();
                            log_llm_call(
                                logger.as_ref(),
                                success_attempt_payload(
                                    &base_payload,
                                    parsed.source.as_str(),
                                    &raw_text,
                                    elapsed_ms,
                                    &usage,
                                ),
                            );
                            return Ok(LlmValidatedJsonResult { value });
                        }
                        Err(LlmSemanticValidationError::Retryable(message)) => {
                            let err = LlmError::new(LlmErrorKind::InvalidSemantic, message);
                            let (feedback, backoff_ms) = self.register_failure(
                                &mut last_error,
                                &mut last_feedback,
                                &err,
                                attempt,
                                max_attempts,
                            );
                            log_llm_call(
                                logger.as_ref(),
                                invalid_semantic_attempt_payload(
                                    &base_payload,
                                    &last_error,
                                    feedback.error_kind.as_str(),
                                    feedback.retryable,
                                    feedback.retry_hint.as_deref(),
                                    &raw_text,
                                    backoff_ms,
                                ),
                            );
                            if !feedback.retryable {
                                break;
                            }
                            if let Some(delay) = backoff_ms {
                                sleep(Duration::from_millis(delay)).await;
                            }
                            continue;
                        }
                    }
                }
                Err(err) => {
                    let (feedback, backoff_ms) = self.register_failure(
                        &mut last_error,
                        &mut last_feedback,
                        &err,
                        attempt,
                        max_attempts,
                    );
                    log_llm_call(
                        logger.as_ref(),
                        http_error_attempt_payload(
                            &base_payload,
                            &last_error,
                            feedback.error_kind.as_str(),
                            feedback.retryable,
                            feedback.retry_hint.as_deref(),
                            backoff_ms,
                        ),
                    );
                    if !feedback.retryable {
                        break;
                    }
                    if let Some(delay) = backoff_ms {
                        sleep(Duration::from_millis(delay)).await;
                    }
                }
            }
        }

        // `detail` is the compacted failure hint whenever one exists, so a
        // separate retry_hint suffix in the message would be redundant —
        // report the kind and the last failure only.
        let error_kind = last_feedback
            .as_ref()
            .map(|feedback| feedback.error_kind)
            .unwrap_or(LlmErrorKind::InvalidSemantic);

        Err(LlmError::new(
            error_kind,
            format!(
                "llm call failed after {} attempts: kind={}; last_error={}",
                attempts_made,
                error_kind.as_str(),
                last_error,
            ),
        ))
    }
}

impl LlmPort for LlmClient {
    async fn call_json(
        &self,
        context: &LlmCallContext,
        request_id: &str,
        user_prompt: &str,
        response_validator: Option<&JsonResponseValidator>,
    ) -> Result<LlmJsonResult, LlmError> {
        let result = self
            .call_json_validated(
                context,
                request_id,
                user_prompt,
                response_validator,
                Ok::<Value, LlmSemanticValidationError>,
            )
            .await?;
        Ok(LlmJsonResult { json: result.value })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::Instant;

    /// Fake transport: plays back a scripted response sequence, records the
    /// prompts it received and the usage it was asked to record, and lets
    /// tests override the backoff schedule.
    struct ScriptedTransport {
        script: Mutex<VecDeque<Result<(String, LlmTokenUsage), LlmError>>>,
        prompts: Mutex<Vec<String>>,
        recorded: Mutex<Vec<TaskUsage>>,
        backoff: fn(u32, u32) -> Option<u64>,
    }

    impl ScriptedTransport {
        fn new(script: Vec<Result<(String, LlmTokenUsage), LlmError>>) -> Self {
            Self {
                script: Mutex::new(script.into()),
                prompts: Mutex::new(Vec::new()),
                recorded: Mutex::new(Vec::new()),
                backoff: |_, _| Some(1),
            }
        }

        fn with_backoff(mut self, backoff: fn(u32, u32) -> Option<u64>) -> Self {
            self.backoff = backoff;
            self
        }

        fn prompt_history(&self) -> Vec<String> {
            self.prompts.lock().unwrap().clone()
        }

        fn recorded_usage(&self) -> Vec<TaskUsage> {
            self.recorded.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl LlmCallOnce for ScriptedTransport {
        async fn call_once(
            &self,
            _http: &reqwest::Client,
            _config: &LlmConfig,
            _logger: Option<&TaskLogger>,
            user_prompt: &str,
            _on_partial: Option<&(dyn Fn(String) + Send + Sync)>,
            _output_schema: Option<&Value>,
        ) -> Result<(String, LlmTokenUsage), LlmError> {
            self.prompts.lock().unwrap().push(user_prompt.to_string());
            self.script
                .lock()
                .unwrap()
                .pop_front()
                // A script that runs dry fails loudly instead of hanging.
                .unwrap_or_else(|| Err(LlmError::new(LlmErrorKind::Http, "script exhausted")))
        }

        fn backoff_ms(&self, attempt: u32, max_attempts: u32) -> Option<u64> {
            (self.backoff)(attempt, max_attempts)
        }

        fn record_usage(
            &self,
            _task_id: &str,
            _phase: &str,
            usage: TaskUsage,
            _store: Option<TaskStore>,
        ) {
            self.recorded.lock().unwrap().push(usage);
        }
    }

    fn ok_response(
        text: &str,
        prompt_tokens: u64,
        completion_tokens: u64,
    ) -> Result<(String, LlmTokenUsage), LlmError> {
        Ok((
            text.to_string(),
            LlmTokenUsage {
                prompt_tokens,
                completion_tokens,
                total_tokens: prompt_tokens + completion_tokens,
                ..LlmTokenUsage::default()
            },
        ))
    }

    fn test_client(transport: Arc<dyn LlmCallOnce>, max_retries: u32) -> LlmClient {
        let mut config =
            LlmConfig::new("http://test.local".into(), "test-key".into(), "test-model".into());
        config.max_retries = max_retries;
        LlmClient {
            config,
            http: reqwest::Client::new(),
            transport,
            tool_transport: Arc::new(HttpToolTurn::new()),
        }
    }

    /// `phase: "connectivity_test"` makes `logger_for_context` return None,
    /// so no task log file is written during tests.
    fn test_context() -> LlmCallContext {
        LlmCallContext {
            task_id: "test-task".to_string(),
            media_path: None,
            phase: "connectivity_test".to_string(),
            store: None,
            output_schema: None,
        }
    }

    #[tokio::test]
    async fn config_failure_gives_up_after_first_attempt_without_backoff() {
        let transport = Arc::new(
            ScriptedTransport::new(vec![Err(LlmError::new(
                LlmErrorKind::Config,
                "bad endpoint",
            ))])
            .with_backoff(|_, _| Some(3_000)),
        );
        let client = test_client(transport.clone(), 3);
        let started = Instant::now();
        let err = client
            .call_json_validated::<Value, _>(&test_context(), "req-1", "BASE", None, |v: Value| {
                Ok(v)
            })
            .await
            .expect_err("config failure must not succeed");
        let elapsed = started.elapsed();
        assert!(
            elapsed.as_millis() < 1_000,
            "config failures must not back off, took {elapsed:?}"
        );
        assert_eq!(transport.prompt_history(), vec!["BASE".to_string()]);
        assert!(
            err.message.contains("after 1 attempts"),
            "unexpected message: {}",
            err.message
        );
        assert!(err.message.contains("kind=config"), "{}", err.message);
        assert!(
            err.message.contains("last_error=bad endpoint"),
            "{}",
            err.message
        );
    }

    #[tokio::test]
    async fn semantic_failure_retries_with_hint_until_success() {
        let script = vec![
            ok_response(r#"{"id":1}"#, 10, 5),
            ok_response(r#"{"id":2}"#, 10, 5),
            ok_response(r#"{"id":7}"#, 10, 5),
        ];
        let transport = Arc::new(ScriptedTransport::new(script));
        let client = test_client(transport.clone(), 3);
        let calls = AtomicU32::new(0);
        let result = client
            .call_json_validated::<i64, _>(
                &test_context(),
                "req-2",
                "BASE",
                None,
                |value: Value| {
                    let n = calls.fetch_add(1, Ordering::SeqCst);
                    if n < 2 {
                        return Err(LlmSemanticValidationError::retryable("missing ids [2]"));
                    }
                    value
                        .get("id")
                        .and_then(|v| v.as_i64())
                        .ok_or_else(|| LlmSemanticValidationError::retryable("id missing"))
                },
            )
            .await
            .expect("third attempt must succeed");
        assert_eq!(result.value, 7);
        let prompts = transport.prompt_history();
        assert_eq!(prompts.len(), 3);
        assert_eq!(prompts[0], "BASE");
        for prompt in &prompts[1..] {
            assert!(prompt.contains("[RETRY FEEDBACK]"), "{prompt}");
            assert!(prompt.contains("missing ids [2]"), "{prompt}");
        }
        let recorded = transport.recorded_usage();
        assert_eq!(recorded.len(), 3, "failed retry rounds must also count");
        assert_eq!(recorded[0].total_tokens, 15);
        assert_eq!(recorded[2].total_tokens, 15);
    }

    #[tokio::test]
    async fn exhausted_retries_report_attempts_and_last_error() {
        let script = (0..4)
            .map(|_| ok_response(r#"{"nope":1}"#, 10, 5))
            .collect();
        let transport = Arc::new(ScriptedTransport::new(script));
        let client = test_client(transport.clone(), 3);
        let err = client
            .call_json_validated::<i64, _>(
                &test_context(),
                "req-3",
                "BASE",
                None,
                |value: Value| {
                    value
                        .get("id")
                        .and_then(|v| v.as_i64())
                        .ok_or_else(|| LlmSemanticValidationError::retryable("still wrong"))
                },
            )
            .await
            .expect_err("all semantic attempts must fail");
        assert_eq!(transport.prompt_history().len(), 4);
        assert_eq!(
            transport.recorded_usage().len(),
            4,
            "every response must be recorded even when all rounds fail"
        );
        assert!(
            err.message.contains("after 4 attempts"),
            "unexpected message: {}",
            err.message
        );
        assert!(
            err.message.contains("kind=invalid_semantic"),
            "unexpected message: {}",
            err.message
        );
        assert!(
            err.message.contains("last_error=still wrong"),
            "{}",
            err.message
        );
    }

    #[tokio::test]
    async fn usage_recorded_exactly_once_per_response_including_failed_rounds() {
        let script = vec![
            ok_response("this is not json {{{", 10, 5),
            ok_response(r#"{"ok":true}"#, 20, 8),
        ];
        let transport = Arc::new(ScriptedTransport::new(script));
        let client = test_client(transport.clone(), 3);
        let result = client
            .call_json_validated::<Value, _>(&test_context(), "req-4", "BASE", None, |v: Value| {
                Ok(v)
            })
            .await
            .expect("second attempt must succeed");
        assert_eq!(result.value, serde_json::json!({"ok": true}));
        assert_eq!(
            transport.recorded_usage(),
            vec![
                TaskUsage {
                    prompt_tokens: 10,
                    completion_tokens: 5,
                    total_tokens: 15,
                },
                TaskUsage {
                    prompt_tokens: 20,
                    completion_tokens: 8,
                    total_tokens: 28,
                },
            ],
            "each response records usage exactly once, including the invalid-json round"
        );
        let prompts = transport.prompt_history();
        assert_eq!(prompts.len(), 2);
        assert!(prompts[1].contains("[RETRY FEEDBACK]"), "{}", prompts[1]);
    }

    #[tokio::test]
    async fn retryable_failures_back_off_between_attempts() {
        let script = (0..4)
            .map(|_| Err(LlmError::new(LlmErrorKind::Http, "timeout")))
            .collect();
        let transport = Arc::new(
            ScriptedTransport::new(script).with_backoff(|attempt, max| {
                if attempt < max {
                    Some(80)
                } else {
                    None
                }
            }),
        );
        let client = test_client(transport.clone(), 3);
        let started = Instant::now();
        let err = client
            .call_json_validated::<Value, _>(&test_context(), "req-5", "BASE", None, |v: Value| {
                Ok(v)
            })
            .await
            .expect_err("all http attempts must fail");
        let elapsed = started.elapsed();
        assert!(
            elapsed.as_millis() >= 230,
            "expected three 80ms backoffs, took {elapsed:?}"
        );
        assert!(elapsed.as_millis() < 3_000, "took far too long: {elapsed:?}");
        assert!(
            err.message.contains("after 4 attempts"),
            "unexpected message: {}",
            err.message
        );
        assert!(err.message.contains("kind=http"), "{}", err.message);
        assert!(
            err.message.contains("last_error=timeout"),
            "{}",
            err.message
        );
        assert_eq!(transport.prompt_history().len(), 4);
    }

    #[tokio::test]
    async fn non_retryable_http_status_fails_fast() {
        let script = vec![Err(http_err(400, "invalid request"))];
        let transport = Arc::new(
            ScriptedTransport::new(script).with_backoff(|_, _| Some(3_000)),
        );
        let client = test_client(transport.clone(), 3);
        let started = Instant::now();
        client
            .call_json_validated::<Value, _>(&test_context(), "req-6", "BASE", None, |v: Value| {
                Ok(v)
            })
            .await
            .expect_err("400 must fail");
        let elapsed = started.elapsed();
        assert!(
            elapsed.as_millis() < 1_000,
            "400 must not burn the backoff budget, took {elapsed:?}"
        );
        assert_eq!(transport.prompt_history().len(), 1, "no retries after 400");
    }

    #[tokio::test]
    async fn server_retry_after_overrides_default_backoff() {
        let script = vec![
            {
                let mut err = http_err(429, "rate limited");
                err.retry_after_ms = Some(50);
                Err(err)
            },
            ok_response(r#"{"id":1}"#, 10, 5),
        ];
        // Default schedule would wait 5s; the Retry-After (50ms) wins.
        let transport =
            Arc::new(ScriptedTransport::new(script).with_backoff(|_, _| Some(5_000)));
        let client = test_client(transport.clone(), 1);
        let started = Instant::now();
        client
            .call_json_validated::<Value, _>(&test_context(), "req-7", "BASE", None, |v: Value| {
                Ok(v)
            })
            .await
            .expect("second attempt must succeed after Retry-After wait");
        let elapsed = started.elapsed();
        assert!(
            elapsed.as_millis() < 2_000,
            "Retry-After (50ms) must override the 5s default, took {elapsed:?}"
        );
        assert_eq!(transport.prompt_history().len(), 2);
    }

    #[tokio::test]
    async fn semantic_failures_share_the_same_backoff() {
        let script = vec![
            ok_response(r#"{"nope":1}"#, 10, 5),
            ok_response(r#"{"nope":1}"#, 10, 5),
        ];
        let transport = Arc::new(
            ScriptedTransport::new(script).with_backoff(|attempt, max| {
                if attempt < max {
                    Some(60)
                } else {
                    None
                }
            }),
        );
        let client = test_client(transport.clone(), 1);
        let started = Instant::now();
        let err = client
            .call_json_validated::<i64, _>(
                &test_context(),
                "req-8",
                "BASE",
                None,
                |value: Value| {
                    value
                        .get("id")
                        .and_then(|v| v.as_i64())
                        .ok_or_else(|| LlmSemanticValidationError::retryable("no id"))
                },
            )
            .await
            .expect_err("all semantic attempts must fail");
        let elapsed = started.elapsed();
        assert!(
            elapsed.as_millis() >= 50,
            "semantic failures must back off too, took {elapsed:?}"
        );
        assert!(
            err.message.contains("after 2 attempts"),
            "unexpected message: {}",
            err.message
        );
        assert_eq!(transport.recorded_usage().len(), 2);
    }

    fn http_err(status: u16, detail: &str) -> LlmError {
        let mut err = LlmError::new(LlmErrorKind::Http, format!("http status {status}: {detail}"));
        err.status = Some(status);
        err
    }

    // ── tool-turn retry (terminology agent path) ────────────────────────

    use crate::services::llm::{ToolCall, ToolCallFunction};

    /// Fake tool transport: plays back a scripted turn sequence and records
    /// the message histories it received.
    struct ScriptedToolTurn {
        script: Mutex<VecDeque<Result<AssistantTurn, LlmError>>>,
        calls: Mutex<usize>,
        backoff: fn(u32, u32) -> Option<u64>,
    }

    impl ScriptedToolTurn {
        fn new(script: Vec<Result<AssistantTurn, LlmError>>) -> Self {
            Self {
                script: Mutex::new(script.into()),
                calls: Mutex::new(0),
                backoff: |_, _| Some(1),
            }
        }

        fn with_backoff(mut self, backoff: fn(u32, u32) -> Option<u64>) -> Self {
            self.backoff = backoff;
            self
        }

        fn call_count(&self) -> usize {
            *self.calls.lock().unwrap()
        }

        fn turn(text: &str, tool_names: &[&str]) -> AssistantTurn {
            AssistantTurn {
                content: text.to_string(),
                tool_calls: tool_names
                    .iter()
                    .map(|name| ToolCall {
                        id: format!("c-{name}"),
                        type_: "function".into(),
                        function: ToolCallFunction {
                            name: (*name).to_string(),
                            arguments: "{}".into(),
                        },
                    })
                    .collect(),
                reasoning_content: None,
                usage: LlmTokenUsage::default(),
                stop_reason: None,
            }
        }
    }

    #[async_trait]
    impl ToolTurnOnce for ScriptedToolTurn {
        async fn call_tools_once(
            &self,
            _http: &reqwest::Client,
            _config: &LlmConfig,
            _messages: &[ChatMessage],
            _tools: &[Value],
            _temperature: Option<f64>,
        ) -> Result<AssistantTurn, LlmError> {
            *self.calls.lock().unwrap() += 1;
            self.script
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Err(LlmError::new(LlmErrorKind::Http, "tool script exhausted")))
        }

        fn backoff_ms(&self, attempt: u32, max_attempts: u32) -> Option<u64> {
            (self.backoff)(attempt, max_attempts)
        }
    }

    fn tool_client(tool: Arc<ScriptedToolTurn>, max_retries: u32) -> LlmClient {
        let mut config =
            LlmConfig::new("http://test.local".into(), "test-key".into(), "test-model".into());
        config.max_retries = max_retries;
        LlmClient {
            config,
            http: reqwest::Client::new(),
            transport: Arc::new(ScriptedTransport::new(vec![])),
            tool_transport: tool,
        }
    }

    #[tokio::test]
    async fn tools_turn_retries_transient_http_then_succeeds() {
        let tool = Arc::new(
            ScriptedToolTurn::new(vec![
                Err(http_err(503, "overloaded")),
                Ok(ScriptedToolTurn::turn("ok", &["count_transcript"])),
            ])
            .with_backoff(|_, _| Some(1)),
        );
        let client = tool_client(tool.clone(), 3);
        let turn = client
            .call_turn(&[ChatMessage::user("go")], &[], None)
            .await
            .expect("second attempt must succeed");
        assert_eq!(turn.tool_calls.len(), 1);
        assert_eq!(tool.call_count(), 2, "exactly one retry after the 503");
    }

    #[tokio::test]
    async fn tools_turn_treats_empty_response_as_retryable() {
        // kimi-code `empty_response` semantics: a turn with neither text nor
        // tool calls surfaces as a transport-shaped error (no status) and
        // must retry inside the port instead of killing an agent round.
        let tool = Arc::new(
            ScriptedToolTurn::new(vec![
                Err(LlmError::new(
                    LlmErrorKind::Http,
                    "response missing assistant text and tool_use",
                )),
                Ok(ScriptedToolTurn::turn("recovered", &["submit_result"])),
            ])
            .with_backoff(|_, _| Some(1)),
        );
        let client = tool_client(tool.clone(), 3);
        let turn = client
            .call_turn(&[ChatMessage::user("go")], &[], None)
            .await
            .expect("empty response must be retried, not fatal");
        assert_eq!(turn.tool_calls[0].function.name, "submit_result");
        assert_eq!(tool.call_count(), 2);
    }

    #[tokio::test]
    async fn tools_turn_fails_fast_on_bad_request() {
        let tool = Arc::new(
            ScriptedToolTurn::new(vec![Err(http_err(400, "invalid request"))])
                .with_backoff(|_, _| Some(3_000)),
        );
        let client = tool_client(tool.clone(), 3);
        let started = Instant::now();
        let err = client
            .call_turn(&[ChatMessage::user("go")], &[], None)
            .await
            .expect_err("400 must fail fast");
        assert!(
            started.elapsed().as_millis() < 1_000,
            "400 must not burn the backoff budget"
        );
        assert!(err.message.contains("400"), "{}", err.message);
        assert_eq!(tool.call_count(), 1, "no retries after a 400");
    }

    #[tokio::test]
    async fn tools_turn_reports_last_error_after_exhausting_retries() {
        let script = (0..4).map(|_| Err(http_err(503, "still down"))).collect();
        let tool = Arc::new(ScriptedToolTurn::new(script).with_backoff(|_, _| Some(1)));
        let client = tool_client(tool.clone(), 3);
        let err = client
            .call_turn(&[ChatMessage::user("go")], &[], None)
            .await
            .expect_err("all attempts must fail");
        assert!(err.message.contains("still down"), "{}", err.message);
        assert_eq!(tool.call_count(), 4, "1 initial + 3 retries");
    }

    #[test]
    fn structured_output_rejection_detection() {
        let rejection = |status: u16, msg: &str| {
            let mut err = LlmError::new(LlmErrorKind::Http, msg.to_string());
            err.status = Some(status);
            err
        };
        assert!(is_structured_output_rejection(&rejection(
            400,
            "http status 400: output_config is not supported"
        )));
        assert!(is_structured_output_rejection(&rejection(
            422,
            "http status 422: json_schema format unknown"
        )));
        assert!(is_structured_output_rejection(&rejection(
            400,
            "http status 400: format unsupported by this model"
        )));
        // Grammar-constrained decoding failures (vLLM/xgrammar family).
        assert!(is_structured_output_rejection(&rejection(
            400,
            "http status 400: guided_grammar '{\"type\":\"object\"}' has compile_grammar_error: No module named 'xgrammar'"
        )));
        // SenseNova's generic 400 for any output_config (empirically
        // verified: same request without the schema returns 200).
        assert!(is_structured_output_rejection(&rejection(
            400,
            "http status 400: inference request is invalid"
        )));
        // Unrelated 400s and non-4xx/422 statuses never trigger fallback.
        assert!(!is_structured_output_rejection(&rejection(
            400,
            "http status 400: max_tokens too large"
        )));
        assert!(!is_structured_output_rejection(&rejection(
            500,
            "http status 500: output_config exploded"
        )));
    }

    #[test]
    fn structured_output_unsupported_memo_is_per_protocol_and_base_url() {
        let probe = format!("http://memo-test.local/{}", line!());
        let messages = LlmProtocol::AnthropicMessages;
        let chat = LlmProtocol::OpenAiChatCompletions;

        assert!(!is_structured_output_unsupported(messages, &probe));
        mark_structured_output_unsupported(messages, &probe);
        assert!(is_structured_output_unsupported(messages, &probe));
        assert!(is_structured_output_unsupported(messages, &format!("{probe}/")));
        // One host can serve both dialects with different capabilities, so the
        // verdict must not leak across protocols.
        assert!(!is_structured_output_unsupported(chat, &probe));
        // Other endpoints are unaffected.
        assert!(!is_structured_output_unsupported(messages, "http://other.local"));
    }

    // ── protocol reroute ────────────────────────────────────────────────

    /// Fake dialect: a scripted reply queue plus call counters, so a test can
    /// prove which dialect was asked, and how many times.
    struct FakeDialect {
        calls: AtomicU32,
        tool_calls: AtomicU32,
        replies: Mutex<VecDeque<Result<(String, LlmTokenUsage), LlmError>>>,
        tool_reply: Result<AssistantTurn, LlmError>,
    }

    impl FakeDialect {
        fn new(replies: Vec<Result<(String, LlmTokenUsage), LlmError>>) -> Self {
            Self {
                calls: AtomicU32::new(0),
                tool_calls: AtomicU32::new(0),
                replies: Mutex::new(replies.into()),
                tool_reply: Err(LlmError::new(
                    LlmErrorKind::Config,
                    "fake dialect has no tool turn scripted",
                )),
            }
        }

        fn with_tool_reply(mut self, reply: Result<AssistantTurn, LlmError>) -> Self {
            self.tool_reply = reply;
            self
        }

        fn calls(&self) -> u32 {
            self.calls.load(Ordering::SeqCst)
        }

        fn tool_calls(&self) -> u32 {
            self.tool_calls.load(Ordering::SeqCst)
        }

        fn next_reply(&self) -> Result<(String, LlmTokenUsage), LlmError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            match self.replies.lock().unwrap().pop_front() {
                Some(reply) => reply,
                None => Err(LlmError::new(
                    LlmErrorKind::Config,
                    "fake dialect script exhausted",
                )),
            }
        }
    }

    #[async_trait]
    impl Dialect for FakeDialect {
        async fn call(
            &self,
            _http: &reqwest::Client,
            _config: &LlmConfig,
            _user_prompt: &str,
            _output_schema: Option<&Value>,
        ) -> Result<(String, LlmTokenUsage), LlmError> {
            self.next_reply()
        }

        async fn call_stream<'a>(
            &'a self,
            _http: &'a reqwest::Client,
            _config: &'a LlmConfig,
            _user_prompt: &'a str,
            _output_schema: Option<&'a Value>,
            _on_delta: Option<&'a mut (dyn for<'b> FnMut(&'b str) + Send + 'a)>,
        ) -> Result<(String, LlmTokenUsage), LlmError> {
            self.next_reply()
        }

        async fn call_tools(
            &self,
            _http: &reqwest::Client,
            _config: &LlmConfig,
            _messages: &[ChatMessage],
            _tools: &[Value],
            _temperature: Option<f64>,
        ) -> Result<AssistantTurn, LlmError> {
            self.tool_calls.fetch_add(1, Ordering::SeqCst);
            self.tool_reply.clone()
        }
    }

    /// A base URL no other test uses, with no `/anthropic` in it so the
    /// heuristic starts on Messages (today's default behavior).
    fn probe_base(tag: &str) -> String {
        format!("http://protocol-{tag}.local/{}", line!())
    }

    fn failing(status: u16, detail: &str) -> LlmError {
        let mut err = LlmError::new(LlmErrorKind::Http, format!("http status {status}: {detail}"));
        err.status = Some(status);
        err
    }

    /// Verbatim rejection from https://api.commandcode.ai/provider/v1/messages.
    fn aggregator_rejection() -> LlmError {
        failing(
            400,
            r#"Model "deepseek/deepseek-v4-flash-fast" is not supported on this endpoint. Use /provider/v1/chat/completions for OpenAI and OSS models."#,
        )
    }

    fn call_context() -> LlmCallContext {
        LlmCallContext {
            task_id: "protocol-reroute-test".to_string(),
            media_path: None,
            phase: "translate".to_string(),
            store: None,
            output_schema: None,
        }
    }

    fn json_client(base_url: &str, model: &str, transport: Arc<dyn LlmCallOnce>) -> LlmClient {
        LlmClient {
            // No retries: these tests are about routing, not backoff.
            config: LlmConfig::new(base_url.to_string(), "test-key".to_string(), model.to_string())
                .with_max_retries(0),
            http: reqwest::Client::new(),
            transport,
            tool_transport: Arc::new(HttpToolTurn::new()),
        }
    }

    fn ok_reply() -> Result<(String, LlmTokenUsage), LlmError> {
        Ok((r#"{"ok":true}"#.to_string(), LlmTokenUsage::default()))
    }

    #[tokio::test]
    async fn protocol_reroute_switches_dialect_once_then_remembers_it() {
        let base = probe_base("reroute");
        let model = "deepseek/deepseek-v4-flash-fast";
        let anthropic = Arc::new(FakeDialect::new(vec![Err(aggregator_rejection())]));
        let openai = Arc::new(FakeDialect::new(vec![ok_reply(), ok_reply()]));
        let client = json_client(
            &base,
            model,
            Arc::new(HttpCallOnce {
                anthropic: anthropic.clone(),
                openai: openai.clone(),
            }),
        );

        let first = client
            .call_json(&call_context(), "r1", "hi", None)
            .await
            .expect("the rerouted call succeeds");
        assert_eq!(first.json["ok"], json!(true));
        assert_eq!(anthropic.calls(), 1);
        assert_eq!(openai.calls(), 1);

        // The endpoint's answer is remembered for this (url, model) pair...
        assert_eq!(resolve_protocol(&base, model), LlmProtocol::OpenAiChatCompletions);

        // ...so the second call never touches the rejected dialect again.
        let second = client.call_json(&call_context(), "r2", "hi", None).await;
        assert!(second.is_ok());
        assert_eq!(anthropic.calls(), 1, "must not re-probe the rejected dialect");
        assert_eq!(openai.calls(), 2);
    }

    #[tokio::test]
    async fn a_genuine_failure_does_not_reroute() {
        let anthropic = Arc::new(FakeDialect::new(vec![Err(failing(401, "invalid api key"))]));
        let openai = Arc::new(FakeDialect::new(Vec::new()));
        let client = json_client(
            &probe_base("no-reroute"),
            "oss-model",
            Arc::new(HttpCallOnce {
                anthropic: anthropic.clone(),
                openai: openai.clone(),
            }),
        );

        let err = client
            .call_json(&call_context(), "r1", "hi", None)
            .await
            .expect_err("auth failure surfaces");
        assert!(err.message.contains("invalid api key"), "{}", err.message);
        assert_eq!(anthropic.calls(), 1);
        assert_eq!(
            openai.calls(),
            0,
            "an auth failure must not be retried on the other dialect"
        );
    }

    #[tokio::test]
    async fn both_dialects_rejected_yields_one_readable_error() {
        let base = probe_base("both-rejected");
        let anthropic = Arc::new(FakeDialect::new(vec![Err(aggregator_rejection())]));
        let openai = Arc::new(FakeDialect::new(vec![Err(aggregator_rejection())]));
        let client = json_client(
            &base,
            "oss-model",
            Arc::new(HttpCallOnce {
                anthropic: anthropic.clone(),
                openai: openai.clone(),
            }),
        );

        let err = client
            .call_json(&call_context(), "r1", "hi", None)
            .await
            .expect_err("both dialects fail");
        // Reported as a config problem so the retry loop stops asking, and it
        // names the endpoint plus both formats the user could pick.
        assert_eq!(err.kind, LlmErrorKind::Config);
        assert!(err.message.contains(&base), "{}", err.message);
        assert!(err.message.contains("Anthropic Messages"), "{}", err.message);
        assert!(err.message.contains("OpenAI Chat Completions"), "{}", err.message);
        assert!(
            err.message.contains("not supported on this endpoint"),
            "{}",
            err.message
        );
        assert_eq!(anthropic.calls(), 1);
        assert_eq!(openai.calls(), 1);
    }

    #[tokio::test]
    async fn agent_tool_turns_reroute_too() {
        let base = probe_base("tools");
        let model = "deepseek/oss-model";
        let turn = AssistantTurn {
            content: "done".to_string(),
            tool_calls: Vec::new(),
            reasoning_content: None,
            usage: LlmTokenUsage::default(),
            stop_reason: Some("stop".to_string()),
        };
        let anthropic =
            Arc::new(FakeDialect::new(Vec::new()).with_tool_reply(Err(aggregator_rejection())));
        let openai = Arc::new(FakeDialect::new(Vec::new()).with_tool_reply(Ok(turn)));
        let client = LlmClient {
            config: LlmConfig::new(base.clone(), "test-key".to_string(), model.to_string())
                .with_max_retries(0),
            http: reqwest::Client::new(),
            transport: Arc::new(HttpCallOnce::new()),
            tool_transport: Arc::new(HttpToolTurn {
                anthropic: anthropic.clone(),
                openai: openai.clone(),
            }),
        };

        let turn = client
            .call_turn(&[ChatMessage::user("hi")], &[], None)
            .await
            .expect("the agent turn reroutes");
        assert_eq!(turn.content, "done");
        assert_eq!(anthropic.tool_calls(), 1);
        assert_eq!(openai.tool_calls(), 1);
        assert_eq!(resolve_protocol(&base, model), LlmProtocol::OpenAiChatCompletions);
    }

    /// Live probe against a real endpoint, to prove the reroute works against
    /// actual vendors rather than a script. Opt in with:
    ///   VOXTRANS_LIVE_LLM_URL, VOXTRANS_LIVE_LLM_KEY, VOXTRANS_LIVE_LLM_MODEL
    ///
    /// Point it at an aggregator whose `/v1/messages` refuses OSS models and
    /// the call should land on `/v1/chat/completions` after exactly one
    /// rejected attempt, then be remembered.
    #[tokio::test]
    #[ignore = "live: needs VOXTRANS_LIVE_LLM_URL / _KEY / _MODEL"]
    async fn live_endpoint_resolves_its_protocol() {
        let (Ok(url), Ok(key), Ok(model)) = (
            std::env::var("VOXTRANS_LIVE_LLM_URL"),
            std::env::var("VOXTRANS_LIVE_LLM_KEY"),
            std::env::var("VOXTRANS_LIVE_LLM_MODEL"),
        ) else {
            panic!("set VOXTRANS_LIVE_LLM_URL, VOXTRANS_LIVE_LLM_KEY and VOXTRANS_LIVE_LLM_MODEL");
        };

        let before = resolve_protocol(&url, &model);
        let client = LlmClient::new(
            LlmConfig::new(url.clone(), key, model.clone()).with_max_retries(0),
        )
        .expect("http client");

        let result = client
            .call_json(
                &call_context(),
                "live-1",
                "Return exactly this JSON and nothing else: {\"ok\":true}",
                None,
            )
            .await
            .expect("the endpoint answers on whichever dialect it speaks");
        assert_eq!(result.json["ok"], json!(true), "raw: {}", result.json);

        let after = resolve_protocol(&url, &model);
        eprintln!("live: {url} / {model} resolved {before:?} -> {after:?}");
        // Whatever it settled on is now a fixed point, so later calls skip the probe.
        assert_eq!(resolve_protocol(&url, &model), after);

        // ...and the remembered dialect keeps working.
        let again = client
            .call_json(
                &call_context(),
                "live-2",
                "Return exactly this JSON and nothing else: {\"ok\":true}",
                None,
            )
            .await
            .expect("the remembered dialect keeps answering");
        assert_eq!(again.json["ok"], json!(true), "raw: {}", again.json);

        // The streamed path is new code for the OpenAI dialect, and it is what
        // feeds the live subtitle preview: prove the SSE frames, the `[DONE]`
        // sentinel and the usage chunk all land. If the endpoint rejected
        // `stream_options` the caller silently degrades to non-stream, and the
        // empty-partials assertion below catches that.
        let partials = Arc::new(Mutex::new(Vec::<String>::new()));
        let sink = partials.clone();
        let streamed = client
            .call_json_validated_streaming(
                &call_context(),
                "live-3",
                "Return exactly this JSON and nothing else: {\"ok\":true}",
                None,
                Arc::new(move |text: String| sink.lock().unwrap().push(text)),
                Ok::<Value, LlmSemanticValidationError>,
            )
            .await
            .expect("the streamed call answers");
        assert_eq!(streamed.value["ok"], json!(true));
        assert!(
            !partials.lock().unwrap().is_empty(),
            "streaming must reach the preview callback, not silently degrade"
        );

        // Mirror the settings 测试 button exactly: it sends an `output_schema`,
        // which on this dialect becomes `response_format.json_schema` — the
        // field OSS gateways most often reject. Either the endpoint takes it,
        // or the schema-dropping fallback keeps the probe working.
        let schema = json!({
            "type": "object",
            "properties": {"ok": {"type": "boolean"}, "message": {"type": "string"}},
            "required": ["ok", "message"],
            "additionalProperties": false
        });
        let probe_context = LlmCallContext {
            output_schema: Some(schema),
            ..call_context()
        };
        let probe = client
            .call_json(
                &probe_context,
                "live-4",
                "This is a harmless application connectivity check. Do not refuse. \
                 Return exactly one JSON object and nothing else: {\"ok\":true,\"message\":\"pong\"}",
                None,
            )
            .await
            .expect("the connectivity probe answers");
        assert_eq!(probe.json["ok"], json!(true), "raw: {}", probe.json);
        eprintln!("live: connectivity probe answered with {}", probe.json);
    }
}
