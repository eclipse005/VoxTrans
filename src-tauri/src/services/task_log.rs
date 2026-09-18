use serde_json::Value;

pub mod event {
    pub const TASK_STARTED: &str = "task.started";
    pub const TRANSCRIBE_STARTED: &str = "transcribe.started";
    pub const TRANSCRIBE_COMPLETED: &str = "transcribe.completed";
    pub const TRANSCRIBE_SAVED: &str = "transcribe.saved";
    pub const TRANSCRIBE_FAILED: &str = "transcribe.failed";
    pub const TASK_FAILED: &str = "task.failed";
    /// Terminology events are named `terminology.start/end/skip/resumed`.
    /// Older runs wrote `terminology.agent.*`; only the log text changed, so
    /// reading back either spelling keeps working.
    pub const TERMINOLOGY_START: &str = "terminology.start";
    pub const TERMINOLOGY_END: &str = "terminology.end";
    pub const TERMINOLOGY_SKIP: &str = "terminology.skip";
    pub const TERMINOLOGY_RESUMED: &str = "terminology.resumed";
}

#[derive(Debug, Clone)]
struct TaskLogTarget {
    pub task_id: String,
    pub media_path: Option<String>,
    pub channel: String,
}

impl TaskLogTarget {
    fn main(task_id: impl Into<String>) -> Self {
        Self {
            task_id: task_id.into(),
            media_path: None,
            channel: "main".to_string(),
        }
    }

    fn main_with_media(task_id: impl Into<String>, media_path: impl Into<String>) -> Self {
        Self {
            task_id: task_id.into(),
            media_path: Some(media_path.into()),
            channel: "main".to_string(),
        }
    }

    fn llm_with_media(task_id: impl Into<String>, media_path: impl Into<String>) -> Self {
        Self {
            task_id: task_id.into(),
            media_path: Some(media_path.into()),
            channel: "llm".to_string(),
        }
    }

    fn llm(task_id: impl Into<String>) -> Self {
        Self {
            task_id: task_id.into(),
            media_path: None,
            channel: "llm".to_string(),
        }
    }
}

pub struct TaskLogger {
    target: TaskLogTarget,
}

impl TaskLogger {
    pub fn main(task_id: impl Into<String>) -> Self {
        Self {
            target: TaskLogTarget::main(task_id),
        }
    }

    pub fn main_with_media(task_id: impl Into<String>, media_path: impl Into<String>) -> Self {
        Self {
            target: TaskLogTarget::main_with_media(task_id, media_path),
        }
    }

    pub fn llm_with_media(task_id: impl Into<String>, media_path: impl Into<String>) -> Self {
        Self {
            target: TaskLogTarget::llm_with_media(task_id, media_path),
        }
    }

    pub fn llm(task_id: impl Into<String>) -> Self {
        Self {
            target: TaskLogTarget::llm(task_id),
        }
    }

    pub fn event(&self, event_type: &str, payload: Option<&Value>) {
        append_event_best_effort(&self.target, event_type, payload);
        mirror_failure_to_app_log(&self.target, event_type, payload);
    }
}

/// Failure events are mirrored into the app-wide error log. A task log only helps
/// once the user knows *which* task to send; `app.log` is what they can hand over
/// for "it broke". Task correlation travels as `taskId`.
///
/// Only failure events are mirrored — the rest of the task log is per-task
/// detail. `terminology.skip` is deliberately absent: it is recorded where the
/// reason is known (see `terminology_agent`), so mirroring it here would double
/// every line.
fn mirror_failure_to_app_log(target: &TaskLogTarget, event_type: &str, payload: Option<&Value>) {
    let code = match event_type {
        event::TASK_FAILED => "TASK_FAILED",
        event::TRANSCRIBE_FAILED => "TRANSCRIBE_FAILED",
        _ => return,
    };
    let message = payload
        .and_then(|value| value.get("error"))
        .and_then(Value::as_str)
        .unwrap_or(event_type);

    crate::services::app_log::error(
        "task",
        code,
        message,
        Some(serde_json::json!({
            "taskId": &target.task_id,
            "channel": &target.channel,
            "event": event_type,
            "payload": payload,
        })),
    );
}

fn append_event(
    target: &TaskLogTarget,
    event_type: &str,
    payload: Option<&Value>,
) -> Result<(), String> {
    let event_type = event_type.trim();
    if event_type.is_empty() {
        return Err("event_type is required".to_string());
    }

    let message = format_task_log_line(event_type, payload);
    crate::services::logs::append_task_log(crate::services::logs::AppendTaskLogRequest {
        task_id: target.task_id.clone(),
        media_path: target.media_path.clone(),
        channel: target.channel.clone(),
        message,
    })
}

fn append_event_best_effort(target: &TaskLogTarget, event_type: &str, payload: Option<&Value>) {
    let _ = append_event(target, event_type, payload);
}

fn format_task_log_line(event_type: &str, payload: Option<&Value>) -> String {
    match payload {
        None => event_type.to_string(),
        Some(value) if value.as_object().is_some_and(|obj| obj.is_empty()) => {
            event_type.to_string()
        }
        Some(value) => format!(
            "{event_type}\n{}",
            serde_json::to_string_pretty(value).unwrap_or_else(|_| "{}".to_string())
        ),
    }
}
