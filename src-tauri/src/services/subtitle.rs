use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use voxtrans_core::subtitle::srt::{normalize_cues, parse_srt, to_srt_from_cues, validate_cues};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleLoadRequest {
    pub task_id: String,
    pub media_path: String,
    pub fallback_srt: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleLoadResponse {
    pub srt_path: String,
    pub draft_path: String,
    pub content: String,
    pub using_draft: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleSaveRequest {
    pub task_id: String,
    pub media_path: String,
    pub content: String,
    pub autosave: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleSaveResponse {
    pub srt_path: String,
    pub warnings: Vec<String>,
}

pub fn load_subtitle_editor(request: SubtitleLoadRequest) -> Result<SubtitleLoadResponse, String> {
    let media_path = PathBuf::from(&request.media_path);
    let srt_path = crate::services::task_path::task_srt_output_path(&request.task_id, &media_path);
    let draft_path = crate::services::task_path::task_srt_draft_path(&request.task_id, &media_path);

    if let Some(parent) = srt_path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    if let Some(parent) = draft_path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }

    let persisted = std::fs::read_to_string(&srt_path).ok();
    let fallback = request.fallback_srt.filter(|s| !s.trim().is_empty());

    let mut content = persisted
        .or(fallback)
        .unwrap_or_default()
        .replace("\r\n", "\n");

    let mut using_draft = false;
    if let Ok(draft_content) = std::fs::read_to_string(&draft_path) {
        let should_use_draft = if let (Ok(draft_meta), Ok(srt_meta)) =
            (std::fs::metadata(&draft_path), std::fs::metadata(&srt_path))
        {
            draft_meta.modified().ok() > srt_meta.modified().ok()
        } else {
            true
        };

        if should_use_draft && !draft_content.trim().is_empty() {
            content = draft_content.replace("\r\n", "\n");
            using_draft = true;
        }
    }

    let warnings = match parse_srt(&content) {
        Ok(cues) => validate_cues(&normalize_cues(&cues)),
        Err(_err) if content.trim().is_empty() => Vec::new(),
        Err(err) => vec![format!("parse warning: {}", err)],
    };

    Ok(SubtitleLoadResponse {
        srt_path: srt_path.display().to_string(),
        draft_path: draft_path.display().to_string(),
        content,
        using_draft,
        warnings,
    })
}

pub fn save_subtitle_editor(request: SubtitleSaveRequest) -> Result<SubtitleSaveResponse, String> {
    let media_path = PathBuf::from(&request.media_path);
    let srt_path = crate::services::task_path::task_srt_output_path(&request.task_id, &media_path);
    let draft_path = crate::services::task_path::task_srt_draft_path(&request.task_id, &media_path);

    let parsed = parse_srt(&request.content).map_err(|err| err.to_string())?;
    let normalized = normalize_cues(&parsed);
    let warnings = validate_cues(&normalized);
    let normalized_srt = to_srt_from_cues(&normalized);

    if request.autosave {
        if let Some(parent) = draft_path.parent() {
            std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        }
        std::fs::write(&draft_path, normalized_srt).map_err(|err| err.to_string())?;
    } else {
        if let Some(parent) = srt_path.parent() {
            std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        }
        std::fs::write(&srt_path, normalized_srt).map_err(|err| err.to_string())?;
        let _ = std::fs::remove_file(&draft_path);
    }

    Ok(SubtitleSaveResponse {
        srt_path: srt_path.display().to_string(),
        warnings,
    })
}
