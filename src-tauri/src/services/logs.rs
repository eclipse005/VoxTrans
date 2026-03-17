use chrono::Local;
use serde::Deserialize;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppendTaskLogRequest {
    pub task_id: String,
    pub channel: String,
    pub message: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadTaskLogRequest {
    pub task_id: String,
    pub channel: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClearTaskLogsRequest {
    pub task_id: String,
    pub channel: Option<String>,
}

pub fn append_task_log(request: AppendTaskLogRequest) -> Result<(), String> {
    let message = request.message.trim();
    if message.is_empty() {
        return Ok(());
    }

    let log_path = task_log_path(&request.task_id, &request.channel)?;
    if let Some(parent) = log_path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .map_err(|e| e.to_string())?;

    let now = Local::now().format("%Y-%m-%d %H:%M:%S");
    writeln!(file, "[{}] {}", now, message).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn read_task_log(request: ReadTaskLogRequest) -> Result<String, String> {
    let log_path = task_log_path(&request.task_id, &request.channel)?;
    if !log_path.exists() {
        return Ok(String::new());
    }
    fs::read_to_string(log_path).map_err(|e| e.to_string())
}

pub fn clear_task_logs(request: ClearTaskLogsRequest) -> Result<(), String> {
    let channels = if let Some(channel) = request.channel.as_deref() {
        vec![channel.to_string()]
    } else {
        vec!["main".to_string()]
    };

    for channel in channels {
        let log_path = task_log_path(&request.task_id, &channel)?;
        if let Some(parent) = log_path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        fs::write(&log_path, "").map_err(|e| e.to_string())?;
    }

    Ok(())
}

fn task_log_path(task_id: &str, channel: &str) -> Result<PathBuf, String> {
    if task_id.trim().is_empty() {
        return Err("taskId is required".to_string());
    }

    let file_name = match channel.trim() {
        "main" => "main.log",
        _ => return Err("channel must be main".to_string()),
    };

    let task_dir = crate::services::task_path::task_log_dir(task_id);
    Ok(task_dir.join(file_name))
}
