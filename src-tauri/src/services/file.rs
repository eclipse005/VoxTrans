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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportTaskSrtsRequest {
    pub task_id: String,
    pub target_dir: String,
    #[serde(default)]
    pub task_name: Option<String>,
    pub items: Vec<crate::services::subtitle_srt::ExportSrtItem>,
}

pub fn save_srt(request: SaveSrtRequest) -> Result<(), String> {
    let started_at = std::time::Instant::now();
    let logger = match (&request.task_id, &request.media_path) {
        (Some(task_id), Some(media_path))
            if !task_id.trim().is_empty() && !media_path.trim().is_empty() =>
        {
            Some(TaskLogger::main_with_media(
                task_id.clone(),
                media_path.clone(),
            ))
        }
        (Some(task_id), _) if !task_id.trim().is_empty() => Some(TaskLogger::main(task_id.clone())),
        _ => None,
    };
    if let Some(parent) = std::path::Path::new(&request.output_path).parent()
        && let Err(err) = std::fs::create_dir_all(parent) {
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
        return Err(format!("Export directory does not exist: {}", target_dir));
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

pub fn export_task_srts(request: ExportTaskSrtsRequest) -> Result<Vec<String>, String> {
    if request.task_id.trim().is_empty() {
        return Err("taskId is required".to_string());
    }
    let target_dir = request.target_dir.trim();
    if target_dir.is_empty() {
        return Err("targetDir is required".to_string());
    }
    if request.items.is_empty() {
        return Err("items is required".to_string());
    }

    let started_at = std::time::Instant::now();
    let logger = TaskLogger::main(request.task_id.clone());
    let dir_path = PathBuf::from(target_dir);
    if !dir_path.is_dir() {
        return Err(format!("Export directory does not exist: {}", target_dir));
    }

    let task_item = crate::commands::workspace::get_task_queue_item_for_export(&request.task_id)?;
    let segments =
        crate::services::subtitle_srt::parse_segments_json(&task_item.subtitle_segments_json)
            .map_err(|err| format!("Failed to parse subtitle segments: {err}"))?;
    if segments.is_empty() {
        return Err("Current task has no exportable subtitle segments".to_string());
    }
    // Export is pure serialization: what the editor shows is what lands on
    // disk. Beautify runs where the data is produced (SoT materialisation) or
    // when the user asks for it, never as a surprise at export time.

    let output_paths = crate::services::subtitle_srt::write_variants_to_directory(
        &dir_path,
        &segments,
        &request.items,
    )
    .map_err(|err| {
        logger.event(
            event::TRANSCRIBE_FAILED,
            Some(&json!({
                "phase": "export_task_srts",
                "error": err,
            })),
        );
        err
    })?;

    logger.event(
        "transcribe.exported",
        Some(&json!({
            "outputPaths": output_paths.clone(),
            "itemCount": request.items.len(),
            "segmentCount": segments.len(),
            "taskName": request.task_name,
            "exportElapsedSec": round2(started_at.elapsed().as_secs_f64()),
        })),
    );

    Ok(output_paths)
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

#[cfg(test)]
mod tests {
    /// Export is pure serialization: the file must contain exactly what the
    /// editor showed. A beautify pass here would silently rewrite text the
    /// user already reviewed, which is the class of bug this guard exists for.
    /// BONE-BREAK target: re-adding any `beautify_*` call to `export_task_srts`
    /// turns this red.
    #[test]
    fn export_task_srts_does_not_transform_text_or_timings() {
        const SOURCE: &str = include_str!("file.rs");
        let start = SOURCE
            .find("pub fn export_task_srts")
            .expect("export_task_srts must exist");
        let end = SOURCE[start + 1..]
            .find("\npub fn ")
            .map(|offset| start + 1 + offset)
            .unwrap_or(SOURCE.len());
        let body = &SOURCE[start..end];
        assert!(
            !body.contains("beautify_"),
            "export_task_srts must not run beautify; it runs where the SoT is produced"
        );
    }
}
