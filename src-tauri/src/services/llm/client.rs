use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

use crate::services::task_usage::{LlmTokenUsage as TaskUsage, record_llm_usage_best_effort};

use super::chat_completions::{call_chat_completion, call_chat_completion_stream};
use super::error::{LlmError, LlmErrorKind};
use super::event_payload::{
    attempt_base_payload, http_error_attempt_payload,
    invalid_semantic_attempt_payload, log_llm_call, logger_for_context,
    repair_failed_attempt_payload, repair_requested_payload,
    success_attempt_payload,
};
use super::json_guard::{
    JsonRepairOutcome, JsonResponseValidator, extract_and_repair_json_with_outcome,
};
use super::port::{LlmCallContext, LlmConfig, LlmJsonResult, LlmPort, LlmTokenUsage};
use super::retry::{
    RetryFeedback, augment_user_prompt_with_retry_feedback, build_json_repair_prompt,
    feedback_for_semantic, feedback_from_llm_error, retry_backoff_ms,
};

#[derive(Clone)]
pub struct OpenAiCompatLlmClient {
    config: LlmConfig,
    http: reqwest::Client,
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
        Ok(Self { config, http })
    }

    async fn call_once(
        &self,
        user_prompt: &str,
        images: Option<&[String]>,
        on_partial: Option<&(dyn Fn(String) + Send + Sync)>,
    ) -> Result<(String, LlmTokenUsage), LlmError> {
        if let Some(cb) = on_partial {
            let mut delta_cb = |acc: &str| {
                cb(acc.to_string());
            };
            match call_chat_completion_stream(
                &self.http,
                &self.config,
                user_prompt,
                images,
                Some(&mut delta_cb),
            )
            .await
            {
                Ok(result) => return Ok(result),
                Err(err) => {
                    // Provider may reject stream:true — fall back so translation still works.
                    eprintln!(
                        "[warn] chat completion stream failed ({err}); falling back to non-stream"
                    );
                }
            }
        }
        call_chat_completion(&self.http, &self.config, user_prompt, images).await
    }

    pub async fn call_json_validated<T, F>(
        &self,
        context: &LlmCallContext,
        request_id: &str,
        user_prompt: &str,
        images: Option<&[String]>,
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
            images,
            response_validator,
            None,
            semantic_validate,
        )
        .await
    }

    /// Like [`Self::call_json_validated`], but streams tokens and invokes
    /// `on_partial` with the **accumulated** raw assistant text (caller should
    /// throttle UI side-effects). Final validation/repair path is unchanged.
    pub async fn call_json_validated_streaming<T, F>(
        &self,
        context: &LlmCallContext,
        request_id: &str,
        user_prompt: &str,
        images: Option<&[String]>,
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
            images,
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
        images: Option<&[String]>,
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
        let mut validation_failures = 0u32;

        for attempt in 1..=max_attempts {
            let effective_user_prompt = augment_user_prompt_with_retry_feedback(
                user_prompt,
                attempt,
                max_attempts,
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
                .call_once(&effective_user_prompt, images, partial_ref)
                .await
            {
                Ok((raw_text, usage)) => {
                    let mut parsed = match extract_and_repair_json_with_outcome(&raw_text) {
                        Ok(v) => v,
                        Err(err) => {
                            validation_failures += 1;
                            match self
                                .repair_json_response(
                                    context,
                                    request_id,
                                    user_prompt,
                                    response_validator,
                                    &raw_text,
                                    &err,
                                )
                                .await
                            {
                                Ok(repaired) => repaired,
                                Err(repair_err) => {
                                    let feedback = feedback_from_llm_error(&repair_err);
                                    last_error = feedback.detail.clone();
                                    last_feedback = Some(feedback.clone());
                                    log_llm_call(
                                        logger.as_ref(),
                                        repair_failed_attempt_payload(
                                            &base_payload,
                                            "invalid_json",
                                            &last_error,
                                            feedback.error_kind.as_str(),
                                            feedback.retry_hint.as_deref(),
                                            &raw_text,
                                        ),
                                    );
                                    break;
                                }
                            }
                        }
                    };

                    if let Some(validator) = response_validator
                        && let Err(err) = validator.validate(&parsed.value) {
                            validation_failures += 1;
                            match self
                                .repair_json_response(
                                    context,
                                    request_id,
                                    user_prompt,
                                    response_validator,
                                    &raw_text,
                                    &err,
                                )
                                .await
                            {
                                Ok(repaired) => parsed = repaired,
                                Err(repair_err) => {
                                    let feedback = feedback_from_llm_error(&repair_err);
                                    last_error = feedback.detail.clone();
                                    last_feedback = Some(feedback.clone());
                                    log_llm_call(
                                        logger.as_ref(),
                                        repair_failed_attempt_payload(
                                            &base_payload,
                                            "invalid_schema",
                                            &last_error,
                                            feedback.error_kind.as_str(),
                                            feedback.retry_hint.as_deref(),
                                            &raw_text,
                                        ),
                                    );
                                    break;
                                }
                            }
                        }

                    match semantic_validate(parsed.value.clone()) {
                        Ok(value) => {
                            let elapsed_ms = started.elapsed().as_millis();
                            log_llm_call(
                                logger.as_ref(),
                                success_attempt_payload(
                                    &base_payload,
                                    validation_failures,
                                    parsed.source.as_str(),
                                    &raw_text,
                                    elapsed_ms,
                                    &usage,
                                ),
                            );
                            record_llm_usage_best_effort(
                                &context.task_id,
                                &context.phase,
                                TaskUsage {
                                    prompt_tokens: usage.prompt_tokens,
                                    completion_tokens: usage.completion_tokens,
                                    total_tokens: usage.total_tokens,
                                },
                                context.store.clone(),
                            );

                            return Ok(LlmValidatedJsonResult { value });
                        }
                        Err(LlmSemanticValidationError::Retryable(message)) => {
                            let feedback = feedback_for_semantic(message, Some(&raw_text));
                            last_error = feedback.detail.clone();
                            last_feedback = Some(feedback.clone());
                            let backoff_ms = retry_backoff_ms(attempt, max_attempts);
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
                        retry_backoff_ms(attempt, max_attempts)
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

        let exhausted_feedback = last_feedback.unwrap_or_else(|| RetryFeedback {
            error_kind: LlmErrorKind::InvalidSemantic,
            retryable: true,
            retry_hint: None,
            detail: last_error.clone(),
            previous_output: None,
        });

        let retry_hint_suffix = exhausted_feedback
            .retry_hint
            .as_ref()
            .filter(|hint| !hint.eq_ignore_ascii_case(exhausted_feedback.detail.as_str()))
            .map(|hint| format!("; retry_hint={hint}"))
            .unwrap_or_default();

        Err(LlmError::new(
            exhausted_feedback.error_kind,
            format!(
                "llm call failed after {} attempts: kind={}{}",
                max_attempts,
                exhausted_feedback.error_kind.as_str(),
                retry_hint_suffix
            ),
        ))
    }
}

impl OpenAiCompatLlmClient {
    async fn repair_json_response(
        &self,
        context: &LlmCallContext,
        request_id: &str,
        original_prompt: &str,
        response_validator: Option<&JsonResponseValidator>,
        raw_text: &str,
        failure: &LlmError,
    ) -> Result<JsonRepairOutcome, LlmError> {
        let logger = logger_for_context(context);

        let repair_prompt =
            build_json_repair_prompt(original_prompt, response_validator, raw_text, failure);
        log_llm_call(
            logger.as_ref(),
            repair_requested_payload(failure.kind.as_str(), context, request_id),
        );

        // Images are intentionally dropped on the repair retry: the repair
        // prompt is about fixing JSON shape, not re-answering from visuals.
        // Sending images again would only re-cost tokens for a syntactic fix.
        let (repair_raw_text, _) = self.call_once(&repair_prompt, None, None).await?;
        let repaired = extract_and_repair_json_with_outcome(&repair_raw_text)?;
        if let Some(validator) = response_validator {
            validator.validate(&repaired.value)?;
        }
        Ok(repaired)
    }
}

impl LlmPort for OpenAiCompatLlmClient {
    async fn call_json(
        &self,
        context: &LlmCallContext,
        request_id: &str,
        user_prompt: &str,
        images: Option<&[String]>,
        response_validator: Option<&JsonResponseValidator>,
    ) -> Result<LlmJsonResult, LlmError> {
        let result = self
            .call_json_validated(
                context,
                request_id,
                user_prompt,
                images,
                response_validator,
                Ok::<Value, LlmSemanticValidationError>,
            )
            .await?;
        Ok(LlmJsonResult { json: result.value })
    }
}
