use crate::services::task_log::{TaskLogger, event};
use serde::Deserialize;
use serde_json::json;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveSrtRequest {
    #[serde(default)]
    pub task_id: Option<String>,
    #[serde(default)]
    pub media_path: Option<String>,
    pub output_path: String,
    pub content: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportSrtRequest {
    pub task_id: String,
    pub target_dir: String,
    #[serde(default)]
    pub task_name: Option<String>,
    pub content: String,
}

pub fn save_srt(request: SaveSrtRequest) -> Result<(), String> {
    let started_at = std::time::Instant::now();
    let logger = match (&request.task_id, &request.media_path) {
        (Some(task_id), Some(media_path))
            if !task_id.trim().is_empty() && !media_path.trim().is_empty() =>
        {
            Some(TaskLogger::main_with_media(task_id.clone(), media_path.clone()))
        }
        (Some(task_id), _) if !task_id.trim().is_empty() => Some(TaskLogger::main(task_id.clone())),
        _ => None,
    };
    if let Some(parent) = std::path::Path::new(&request.output_path).parent() {
        if let Err(err) = std::fs::create_dir_all(parent) {
            let message = err.to_string();
            if let Some(logger) = &logger {
                logger.event(
                    event::TRANSCRIBE_FAILED,
                    Some(&json!({
                        "phase": "save_srt",
                        "error": message,
                    })),
                );
            }
            return Err(message);
        }
    }
    let srt_bytes = request.content.len();
    if let Err(err) = std::fs::write(&request.output_path, request.content.as_bytes()) {
        let message = err.to_string();
        if let Some(logger) = &logger {
            logger.event(
                event::TRANSCRIBE_FAILED,
                Some(&json!({
                    "phase": "save_srt",
                    "error": message,
                })),
            );
        }
        return Err(message);
    }

    if let Some(logger) = &logger {
        let payload = json!({
            "outputPath": request.output_path,
            "srtBytes": srt_bytes,
            "saveElapsedSec": round2(started_at.elapsed().as_secs_f64()),
        });
        logger.event(event::TRANSCRIBE_SAVED, Some(&payload));
    }
    Ok(())
}

pub fn export_srt(request: ExportSrtRequest) -> Result<String, String> {
    if request.task_id.trim().is_empty() {
        return Err("taskId is required".to_string());
    }
    let target_dir = request.target_dir.trim();
    if target_dir.is_empty() {
        return Err("targetDir is required".to_string());
    }

    let started_at = std::time::Instant::now();
    let logger = TaskLogger::main(request.task_id.clone());

    let dir_path = PathBuf::from(target_dir);
    if !dir_path.is_dir() {
        return Err(format!("导出目录不存在: {}", target_dir));
    }

    let file_stem = request
        .task_name
        .as_deref()
        .map(crate::services::task_path::sanitize_filename_component)
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| format!("subtitle_{}", request.task_id));
    let output_path = dir_path.join(format!("{}_en.srt", file_stem));

    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    std::fs::write(&output_path, request.content.as_bytes()).map_err(|err| {
        let message = err.to_string();
        logger.event(
            event::TRANSCRIBE_FAILED,
            Some(&json!({
                "phase": "export_srt",
                "error": message,
            })),
        );
        message
    })?;

    logger.event(
        "transcribe.exported",
        Some(&json!({
            "outputPath": output_path.display().to_string(),
            "srtBytes": request.content.len(),
            "exportElapsedSec": round2(started_at.elapsed().as_secs_f64()),
        })),
    );

    Ok(output_path.display().to_string())
}

pub fn get_file_size(path: String) -> Result<u64, String> {
    let metadata = std::fs::metadata(&path).map_err(|err| err.to_string())?;
    Ok(metadata.len())
}

fn round2(value: f64) -> f64 {
    if !value.is_finite() {
        return 0.0;
    }
    (value * 100.0).round() / 100.0
}
