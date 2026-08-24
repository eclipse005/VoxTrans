use serde_json::Value;

pub mod event {
    pub const TASK_STARTED: &str = "task.started";
    pub const TRANSCRIBE_STARTED: &str = "transcribe.started";
    pub const TRANSCRIBE_COMPLETED: &str = "transcribe.completed";
    pub const TRANSCRIBE_SAVED: &str = "transcribe.saved";
    pub const TRANSCRIBE_FAILED: &str = "transcribe.failed";
    pub const TASK_FAILED: &str = "task.failed";
    pub const TERMINOLOGY_AGENT_START: &str = "terminology.agent.start";
    pub const TERMINOLOGY_AGENT_END: &str = "terminology.agent.end";
    pub const TERMINOLOGY_AGENT_RESUMED: &str = "terminology.agent.resumed";
    pub const AGENT_START: &str = "agent.start";
    pub const AGENT_END: &str = "agent.end";
    pub const AGENT_SKIP: &str = "agent.skip";
    pub const AGENT_RESUMED: &str = "agent.resumed";
    pub const AGENT_WINDOW_START: &str = "agent.window.start";
    pub const AGENT_WINDOW_CHECKPOINT: &str = "agent.window.checkpoint";
    pub const AGENT_WINDOW_END: &str = "agent.window.end";
    pub const AGENT_ROUND: &str = "agent.round";
    pub const AGENT_TOOL: &str = "agent.tool";
    pub const AGENT_HARNESS: &str = "agent.harness";
}

#[derive(Debug, Clone)]
pub struct TaskLogTarget {
    pub task_id: String,
    pub media_path: Option<String>,
    pub channel: String,
}

impl TaskLogTarget {
    pub fn main(task_id: impl Into<String>) -> Self {
        Self {
            task_id: task_id.into(),
            media_path: None,
            channel: "main".to_string(),
        }
    }

    pub fn main_with_media(task_id: impl Into<String>, media_path: impl Into<String>) -> Self {
        Self {
            task_id: task_id.into(),
            media_path: Some(media_path.into()),
            channel: "main".to_string(),
        }
    }

    pub fn llm_with_media(task_id: impl Into<String>, media_path: impl Into<String>) -> Self {
        Self {
            task_id: task_id.into(),
            media_path: Some(media_path.into()),
            channel: "llm".to_string(),
        }
    }

    pub fn llm(task_id: impl Into<String>) -> Self {
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

    pub fn agent_with_media(task_id: impl Into<String>, media_path: impl Into<String>) -> Self {
        Self {
            target: TaskLogTarget {
                task_id: task_id.into(),
                media_path: Some(media_path.into()),
                channel: "agent".to_string(),
            },
        }
    }

    pub fn event(&self, event_type: &str, payload: Option<&Value>) {
        append_event_best_effort(&self.target, event_type, payload);
    }
}

pub fn append_event(
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

pub fn append_event_best_effort(target: &TaskLogTarget, event_type: &str, payload: Option<&Value>) {
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
