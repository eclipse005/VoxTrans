use async_trait::async_trait;
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

use crate::db::store::TaskStore;
use crate::services::task_log::TaskLogger;
use crate::services::task_usage::{LlmTokenUsage as TaskUsage, record_llm_usage_best_effort};

use super::chat_completions::{
    call_chat_completion, call_chat_completion_stream, call_chat_completion_tools, AssistantTurn,
    ChatMessage,
};
use super::error::{LlmError, LlmErrorKind};
use super::event_payload::{
    attempt_base_payload, failed_attempt_payload, http_error_attempt_payload,
    invalid_semantic_attempt_payload, log_llm_call, logger_for_context,
    success_attempt_payload,
};
use super::json_guard::{JsonResponseValidator, extract_and_repair_json_with_outcome};
use super::port::{LlmCallContext, LlmConfig, LlmJsonResult, LlmPort, LlmTokenUsage};
use super::retry::{
    RetryFeedback, augment_user_prompt_with_retry_feedback, feedback_from_llm_error,
    retry_backoff_ms,
};

/// One raw chat-completion call without retries or JSON validation, plus
/// the per-attempt hooks the retry loop delegates to. This is the seam
/// unit tests use: production goes through [`HttpCallOnce`], tests script
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

/// Production transport: streams when a callback is given, falls back to a
/// plain completion when the provider rejects streaming.
struct HttpCallOnce;

#[async_trait]
impl LlmCallOnce for HttpCallOnce {
    async fn call_once(
        &self,
        http: &reqwest::Client,
        config: &LlmConfig,
        logger: Option<&TaskLogger>,
        user_prompt: &str,
        on_partial: Option<&(dyn Fn(String) + Send + Sync)>,
    ) -> Result<(String, LlmTokenUsage), LlmError> {
        if let Some(cb) = on_partial {
            let mut delta_cb = |acc: &str| {
                cb(acc.to_string());
            };
            match call_chat_completion_stream(http, config, user_prompt, Some(&mut delta_cb))
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
                        "[warn] chat completion stream failed ({err}); falling back to non-stream"
                    ),
                },
            }
        }
        call_chat_completion(http, config, user_prompt).await
    }
}

#[derive(Clone)]
pub struct OpenAiCompatLlmClient {
    config: LlmConfig,
    http: reqwest::Client,
    transport: Arc<dyn LlmCallOnce>,
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

impl OpenAiCompatLlmClient {
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
        })
    }

    async fn call_once(
        &self,
        logger: Option<&TaskLogger>,
        user_prompt: &str,
        on_partial: Option<&(dyn Fn(String) + Send + Sync)>,
    ) -> Result<(String, LlmTokenUsage), LlmError> {
        self.transport
            .call_once(&self.http, &self.config, logger, user_prompt, on_partial)
            .await
    }

    /// OpenAI-compatible tool-calling turn for the terminology agent.
    pub async fn call_tools(
        &self,
        messages: &[ChatMessage],
        tools: &[serde_json::Value],
        temperature: Option<f64>,
    ) -> Result<AssistantTurn, LlmError> {
        call_chat_completion_tools(&self.http, &self.config, messages, tools, temperature).await
    }

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
        // between attempts. Whatever fails on the final attempt exits the
        // loop and is reported as `last_error` in the error below.
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
                .call_once(logger.as_ref(), &effective_user_prompt, partial_ref)
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
                        self.transport.backoff_ms(attempt, max_attempts)
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

impl LlmPort for OpenAiCompatLlmClient {
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
    use std::sync::Mutex;
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
            },
        ))
    }

    fn test_client(transport: Arc<dyn LlmCallOnce>, max_retries: u32) -> OpenAiCompatLlmClient {
        let mut config =
            LlmConfig::new("http://test.local".into(), "test-key".into(), "test-model".into());
        config.max_retries = max_retries;
        OpenAiCompatLlmClient {
            config,
            http: reqwest::Client::new(),
            transport,
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
            "unexpected message: {}",
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
                "req-6",
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
}
