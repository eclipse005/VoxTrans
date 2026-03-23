use crate::services::final_subtitle::{
    FinalSubtitleTrack, final_subtitle_segments_to_srt, parse_final_subtitle_segments,
};
use crate::services::task_log::{TaskLogger, event};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::SqlitePool;
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
    pub items: Vec<ExportSrtItem>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub enum ExportSrtItem {
    Source,
    Target,
    BilingualSourceFirst,
    BilingualTargetFirst,
}

#[derive(Debug, Deserialize, sqlx::FromRow)]
struct ExportTaskRow {
    id: String,
    name: String,
    media_path: String,
    source_lang: String,
    target_lang: String,
    subtitle_segments_json: String,
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

pub async fn export_task_srts(
    pool: &SqlitePool,
    request: ExportTaskSrtsRequest,
) -> Result<Vec<String>, String> {
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
    let row = sqlx::query_as::<_, ExportTaskRow>(
        "SELECT id, name, media_path, source_lang, target_lang, subtitle_segments_json
         FROM task_runs
         WHERE id = ?",
    )
    .bind(request.task_id.trim())
    .fetch_optional(pool)
    .await
    .map_err(|err| err.to_string())?
    .ok_or_else(|| "task not found".to_string())?;
    let logger = TaskLogger::main_with_media(row.id.clone(), row.media_path.clone());

    let dir_path = PathBuf::from(target_dir);
    if !dir_path.is_dir() {
        return Err(format!("导出目录不存在: {}", target_dir));
    }

    let segments = parse_final_subtitle_segments(&row.subtitle_segments_json);
    if segments.is_empty() {
        return Err("最终字幕为空，无法导出".to_string());
    }

    let source_suffix = normalize_lang_suffix(&row.source_lang, "en");
    let target_suffix = normalize_lang_suffix(&row.target_lang, "zh");
    let file_stem = request
        .task_name
        .as_deref()
        .map(crate::services::task_path::sanitize_filename_component)
        .filter(|s| !s.trim().is_empty())
        .or_else(|| {
            let candidate = crate::services::task_path::sanitize_filename_component(&row.name);
            if candidate.trim().is_empty() {
                None
            } else {
                Some(candidate)
            }
        })
        .unwrap_or_else(|| format!("subtitle_{}", row.id));

    let mut exported_paths: Vec<String> = Vec::new();
    for item in request.items {
        let content = match item {
            ExportSrtItem::Source => {
                final_subtitle_segments_to_srt(&segments, FinalSubtitleTrack::Source)
            }
            ExportSrtItem::Target => {
                final_subtitle_segments_to_srt(&segments, FinalSubtitleTrack::Target)
            }
            ExportSrtItem::BilingualSourceFirst => final_subtitle_segments_to_srt(
                &segments,
                FinalSubtitleTrack::BilingualSourceFirst,
            ),
            ExportSrtItem::BilingualTargetFirst => final_subtitle_segments_to_srt(
                &segments,
                FinalSubtitleTrack::BilingualTargetFirst,
            ),
        };
        if content.trim().is_empty() {
            return Err("所选导出项为空，无法导出".to_string());
        }

        let file_name = match item {
            ExportSrtItem::Source => format!("{}_{}.srt", file_stem, source_suffix),
            ExportSrtItem::Target => format!("{}_{}.srt", file_stem, target_suffix),
            ExportSrtItem::BilingualSourceFirst => {
                format!("{}_{}_{}.srt", file_stem, source_suffix, target_suffix)
            }
            ExportSrtItem::BilingualTargetFirst => {
                format!("{}_{}_{}.srt", file_stem, target_suffix, source_suffix)
            }
        };
        let output_path = ensure_unique_output_path(&dir_path, &file_name);
        std::fs::write(&output_path, content.as_bytes()).map_err(|err| err.to_string())?;
        exported_paths.push(output_path.display().to_string());
    }

    logger.event(
        "transcribe.exported",
        Some(&json!({
            "outputPathList": exported_paths,
            "exportedTotal": exported_paths.len(),
            "exportElapsedSec": round2(started_at.elapsed().as_secs_f64()),
        })),
    );

    Ok(exported_paths)
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

fn normalize_lang_suffix(lang: &str, default_lang: &str) -> String {
    let lowered = lang.trim().to_lowercase();
    if lowered.is_empty() {
        return default_lang.to_string();
    }
    if lowered.starts_with("zh") {
        return "zh".to_string();
    }
    let prefix = lowered
        .split(|ch: char| ch == '-' || ch == '_' || !ch.is_ascii_alphanumeric())
        .find(|part| !part.is_empty())
        .unwrap_or(default_lang);
    crate::services::task_path::sanitize_filename_component(prefix)
}

fn ensure_unique_output_path(dir: &std::path::Path, file_name: &str) -> PathBuf {
    let initial = dir.join(file_name);
    if !initial.exists() {
        return initial;
    }
    let stem = std::path::Path::new(file_name)
        .file_stem()
        .map(|v| v.to_string_lossy().to_string())
        .unwrap_or_else(|| "subtitle".to_string());
    let ext = std::path::Path::new(file_name)
        .extension()
        .map(|v| v.to_string_lossy().to_string())
        .unwrap_or_else(|| "srt".to_string());
    for seq in 2..=9_999 {
        let candidate = dir.join(format!("{stem}_{seq}.{ext}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    initial
}
