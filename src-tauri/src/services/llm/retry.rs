use crate::services::prompts::llm::{
    build_json_repair_prompt as build_json_repair_prompt_text, build_retry_constrained_prompt,
};

use super::error::{LlmError, LlmErrorKind};
use super::json_guard::JsonResponseValidator;

const RETRY_HINT_MAX_CHARS: usize = 320;
const REPAIR_RAW_TEXT_MAX_CHARS: usize = 8_000;

#[derive(Debug, Clone)]
pub(super) struct RetryFeedback {
    pub(super) error_kind: String,
    pub(super) retryable: bool,
    pub(super) retry_hint: Option<String>,
    pub(super) detail: String,
}

pub(super) fn retry_backoff_ms(attempt: u32, max_attempts: u32) -> Option<u64> {
    if attempt >= max_attempts {
        return None;
    }
    let exp = attempt.saturating_sub(1).min(2);
    let base_ms = 2_000u64;
    Some(base_ms.saturating_mul(1u64 << exp))
}

pub(super) fn feedback_for_semantic(message: String) -> RetryFeedback {
    let hint = compact_hint(&message, RETRY_HINT_MAX_CHARS);
    RetryFeedback {
        error_kind: LlmErrorKind::InvalidSemantic.as_str().to_string(),
        retryable: true,
        retry_hint: Some(hint),
        detail: message,
    }
}

pub(super) fn feedback_from_llm_error(err: &LlmError) -> RetryFeedback {
    let retryable = !matches!(err.kind, LlmErrorKind::Config);
    let retry_hint = retry_hint_from_error(err.kind, &err.message);
    let detail = retry_hint
        .clone()
        .unwrap_or_else(|| compact_hint(&err.message, RETRY_HINT_MAX_CHARS));
    RetryFeedback {
        error_kind: err.kind.as_str().to_string(),
        retryable,
        retry_hint,
        detail,
    }
}

pub(super) fn augment_user_prompt_with_retry_feedback(
    base_user_prompt: &str,
    attempt: u32,
    max_attempts: u32,
    last_feedback: Option<&RetryFeedback>,
) -> String {
    if attempt <= 1 {
        return base_user_prompt.to_string();
    }

    let Some(feedback) = last_feedback else {
        return base_user_prompt.to_string();
    };
    let Some(hint) = feedback.retry_hint.as_ref() else {
        return base_user_prompt.to_string();
    };
    if !feedback.retryable {
        return base_user_prompt.to_string();
    }

    build_retry_constrained_prompt(base_user_prompt, attempt, max_attempts, hint)
}

pub(super) fn build_json_repair_prompt(
    original_prompt: &str,
    response_validator: Option<&JsonResponseValidator>,
    raw_text: &str,
    failure: &LlmError,
) -> String {
    let schema_constraints = response_validator
        .map(|validator| validator.describe_constraints())
        .unwrap_or_else(|| "Return one valid JSON value.".to_string());
    let failure_hint = retry_hint_from_error(failure.kind, &failure.message)
        .unwrap_or_else(|| compact_hint(&failure.message, RETRY_HINT_MAX_CHARS));
    let original_prompt = truncate_chars(original_prompt.trim(), 2_000);
    let raw_text = truncate_chars(raw_text.trim(), REPAIR_RAW_TEXT_MAX_CHARS);

    build_json_repair_prompt_text(
        &original_prompt,
        &schema_constraints,
        &failure_hint,
        &raw_text,
    )
}

fn retry_hint_from_error(kind: LlmErrorKind, message: &str) -> Option<String> {
    let trimmed = message.trim();
    if trimmed.is_empty() {
        return None;
    }

    let hint = match kind {
        LlmErrorKind::InvalidSchema => {
            strip_prefix_case_insensitive(trimmed, "schema check failed:")
                .unwrap_or(trimmed)
                .trim()
        }
        LlmErrorKind::InvalidJson => {
            let detail = strip_prefix_case_insensitive(
                trimmed,
                "failed to extract valid json from llm response:",
            )
            .unwrap_or(trimmed)
            .trim();
            return Some(compact_invalid_json_hint(detail, RETRY_HINT_MAX_CHARS));
        }
        LlmErrorKind::InvalidSemantic => trimmed,
        _ => return None,
    };

    if hint.is_empty() {
        None
    } else {
        Some(compact_hint(hint, RETRY_HINT_MAX_CHARS))
    }
}

fn compact_invalid_json_hint(detail: &str, max_chars: usize) -> String {
    let mut reasons: Vec<String> = Vec::new();
    for part in detail.split('|') {
        let mut item = part.trim();
        if item.is_empty() {
            continue;
        }
        if let Some((head, _)) = item.split_once("; near:") {
            item = head.trim();
        }
        if let Some((head, _)) = item.split_once("; raw preview:") {
            item = head.trim();
        }
        item = strip_prefix_case_insensitive(item, "candidate parse failed:")
            .unwrap_or(item)
            .trim();
        item = strip_prefix_case_insensitive(item, "repaired candidate parse failed:")
            .unwrap_or(item)
            .trim();
        if item.is_empty() {
            continue;
        }
        let normalized = compact_hint(item, max_chars);
        if !reasons
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(&normalized))
        {
            reasons.push(normalized);
        }
    }

    if reasons.is_empty() {
        return compact_hint(detail, max_chars);
    }
    compact_hint(&reasons.join("; "), max_chars)
}

fn compact_hint(input: &str, max_chars: usize) -> String {
    let normalized = input
        .replace('\r', " ")
        .replace('\n', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    truncate_chars(&normalized, max_chars)
}

fn truncate_chars(input: &str, max_chars: usize) -> String {
    if input.chars().count() <= max_chars {
        return input.to_string();
    }
    let mut out = String::new();
    for (index, ch) in input.chars().enumerate() {
        if index >= max_chars {
            break;
        }
        out.push(ch);
    }
    out.push_str("...");
    out
}

fn strip_prefix_case_insensitive<'a>(input: &'a str, prefix: &str) -> Option<&'a str> {
    if input.len() < prefix.len() {
        return None;
    }
    let (head, tail) = input.split_at(prefix.len());
    if head.eq_ignore_ascii_case(prefix) {
        Some(tail)
    } else {
        None
    }
}
