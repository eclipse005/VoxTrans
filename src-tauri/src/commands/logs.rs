use tauri::{AppHandle, Manager};

use crate::db::store::TaskStore;
use crate::services::logs;
use crate::services::task_usage;

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadTaskLogCommandRequest {
    pub task_id: String,
    #[serde(default)]
    pub media_path: Option<String>,
    pub channel: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClearTaskLogsCommandRequest {
    pub task_id: String,
    #[serde(default)]
    pub media_path: Option<String>,
    pub channel: Option<String>,
}

#[tauri::command]
pub fn read_task_log(request: ReadTaskLogCommandRequest) -> Result<String, String> {
    crate::services::app_log::guard("logs", "READ_TASK_LOG_FAILED", || {
        logs::read_task_log(logs::ReadTaskLogRequest {
            task_id: request.task_id,
            media_path: request.media_path,
            channel: request.channel,
        })
    })
}

/// The app log's state, for the settings screen.
///
/// Read-only: it reports whether writes are landing, and why they are not.
/// Without this, a log that quietly stopped being written looks exactly like a
/// run where nothing went wrong.
#[tauri::command]
pub async fn get_app_log_health() -> Result<crate::services::app_log::AppLogHealth, String> {
    Ok(crate::services::app_log::health())
}

#[tauri::command]
pub fn clear_task_logs(request: ClearTaskLogsCommandRequest) -> Result<(), String> {
    crate::services::app_log::guard("logs", "CLEAR_TASK_LOGS_FAILED", || {
        logs::clear_task_logs(logs::ClearTaskLogsRequest {
            task_id: request.task_id,
            media_path: request.media_path,
            channel: request.channel,
        })
    })
}

#[tauri::command]
pub async fn get_task_total_tokens(app: AppHandle, task_id: String) -> Result<u64, String> {
    crate::services::app_log::guard_async("logs", "GET_TASK_TOTAL_TOKENS_FAILED", async {
        let store = app.state::<TaskStore>().inner().clone();
        task_usage::get_task_total_tokens(&task_id, &store).await
    })
    .await
}

/// Both caps mirror the frontend truncation in `src/app/utils/errors.ts`. The
/// backend enforces its own regardless: a buggy or hostile WebView must not be
/// able to write unbounded text into app.log.
const FRONTEND_MESSAGE_MAX_CHARS: usize = 2000;
const FRONTEND_CONTEXT_MAX_CHARS: usize = 200;
/// Counts toward the cap so a truncated field never exceeds its budget.
const FRONTEND_TRUNCATION_MARKER: char = '…';
const FRONTEND_UNKNOWN_CONTEXT: &str = "unknown";
const FRONTEND_EMPTY_MESSAGE: &str = "(empty error)";

/// 前端错误上报：`context` 是调用点标签（如 "react.render"、"registerTaskUpload"），
/// `message` 是已序列化的错误文本。记录到 app_log，scope 固定 "frontend"。
/// 永远返回 Ok：上报失败不能再产生一个错误（会递归）。
#[tauri::command]
pub async fn report_frontend_error(context: String, message: String) -> Result<(), String> {
    let code = frontend_error_code(&context);

    crate::services::app_log::error(
        "frontend",
        code,
        normalize_frontend_message(&message),
        Some(serde_json::json!({
            "context": normalize_frontend_context(&context),
            "origin": "webview",
        })),
    );

    Ok(())
}

/// Stable code derived from the call-site label, so "did this bug come back?"
/// is answerable by grepping one identifier.
fn frontend_error_code(context: &str) -> &'static str {
    match context.trim().to_ascii_lowercase().as_str() {
        "window.error" => "FRONTEND_WINDOW_ERROR",
        "window.unhandledrejection" => "FRONTEND_UNHANDLED_REJECTION",
        "react.render" => "FRONTEND_REACT_RENDER",
        "react.componentstack" => "FRONTEND_REACT_COMPONENT_STACK",
        _ => "FRONTEND_ERROR",
    }
}

/// Trim an untrusted context label: whitespace-only becomes `"unknown"`, and a
/// component stack accidentally passed in the context slot is clipped before it
/// reaches the log.
fn normalize_frontend_context(context: &str) -> String {
    let trimmed = context.trim();
    if trimmed.is_empty() {
        return FRONTEND_UNKNOWN_CONTEXT.to_string();
    }
    truncate_frontend_chars(trimmed, FRONTEND_CONTEXT_MAX_CHARS)
}

/// Trim an untrusted message: empty becomes a placeholder, oversized stacks are
/// clipped. The frontend already does this, but the backend never trusts it.
fn normalize_frontend_message(message: &str) -> String {
    let trimmed = message.trim();
    if trimmed.is_empty() {
        return FRONTEND_EMPTY_MESSAGE.to_string();
    }
    truncate_frontend_chars(trimmed, FRONTEND_MESSAGE_MAX_CHARS)
}

/// Character-safe clip: `chars()` never splits a multi-byte sequence, unlike a
/// byte slice or a UTF-16 index. The marker is part of the budget so the result
/// never exceeds `max_chars`.
fn truncate_frontend_chars(text: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let keep = max_chars.saturating_sub(1);
    let mut truncated: String = text.chars().take(keep).collect();
    truncated.push(FRONTEND_TRUNCATION_MARKER);
    truncated
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frontend_error_codes_are_normalized_from_the_call_site_label() {
        assert_eq!(frontend_error_code("window.error"), "FRONTEND_WINDOW_ERROR");
        assert_eq!(
            frontend_error_code("window.unhandledrejection"),
            "FRONTEND_UNHANDLED_REJECTION"
        );
        assert_eq!(frontend_error_code("react.render"), "FRONTEND_REACT_RENDER");
        assert_eq!(
            frontend_error_code("react.componentStack"),
            "FRONTEND_REACT_COMPONENT_STACK"
        );
        assert_eq!(frontend_error_code("registerTaskUpload"), "FRONTEND_ERROR");
        assert_eq!(frontend_error_code(""), "FRONTEND_ERROR");
    }

    #[test]
    fn empty_frontend_inputs_get_placeholders_instead_of_blank_fields() {
        assert_eq!(normalize_frontend_context(""), "unknown");
        assert_eq!(normalize_frontend_context("  \n\t "), "unknown");
        assert_eq!(
            normalize_frontend_context("registerTaskUpload"),
            "registerTaskUpload"
        );
        assert_eq!(normalize_frontend_message(""), "(empty error)");
        assert_eq!(normalize_frontend_message("   "), "(empty error)");
    }

    #[test]
    fn long_messages_are_truncated_by_character_not_by_byte() {
        // CJK (3 bytes) followed by emoji (4 bytes, a surrogate pair in UTF-16):
        // a byte-based or UTF-16-based cut would corrupt the boundary.
        let message = format!("{}{}", "错".repeat(1500), "🙂".repeat(1500));
        assert_eq!(message.chars().count(), 3000);

        let truncated = normalize_frontend_message(&message);

        assert_eq!(truncated.chars().count(), FRONTEND_MESSAGE_MAX_CHARS);
        assert!(truncated.ends_with(FRONTEND_TRUNCATION_MARKER));
        assert!(truncated.starts_with(&"错".repeat(1500)));
        assert_eq!(
            truncated.chars().filter(|ch| *ch == '🙂').count(),
            FRONTEND_MESSAGE_MAX_CHARS - 1500 - 1
        );
    }

    #[test]
    fn messages_at_the_cap_are_kept_verbatim() {
        let exact = "a".repeat(FRONTEND_MESSAGE_MAX_CHARS);
        assert_eq!(normalize_frontend_message(&exact), exact);

        let one_over = "a".repeat(FRONTEND_MESSAGE_MAX_CHARS + 1);
        let truncated = normalize_frontend_message(&one_over);
        assert_eq!(truncated.chars().count(), FRONTEND_MESSAGE_MAX_CHARS);
        assert!(truncated.ends_with(FRONTEND_TRUNCATION_MARKER));
        assert_eq!(
            truncated.chars().filter(|ch| *ch == 'a').count(),
            FRONTEND_MESSAGE_MAX_CHARS - 1
        );
    }

    #[test]
    fn long_contexts_are_capped_so_component_stacks_stay_out_of_the_file() {
        let context = "x".repeat(FRONTEND_CONTEXT_MAX_CHARS + 42);
        let normalized = normalize_frontend_context(&context);
        assert_eq!(normalized.chars().count(), FRONTEND_CONTEXT_MAX_CHARS);
        assert!(normalized.ends_with(FRONTEND_TRUNCATION_MARKER));
    }

    #[test]
    fn truncation_never_splits_a_multibyte_character() {
        let text = "🙂".repeat(10);
        let truncated = truncate_frontend_chars(&text, 6);
        assert_eq!(truncated.chars().count(), 6);
        assert_eq!(
            truncated,
            format!("{}{}", "🙂".repeat(5), FRONTEND_TRUNCATION_MARKER)
        );
        // Every character is still a whole code point (no replacement chars).
        assert!(!truncated.contains('\u{FFFD}'));
    }
}
