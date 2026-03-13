use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveSrtRequest {
    pub output_path: String,
    pub content: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskTranslatedSrtPathRequest {
    pub task_id: String,
    pub media_path: String,
    pub target_language: String,
}

pub fn save_srt(request: SaveSrtRequest) -> Result<(), String> {
    if let Some(parent) = std::path::Path::new(&request.output_path).parent() {
        std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    std::fs::write(&request.output_path, request.content).map_err(|err| err.to_string())
}

pub fn get_file_size(path: String) -> Result<u64, String> {
    let metadata = std::fs::metadata(&path).map_err(|err| err.to_string())?;
    Ok(metadata.len())
}

pub fn get_task_translated_srt_path(request: TaskTranslatedSrtPathRequest) -> Result<String, String> {
    if request.task_id.trim().is_empty() {
        return Err("taskId is required".to_string());
    }
    if request.media_path.trim().is_empty() {
        return Err("mediaPath is required".to_string());
    }
    let media_path = PathBuf::from(request.media_path);
    let path = crate::services::task_path::task_translated_srt_output_path(
        &request.task_id,
        &media_path,
        &request.target_language,
    );
    Ok(path.display().to_string())
}
