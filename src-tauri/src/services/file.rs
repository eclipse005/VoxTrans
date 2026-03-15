use crate::services::task_log::{TaskLogTarget, append_event_best_effort, event};
use serde::Deserialize;
use serde_json::json;

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

pub fn save_srt(request: SaveSrtRequest) -> Result<(), String> {
    let log_target = match (&request.task_id, &request.media_path) {
        (Some(task_id), Some(media_path))
            if !task_id.trim().is_empty() && !media_path.trim().is_empty() =>
        {
            Some(TaskLogTarget::main(task_id.clone(), media_path.clone()))
        }
        _ => None,
    };
    if let Some(target) = &log_target {
        append_event_best_effort(
            target,
            event::TRANSCRIBE_SAVE_SRT_STARTED,
            Some(&json!({ "outputPath": request.output_path })),
        );
    }

    if let Some(parent) = std::path::Path::new(&request.output_path).parent() {
        if let Err(err) = std::fs::create_dir_all(parent) {
            let message = err.to_string();
            if let Some(target) = &log_target {
                append_event_best_effort(
                    target,
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
    if let Err(err) = std::fs::write(&request.output_path, request.content) {
        let message = err.to_string();
        if let Some(target) = &log_target {
            append_event_best_effort(
                target,
                event::TRANSCRIBE_FAILED,
                Some(&json!({
                    "phase": "save_srt",
                    "error": message,
                })),
            );
        }
        return Err(message);
    }

    if let Some(target) = &log_target {
        append_event_best_effort(
            target,
            event::TRANSCRIBE_SAVE_SRT_COMPLETED,
            Some(&json!({ "outputPath": request.output_path })),
        );
    }
    Ok(())
}

pub fn get_file_size(path: String) -> Result<u64, String> {
    let metadata = std::fs::metadata(&path).map_err(|err| err.to_string())?;
    Ok(metadata.len())
}
