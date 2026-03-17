use crate::services::task_log::{TaskLogger, event};
use serde::Deserialize;
use serde_json::json;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveSrtRequest {
    #[serde(default)]
    pub task_id: Option<String>,
    pub output_path: String,
    pub content: String,
}

pub fn save_srt(request: SaveSrtRequest) -> Result<(), String> {
    let started_at = std::time::Instant::now();
    let logger = match &request.task_id {
        Some(task_id) if !task_id.trim().is_empty() => Some(TaskLogger::main(task_id.clone())),
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
