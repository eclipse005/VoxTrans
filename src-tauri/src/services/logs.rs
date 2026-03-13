use chrono::Local;
use serde::Deserialize;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppendTaskLogRequest {
    pub task_id: String,
    pub media_path: String,
    pub channel: String,
    pub message: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadTaskLogRequest {
    pub task_id: String,
    pub media_path: String,
    pub channel: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClearTaskLogsRequest {
    pub task_id: String,
    pub media_path: String,
    pub channel: Option<String>,
}

pub fn append_task_log(request: AppendTaskLogRequest) -> Result<(), String> {
    let message = request.message.trim();
    if message.is_empty() {
        return Ok(());
    }

    let log_path = task_log_path(&request.task_id, &request.media_path, &request.channel)?;
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
    let log_path = task_log_path(&request.task_id, &request.media_path, &request.channel)?;
    if !log_path.exists() {
        return Ok(String::new());
    }
    fs::read_to_string(log_path).map_err(|e| e.to_string())
}

pub fn clear_task_logs(request: ClearTaskLogsRequest) -> Result<(), String> {
    let channels = if let Some(channel) = request.channel.as_deref() {
        vec![channel.to_string()]
    } else {
        vec!["main".to_string(), "llm".to_string()]
    };

    for channel in channels {
        let log_path = task_log_path(&request.task_id, &request.media_path, &channel)?;
        if let Some(parent) = log_path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        fs::write(&log_path, "").map_err(|e| e.to_string())?;
    }

    Ok(())
}

fn task_log_path(task_id: &str, media_path: &str, channel: &str) -> Result<PathBuf, String> {
    if task_id.trim().is_empty() {
        return Err("taskId is required".to_string());
    }
    if media_path.trim().is_empty() {
        return Err("mediaPath is required".to_string());
    }

    let file_name = match channel.trim() {
        "main" => "main.log",
        "llm" => "llm.log",
        _ => return Err("channel must be main or llm".to_string()),
    };

    let task_dir = task_output_dir(task_id, Path::new(media_path));
    Ok(task_dir.join("log").join(file_name))
}

fn task_output_dir(task_id: &str, audio_path: &Path) -> PathBuf {
    let stem = audio_path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "transcript".to_string());
    let safe_stem = sanitize_filename_component(&stem);
    let safe_task_id = sanitize_filename_component(task_id);
    resolve_output_dir().join(format!("{}_{}", safe_stem, safe_task_id))
}

fn sanitize_filename_component(raw: &str) -> String {
    raw.chars()
        .map(|ch| match ch {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            _ => ch,
        })
        .collect::<String>()
        .trim()
        .trim_matches('.')
        .to_string()
}

fn resolve_output_dir() -> PathBuf {
    if let Ok(custom_dir) = std::env::var("VOXTRANS_OUTPUT_DIR") {
        let path = PathBuf::from(custom_dir);
        if !path.as_os_str().is_empty() {
            return path;
        }
    }

    let tauri_manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let project_root = tauri_manifest_dir
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or(tauri_manifest_dir);
    project_root.join("output")
}
