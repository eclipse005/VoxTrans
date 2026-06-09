use crate::db::store::TaskStore;
use crate::services::task_log::{TaskLogger, event};
use serde::Deserialize;
use serde_json::json;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportTaskSrtsRequest {
    pub task_id: String,
    pub target_dir: String,
    #[serde(default)]
    pub task_name: Option<String>,
    pub items: Vec<crate::services::subtitle_srt::ExportSrtItem>,
}

pub fn export_task_srts(store: &TaskStore, request: ExportTaskSrtsRequest) -> Result<Vec<String>, String> {
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
        return Err(format!("导出目录不存在: {}", target_dir));
    }

    let task_item = crate::commands::workspace::get_task_queue_item_for_export(&request.task_id)?;
    let (task_enable_subtitle_beautify, subtitle_length_preset, target_lang) =
        crate::commands::workspace::task_subtitle_beautify_context(store, &request.task_id)?;
    let segments =
        crate::services::subtitle_srt::parse_segments_json(&task_item.subtitle_segments_json)
            .map_err(|err| format!("字幕片段解析失败: {err}"))?;
    if segments.is_empty() {
        return Err("当前任务没有可导出的字幕片段".to_string());
    }
    let mut segments = segments;
    if task_enable_subtitle_beautify {
        crate::services::subtitle_beautify::beautify_subtitle_srt_segments(
            &mut segments,
            &subtitle_length_preset,
            &target_lang,
        );
    }

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
