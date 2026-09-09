use async_trait::async_trait;
use serde_json::{Value, json};
use std::collections::HashSet;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use tokio::time::sleep;

use crate::db::store::TaskStore;
use crate::services::task_log::TaskLogger;
use crate::services::task_usage::{LlmTokenUsage as TaskUsage, record_llm_usage_best_effort};

use super::anthropic::{
    AssistantTurn, ChatMessage, call_message, call_message_stream, call_message_tools,
};
use super::base_url::normalize_base_url;
use super::error::{LlmError, LlmErrorKind};
use super::event_payload::{
    attempt_base_payload, failed_attempt_payload, http_error_attempt_payload,
    invalid_semantic_attempt_payload, log_llm_call, logger_for_context, success_attempt_payload,
};
use super::json_guard::{JsonResponseValidator, extract_and_repair_json_with_outcome};
use super::port::{LlmCallContext, LlmConfig, LlmJsonResult, LlmPort, LlmTokenUsage};
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

/// Endpoints that already rejected `output_config` once — remembered per
/// normalized base_url so later batches skip the schema instead of eating a
/// 400 every round (project's existing global-mutex pattern).
static STRUCTURED_OUTPUT_UNSUPPORTED: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

fn is_structured_output_unsupported(base_url: &str) -> bool {
    let key = normalize_base_url(base_url);
    STRUCTURED_OUTPUT_UNSUPPORTED
        .get_or_init(|| Mutex::new(HashSet::new()))
        .lock()
        .map(|set| set.contains(&key))
        .unwrap_or(false)
}

fn mark_structured_output_unsupported(base_url: &str) {
    let key = normalize_base_url(base_url);
    if let Ok(mut set) = STRUCTURED_OUTPUT_UNSUPPORTED
        .get_or_init(|| Mutex::new(HashSet::new()))
        .lock()
    {
        set.insert(key);
    }
}

/// Does this error mean "endpoint does not support `output_config.format`"?
/// Only schema-related 400/422s trigger the fallback; unrelated bad requests
/// keep failing loudly.
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

/// Production transport: streams when a callback is given, falls back to a
/// plain completion when the provider rejects streaming, and falls back to
/// unstructured generation when it rejects `output_config`.
struct HttpCallOnce;

impl HttpCallOnce {
    async fn attempt(
        &self,
        http: &reqwest::Client,
        config: &LlmConfig,
        logger: Option<&TaskLogger>,
        user_prompt: &str,
        on_partial: Option<&(dyn Fn(String) + Send + Sync)>,
        output_schema: Option<&Value>,
    ) -> Result<(String, LlmTokenUsage), LlmError> {
        if let Some(cb) = on_partial {
            let mut delta_cb = |acc: &str| {
                cb(acc.to_string());
            };
            match call_message_stream(http, config, user_prompt, output_schema, Some(&mut delta_cb))
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
                            "error": err.message,
                        }),
                    ),
                    None => eprintln!(
                        "[warn] anthropic stream failed ({err}); falling back to non-stream"
                    ),
                },
            }
        }
        call_message(http, config, user_prompt, output_schema).await
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
        let effective_schema = match output_schema {
            Some(schema) if !is_structured_output_unsupported(&config.base_url) => Some(schema),
            _ => None,
        };
        if let Some(schema) = effective_schema {
            return match self
                .attempt(http, config, logger, user_prompt, on_partial, Some(schema))
                .await
            {
                Ok(result) => Ok(result),
                Err(err) if is_structured_output_rejection(&err) => {
                    mark_structured_output_unsupported(&config.base_url);
                    match logger {
                        Some(logger) => log_llm_call(
                            Some(logger),
                            json!({
                                "status": "structured_output_fallback",
                                "error": err.message,
                            }),
                        ),
                        None => eprintln!(
                            "[warn] endpoint rejected structured output ({err}); falling back to plain JSON"
                        ),
                    }
                    self.attempt(http, config, logger, user_prompt, on_partial, None)
                        .await
                }
                Err(err) => Err(err),
            };
        }
        self.attempt(http, config, logger, user_prompt, on_partial, None)
            .await
    }
}

#[derive(Clone)]
pub struct AnthropicLlmClient {
    config: LlmConfig,
    http: reqwest::Client,
    transport: Arc<dyn LlmCallOnce>,
    /// Wire seam for tool turns; the retry loop lives in the
    /// [`ToolTurnCaller`] impl below.
    tool_transport: Arc<dyn ToolTurnOnce>,
}

/// One tool-calling turn against the wire, no retries — the seam the retry
/// loop in [`AnthropicLlmClient`]'s [`ToolTurnCaller`] impl and tests
/// delegate to.
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

struct HttpToolTurn;

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
        call_message_tools(http, config, messages, tools, temperature).await
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
impl ToolTurnCaller for AnthropicLlmClient {
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
                    eprintln!(
                        "[terminology-llm] tools turn attempt {attempt}/{max_attempts} failed ({err}); retrying"
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

impl AnthropicLlmClient {
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
            transport: Arc::new(HttpCallOnce),
            tool_transport: Arc::new(HttpToolTurn),
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
                            let feedback = feedback_from_llm_error(&err);
                            last_error = feedback.detail.clone();
                            last_feedback = Some(feedback.clone());
                            let backoff_ms = if feedback.retryable {
                                self.transport.backoff_ms(attempt, max_attempts)
                            } else {
                                None
                            };
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
                        let feedback = feedback_from_llm_error(&err);
                        last_error = feedback.detail.clone();
                        last_feedback = Some(feedback.clone());
                        let backoff_ms = if feedback.retryable {
                            self.transport.backoff_ms(attempt, max_attempts)
                        } else {
                            None
                        };
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
                            let feedback = feedback_from_llm_error(&err);
                            last_error = feedback.detail.clone();
                            last_feedback = Some(feedback.clone());
                            let backoff_ms = if feedback.retryable {
                                self.transport.backoff_ms(attempt, max_attempts)
                            } else {
                                None
                            };
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
                    let feedback = feedback_from_llm_error(&err);
                    last_error = feedback.detail.clone();
                    last_feedback = Some(feedback.clone());
                    let backoff_ms = if feedback.retryable {
                        // Server-advised wait wins, capped at the sequence
                        // plateau (policy lives in retry.rs).
                        self.transport
                            .backoff_ms(attempt, max_attempts)
                            .map(|default| effective_backoff_ms(default, err.retry_after_ms))
                    } else {
                        None
                    };
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

impl LlmPort for AnthropicLlmClient {
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

    fn test_client(transport: Arc<dyn LlmCallOnce>, max_retries: u32) -> AnthropicLlmClient {
        let mut config =
            LlmConfig::new("http://test.local".into(), "test-key".into(), "test-model".into());
        config.max_retries = max_retries;
        AnthropicLlmClient {
            config,
            http: reqwest::Client::new(),
            transport,
            tool_transport: Arc::new(HttpToolTurn),
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

    fn tool_client(tool: Arc<ScriptedToolTurn>, max_retries: u32) -> AnthropicLlmClient {
        let mut config =
            LlmConfig::new("http://test.local".into(), "test-key".into(), "test-model".into());
        config.max_retries = max_retries;
        AnthropicLlmClient {
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
    fn structured_output_unsupported_memo_is_per_base_url() {
        let probe = format!("http://memo-test.local/{}", line!());
        assert!(!is_structured_output_unsupported(&probe));
        mark_structured_output_unsupported(&probe);
        assert!(is_structured_output_unsupported(&probe));
        assert!(is_structured_output_unsupported(&format!("{probe}/")));
        // Other endpoints are unaffected.
        assert!(!is_structured_output_unsupported("http://other.local"));
    }
}
