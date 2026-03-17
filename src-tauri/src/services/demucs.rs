use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::PathBuf;

use crate::services::task_log::TaskLogger;

mod audio_prep;
mod output_resolve;
mod process_runner;
mod progress_parse;

use audio_prep::prepare_demucs_input;
use output_resolve::find_vocals_path;
use process_runner::run_demucs_with_progress;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeparateVocalsRequest {
    pub task_id: String,
    pub audio_path: String,
    pub model: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SeparateVocalsResponse {
    pub vocals_path: String,
}

pub fn separate_vocals_blocking<F>(
    request: SeparateVocalsRequest,
    mut on_progress: F,
) -> Result<SeparateVocalsResponse, String>
where
    F: FnMut(u32),
{
    let started_at = std::time::Instant::now();
    let logger = TaskLogger::main(request.task_id.clone());
    let demucs_model_dir = crate::services::model::resolve_engine_model_dir(
        crate::services::model::ModelTarget::Demucs,
    );
    let demucs_model_file = demucs_model_dir.join(format!("{}.safetensors", request.model));
    if !demucs_model_file.is_file() {
        let err = format!(
            "人声分离模型未就绪: {}。请先到模型中心下载后再试。",
            demucs_model_file.display()
        );
        logger.event(
            "demucs.failed",
            Some(&json!({
                "error": err,
                "elapsedSec": round2(started_at.elapsed().as_secs_f64()),
            })),
        );
        return Err(err);
    }

    let input_path = PathBuf::from(&request.audio_path);
    let output_root = crate::services::task_path::task_output_dir(&request.task_id, &input_path)
        .join("demucs")
        .join(&request.model);
    std::fs::create_dir_all(&output_root).map_err(|err| err.to_string())?;

    let demucs_input = match prepare_demucs_input(&input_path, &output_root) {
        Ok(path) => path,
        Err(err) => {
            logger.event(
                "demucs.failed",
                Some(&json!({
                    "error": err,
                    "elapsedSec": round2(started_at.elapsed().as_secs_f64()),
                })),
            );
            return Err(err);
        }
    };

    logger.event(
        "demucs.started",
        Some(&json!({
            "model": request.model,
            "inputPath": input_path.display().to_string(),
            "demucsInputPath": demucs_input.display().to_string(),
        })),
    );

    if let Err(err) = run_demucs_with_progress(
        &request.model,
        &demucs_model_dir,
        &output_root,
        &demucs_input,
        |percent| on_progress(percent),
    ) {
        logger.event(
            "demucs.failed",
            Some(&json!({
                "error": err,
                "elapsedSec": round2(started_at.elapsed().as_secs_f64()),
            })),
        );
        return Err(err);
    }

    let vocals_path = find_vocals_path(&output_root, &demucs_input)
        .ok_or_else(|| format!("未找到 vocals.wav 输出: {}", output_root.display()))?;
    logger.event(
        "demucs.completed",
        Some(&json!({
            "vocalsPath": vocals_path.display().to_string(),
            "elapsedSec": round2(started_at.elapsed().as_secs_f64()),
        })),
    );
    Ok(SeparateVocalsResponse {
        vocals_path: vocals_path.display().to_string(),
    })
}

fn round2(value: f64) -> f64 {
    if !value.is_finite() {
        return 0.0;
    }
    (value * 100.0).round() / 100.0
}
