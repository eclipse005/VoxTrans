use crate::services::prompts::llm::{
    build_json_repair_prompt as build_json_repair_prompt_text, build_retry_constrained_prompt,
};

use super::error::{LlmError, LlmErrorKind};
use super::json_guard::JsonResponseValidator;

/// Compact budget for generic / HTTP retry hints.
const RETRY_HINT_MAX_CHARS: usize = 320;
/// Semantic validation messages list every bad id; keep more room so the model
/// can see the full missing/empty/duplicate set in one retry.
const SEMANTIC_RETRY_HINT_MAX_CHARS: usize = 900;
/// Truncated previous assistant output attached to semantic retries.
const PREVIOUS_OUTPUT_MAX_CHARS: usize = 4_000;
const REPAIR_RAW_TEXT_MAX_CHARS: usize = 8_000;

/// Base delay (ms) for exponential backoff between retry attempts.
/// Actual delay = `BASE * 2^exp`, where exp is capped at 2, so the
/// sequence is 2s, 4s, 8s, 8s, 8s, ... Extracted from a magic literal
/// so the tuning knob is in one place.
const RETRY_BACKOFF_BASE_MS: u64 = 2_000;
/// Max exponent for backoff — caps the doubling so the delay doesn't
/// grow unbounded after many attempts.
const RETRY_BACKOFF_MAX_EXP: u32 = 2;

#[derive(Debug, Clone)]
pub(super) struct RetryFeedback {
    pub(super) error_kind: LlmErrorKind,
    pub(super) retryable: bool,
    pub(super) retry_hint: Option<String>,
    pub(super) detail: String,
    /// Truncated previous model output for semantic correction retries.
    pub(super) previous_output: Option<String>,
}

pub(super) fn retry_backoff_ms(attempt: u32, max_attempts: u32) -> Option<u64> {
    if attempt >= max_attempts {
        return None;
    }
    let exp = attempt.saturating_sub(1).min(RETRY_BACKOFF_MAX_EXP);
    Some(RETRY_BACKOFF_BASE_MS.saturating_mul(1u64 << exp))
}

pub(super) fn feedback_for_semantic(
    message: String,
    previous_output: Option<&str>,
) -> RetryFeedback {
    let hint = compact_hint(&message, SEMANTIC_RETRY_HINT_MAX_CHARS);
    let previous_output = previous_output
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| truncate_chars(s, PREVIOUS_OUTPUT_MAX_CHARS));
    RetryFeedback {
        error_kind: LlmErrorKind::InvalidSemantic,
        retryable: true,
        retry_hint: Some(hint),
        detail: message,
        previous_output,
    }
}

pub(super) fn feedback_from_llm_error(err: &LlmError) -> RetryFeedback {
    let retryable = !matches!(err.kind, LlmErrorKind::Config);
    let retry_hint = retry_hint_from_error(err.kind, &err.message);
    let detail = retry_hint
        .clone()
        .unwrap_or_else(|| compact_hint(&err.message, RETRY_HINT_MAX_CHARS));
    RetryFeedback {
        error_kind: err.kind,
        retryable,
        retry_hint,
        detail,
        previous_output: None,
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

    build_retry_constrained_prompt(
        base_user_prompt,
        attempt,
        max_attempts,
        hint,
        feedback.previous_output.as_deref(),
    )
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
        let max = if matches!(kind, LlmErrorKind::InvalidSemantic) {
            SEMANTIC_RETRY_HINT_MAX_CHARS
        } else {
            RETRY_HINT_MAX_CHARS
        };
        Some(compact_hint(hint, max))
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
        .replace(['\r', '\n'], " ")
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

#[cfg(test)]
mod tests {
    use super::{
        RetryFeedback, augment_user_prompt_with_retry_feedback, feedback_for_semantic,
    };
    use crate::services::llm::error::LlmErrorKind;

    #[test]
    fn semantic_feedback_keeps_aggregated_ids_and_previous_output() {
        let message = "missing ids [3,5,7]; empty ids [2]; got ids [1,4,6]; expected 7 items";
        let prev = r#"{"translations":[{"id":1,"text":"ok"}]}"#;
        let fb = feedback_for_semantic(message.to_string(), Some(prev));

        assert!(fb.retryable);
        assert_eq!(fb.error_kind, LlmErrorKind::InvalidSemantic);
        let hint = fb.retry_hint.expect("hint");
        assert!(hint.contains("missing ids [3,5,7]"));
        assert!(hint.contains("empty ids [2]"));
        assert_eq!(fb.previous_output.as_deref(), Some(prev));
    }

    #[test]
    fn augment_prompt_attaches_previous_output_on_retry() {
        let feedback = RetryFeedback {
            error_kind: LlmErrorKind::InvalidSemantic,
            retryable: true,
            retry_hint: Some("missing ids [2]".into()),
            detail: "missing ids [2]".into(),
            previous_output: Some(r#"{"translations":[{"id":1,"text":"a"}]}"#.into()),
        };

        let prompt =
            augment_user_prompt_with_retry_feedback("BASE", 2, 4, Some(&feedback));
        assert!(prompt.contains("BASE"));
        assert!(prompt.contains("missing ids [2]"));
        assert!(prompt.contains("## Previous incomplete output"));
        assert!(prompt.contains("\"id\":1"));
        assert!(prompt.contains("FULL batch"));
    }

    #[test]
    fn first_attempt_returns_base_prompt_unchanged() {
        let feedback = feedback_for_semantic("missing ids [1]".into(), Some("{}"));
        let prompt =
            augment_user_prompt_with_retry_feedback("BASE_ONLY", 1, 4, Some(&feedback));
        assert_eq!(prompt, "BASE_ONLY");
    }
}
