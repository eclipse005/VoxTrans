use serde_json::Value;
use std::time::Duration;
use tokio::time::sleep;

use crate::services::task_usage::{LlmTokenUsage as TaskUsage, record_llm_usage_best_effort};

use super::cache::{append_cache_entry, read_cache_hit};
use super::chat_completions::call_chat_completion;
use super::error::{LlmError, LlmErrorKind};
use super::event_payload::{
    attempt_base_payload, cache_hit_payload, cache_invalid_payload, http_error_attempt_payload,
    invalid_semantic_attempt_payload, log_llm_call, logger_for_context,
    repair_failed_attempt_payload, repair_requested_payload, should_persist_artifacts,
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

    async fn call_once(&self, user_prompt: &str) -> Result<(String, LlmTokenUsage), LlmError> {
        call_chat_completion(&self.http, &self.config, user_prompt).await
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
        let logger = logger_for_context(context);
        let persist_artifacts = should_persist_artifacts(context);

        if persist_artifacts
            && let Some(cache_hit) =
                read_cache_hit(context, &self.config, "", user_prompt, response_validator)
        {
            if let Some(validator) = response_validator {
                if let Err(err) = validator.validate(&cache_hit.json) {
                    log_llm_call(
                        logger.as_ref(),
                        cache_invalid_payload(
                            "cache_invalid_schema",
                            err.message,
                            context,
                            request_id,
                        ),
                    );
                } else {
                    match semantic_validate(cache_hit.json.clone()) {
                        Ok(value) => {
                            log_llm_call(
                                logger.as_ref(),
                                cache_hit_payload(&self.config, context, request_id),
                            );
                            return Ok(LlmValidatedJsonResult { value });
                        }
                        Err(LlmSemanticValidationError::Retryable(message)) => {
                            log_llm_call(
                                logger.as_ref(),
                                cache_invalid_payload(
                                    "cache_invalid_semantic",
                                    message,
                                    context,
                                    request_id,
                                ),
                            );
                        }
                    }
                }
            } else if let Ok(value) = semantic_validate(cache_hit.json.clone()) {
                log_llm_call(
                    logger.as_ref(),
                    cache_hit_payload(&self.config, context, request_id),
                );
                return Ok(LlmValidatedJsonResult { value });
            }
        }

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

            match self.call_once(&effective_user_prompt).await {
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
                                            &feedback.error_kind,
                                            feedback.retry_hint.as_deref(),
                                            &raw_text,
                                        ),
                                    );
                                    break;
                                }
                            }
                        }
                    };

                    if let Some(validator) = response_validator {
                        if let Err(err) = validator.validate(&parsed.value) {
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
                                            &feedback.error_kind,
                                            feedback.retry_hint.as_deref(),
                                            &raw_text,
                                        ),
                                    );
                                    break;
                                }
                            }
                        }
                    }

                    match semantic_validate(parsed.value.clone()) {
                        Ok(value) => {
                            if persist_artifacts {
                                append_cache_entry(
                                    context,
                                    &self.config,
                                    "",
                                    user_prompt,
                                    response_validator,
                                    &raw_text,
                                    &parsed.value,
                                    &usage,
                                );
                            }
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
                            if persist_artifacts {
                                record_llm_usage_best_effort(
                                    &context.task_id,
                                    &context.phase,
                                    TaskUsage {
                                        prompt_tokens: usage.prompt_tokens,
                                        completion_tokens: usage.completion_tokens,
                                        total_tokens: usage.total_tokens,
                                    },
                                );
                            }

                            return Ok(LlmValidatedJsonResult { value });
                        }
                        Err(LlmSemanticValidationError::Retryable(message)) => {
                            let feedback = feedback_for_semantic(message);
                            last_error = feedback.detail.clone();
                            last_feedback = Some(feedback.clone());
                            let backoff_ms = retry_backoff_ms(attempt, max_attempts);
                            log_llm_call(
                                logger.as_ref(),
                                invalid_semantic_attempt_payload(
                                    &base_payload,
                                    &last_error,
                                    &feedback.error_kind,
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
                            &feedback.error_kind,
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
            error_kind: LlmErrorKind::InvalidSemantic.as_str().to_string(),
            retryable: true,
            retry_hint: None,
            detail: last_error.clone(),
        });

        let retry_hint_suffix = exhausted_feedback
            .retry_hint
            .as_ref()
            .filter(|hint| !hint.eq_ignore_ascii_case(exhausted_feedback.detail.as_str()))
            .map(|hint| format!("; retry_hint={hint}"))
            .unwrap_or_default();

        Err(LlmError::new(
            LlmErrorKind::InvalidSemantic,
            format!(
                "llm call failed after {} attempts: kind={}{}",
                max_attempts, exhausted_feedback.error_kind, retry_hint_suffix
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

        let (repair_raw_text, _) = self.call_once(&repair_prompt).await?;
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
