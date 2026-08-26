use super::error::{LlmError, LlmErrorKind};

const RETRY_HINT_MAX_CHARS: usize = 320;
const SEMANTIC_RETRY_HINT_MAX_CHARS: usize = 900;

/// HTTP statuses worth retrying with backoff. Client errors outside this set
/// (400 bad request, 401/403 auth, 404 model) cannot succeed on retry, so
/// they fail fast instead of burning the backoff budget (previously every
/// Http error was retried).
const RETRYABLE_HTTP_STATUSES: [u16; 8] = [408, 409, 429, 500, 502, 503, 504, 529];

/// Base delay (ms) for exponential backoff between retry attempts.
/// Actual delay = `BASE * 2^exp`, where exp is capped at 2, so the
/// sequence is 2s, 4s, 8s, 8s, 8s, ... The client applies it to every
/// retryable failure (HTTP, JSON, schema, semantic) alike.
const RETRY_BACKOFF_BASE_MS: u64 = 2_000;
const RETRY_BACKOFF_MAX_EXP: u32 = 2;

#[derive(Debug, Clone)]
pub(super) struct RetryFeedback {
    pub(super) error_kind: LlmErrorKind,
    pub(super) retryable: bool,
    pub(super) retry_hint: Option<String>,
    pub(super) detail: String,
}

pub(super) fn retry_backoff_ms(attempt: u32, max_attempts: u32) -> Option<u64> {
    if attempt >= max_attempts {
        return None;
    }
    let exp = attempt.saturating_sub(1).min(RETRY_BACKOFF_MAX_EXP);
    Some(RETRY_BACKOFF_BASE_MS.saturating_mul(1u64 << exp))
}

/// The plateau of the default sequence (`BASE * 2^MAX_EXP`, i.e. 8s).
pub(super) const RETRY_BACKOFF_PLATEAU_MS: u64 =
    RETRY_BACKOFF_BASE_MS * (1u64 << RETRY_BACKOFF_MAX_EXP);

/// Effective wait between retry attempts. A server-advised `Retry-After`
/// wins over the default step but is capped at the sequence's plateau, so a
/// misbehaving gateway cannot stall a task past the normal backoff budget.
pub(super) fn effective_backoff_ms(default_ms: u64, retry_after_ms: Option<u64>) -> u64 {
    match retry_after_ms {
        None => default_ms,
        Some(server_ms) => server_ms.min(RETRY_BACKOFF_PLATEAU_MS),
    }
}

pub(super) fn feedback_from_llm_error(err: &LlmError) -> RetryFeedback {
    // JSON/schema/semantic failures are retried in the client with the
    // failure hint appended to the prompt; config failures can never
    // succeed on retry. Every retryable failure shares the same
    // exponential-backoff sequence between attempts.
    let retryable = match err.kind {
        LlmErrorKind::Config => false,
        LlmErrorKind::Http => match err.status {
            // Transport-level failures without a response keep retrying.
            None => true,
            Some(status) => RETRYABLE_HTTP_STATUSES.contains(&status),
        },
        _ => true,
    };
    let retry_hint = retry_hint_from_error(err.kind, &err.message);
    let detail = retry_hint
        .clone()
        .unwrap_or_else(|| compact_hint(&err.message, hint_budget(err.kind)));
    RetryFeedback {
        error_kind: err.kind,
        retryable,
        retry_hint,
        detail,
    }
}

/// Append the compacted failure hint to the user prompt when the previous
/// attempt failed JSON/schema/semantic validation and will be retried.
/// Returns the base prompt unchanged on the first attempt, when there is
/// no feedback yet, when the last failure was not retryable, or when it
/// carried no hint (HTTP failures retry with backoff and need no prompt
/// changes).
pub(super) fn augment_user_prompt_with_retry_feedback(
    base_user_prompt: &str,
    attempt: u32,
    last_feedback: Option<&RetryFeedback>,
) -> String {
    if attempt <= 1 {
        return base_user_prompt.to_string();
    }
    let Some(feedback) = last_feedback else {
        return base_user_prompt.to_string();
    };
    if !feedback.retryable {
        return base_user_prompt.to_string();
    }
    let Some(hint) = feedback.retry_hint.as_ref() else {
        return base_user_prompt.to_string();
    };
    format!(
        "{base_user_prompt}\n\n[RETRY FEEDBACK] Your previous response was rejected: {hint}. Respond again with corrected output only."
    )
}

fn hint_budget(kind: LlmErrorKind) -> usize {
    if matches!(kind, LlmErrorKind::InvalidSemantic) {
        SEMANTIC_RETRY_HINT_MAX_CHARS
    } else {
        RETRY_HINT_MAX_CHARS
    }
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
        Some(compact_hint(hint, hint_budget(kind)))
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
        RETRY_BACKOFF_PLATEAU_MS, RetryFeedback, SEMANTIC_RETRY_HINT_MAX_CHARS,
        augment_user_prompt_with_retry_feedback, effective_backoff_ms, feedback_from_llm_error,
        retry_backoff_ms,
    };
    use crate::services::llm::error::{LlmError, LlmErrorKind};

    #[test]
    fn feedback_marks_all_but_config_failures_retryable() {
        // Transport-level failure without a status stays retryable.
        let http = feedback_from_llm_error(&LlmError::new(LlmErrorKind::Http, "timeout"));
        assert!(http.retryable);

        let json = feedback_from_llm_error(&LlmError::new(
            LlmErrorKind::InvalidJson,
            "failed to extract valid json from llm response: boom",
        ));
        assert!(json.retryable);
        assert!(json.retry_hint.is_some());

        let schema = feedback_from_llm_error(&LlmError::new(
            LlmErrorKind::InvalidSchema,
            "schema check failed: missing key k",
        ));
        assert!(schema.retryable);

        let semantic = feedback_from_llm_error(&LlmError::new(
            LlmErrorKind::InvalidSemantic,
            "missing ids [2]",
        ));
        assert!(semantic.retryable);
        assert_eq!(semantic.retry_hint.as_deref(), Some("missing ids [2]"));

        let config = feedback_from_llm_error(&LlmError::new(LlmErrorKind::Config, "bad url"));
        assert!(!config.retryable);
    }

    #[test]
    fn http_status_classification() {
        let with_status = |status: u16| {
            let mut err = LlmError::new(LlmErrorKind::Http, format!("http status {status}"));
            err.status = Some(status);
            err
        };
        for status in [408, 409, 429, 500, 502, 503, 504, 529] {
            assert!(
                feedback_from_llm_error(&with_status(status)).retryable,
                "{status} should be retryable"
            );
        }
        for status in [400, 401, 403, 404] {
            assert!(
                !feedback_from_llm_error(&with_status(status)).retryable,
                "{status} should fail fast"
            );
        }
    }

    #[test]
    fn retry_prompt_appends_compacted_hint_from_last_feedback() {
        let feedback = RetryFeedback {
            error_kind: LlmErrorKind::InvalidSemantic,
            retryable: true,
            retry_hint: Some("missing ids [2]".into()),
            detail: "missing ids [2]".into(),
        };
        let prompt = augment_user_prompt_with_retry_feedback("BASE", 2, Some(&feedback));
        assert!(prompt.starts_with("BASE"));
        assert!(prompt.contains("\n\n[RETRY FEEDBACK]"));
        assert!(prompt.contains("missing ids [2]"));
        assert!(prompt.contains("Respond again with corrected output only."));
    }

    #[test]
    fn retry_prompt_first_attempt_returns_base_prompt() {
        let feedback = RetryFeedback {
            error_kind: LlmErrorKind::InvalidSemantic,
            retryable: true,
            retry_hint: Some("missing ids [2]".into()),
            detail: "missing ids [2]".into(),
        };
        let prompt = augment_user_prompt_with_retry_feedback("BASE", 1, Some(&feedback));
        assert_eq!(prompt, "BASE");
    }

    #[test]
    fn retry_prompt_unchanged_without_hint_or_retryable_feedback() {
        let http = RetryFeedback {
            error_kind: LlmErrorKind::Http,
            retryable: true,
            retry_hint: None,
            detail: "timeout".into(),
        };
        assert_eq!(
            augment_user_prompt_with_retry_feedback("BASE", 2, Some(&http)),
            "BASE"
        );

        let config = RetryFeedback {
            error_kind: LlmErrorKind::Config,
            retryable: false,
            retry_hint: None,
            detail: "bad url".into(),
        };
        assert_eq!(
            augment_user_prompt_with_retry_feedback("BASE", 2, Some(&config)),
            "BASE"
        );

        assert_eq!(augment_user_prompt_with_retry_feedback("BASE", 2, None), "BASE");
    }

    #[test]
    fn retry_prompt_compacts_overlong_semantic_messages() {
        let long_message = "missing ids [".to_string()
            + &(1..=300).map(|i| i.to_string()).collect::<Vec<_>>().join(",")
            + "]";
        let feedback = feedback_from_llm_error(&LlmError::new(
            LlmErrorKind::InvalidSemantic,
            long_message,
        ));
        let hint = feedback.retry_hint.as_ref().expect("hint");
        assert!(hint.chars().count() <= SEMANTIC_RETRY_HINT_MAX_CHARS + 3);
        assert!(hint.ends_with("..."));

        let prompt = augment_user_prompt_with_retry_feedback("BASE", 2, Some(&feedback));
        assert!(prompt.contains("[RETRY FEEDBACK]"));
        assert!(prompt.contains(hint));
    }

    #[test]
    fn backoff_stops_on_last_attempt() {
        assert_eq!(retry_backoff_ms(1, 4), Some(2_000));
        assert_eq!(retry_backoff_ms(4, 4), None);
    }

    #[test]
    fn effective_backoff_honors_retry_after_up_to_plateau() {
        assert_eq!(effective_backoff_ms(2_000, None), 2_000);
        // Server-advised wait below the plateau wins.
        assert_eq!(effective_backoff_ms(8_000, Some(50)), 50);
        // A bogus huge Retry-After cannot stall the task past the plateau.
        assert_eq!(effective_backoff_ms(8_000, Some(86_400_000)), RETRY_BACKOFF_PLATEAU_MS);
    }
}
