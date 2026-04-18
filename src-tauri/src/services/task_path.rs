use std::path::{Path, PathBuf};

pub const ARTIFACTS_DIR_NAME: &str = "artifacts";

pub fn sanitize_filename_component(raw: &str) -> String {
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

pub fn task_output_dir(task_id: &str, audio_path: &Path) -> PathBuf {
    if is_virtual_media_path(audio_path) {
        if let Some(existing_dir) = find_existing_task_dir_by_id(task_id) {
            return existing_dir;
        }
        return task_output_dir_by_id(task_id);
    }
    if let Some(existing_dir) = media_parent_under_output(audio_path) {
        if let Some(task_dir) = find_existing_task_dir_by_id(task_id) {
            return task_dir;
        }
        return existing_dir;
    }
    let stem = audio_path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "transcript".to_string());
    let safe_stem = sanitize_filename_component(&stem);
    let safe_task_id = sanitize_filename_component(task_id);
    crate::services::output::resolve_output_dir().join(format!("{}_{}", safe_stem, safe_task_id))
}

pub fn task_output_dir_by_id(task_id: &str) -> PathBuf {
    let safe_task_id = sanitize_filename_component(task_id);
    crate::services::output::resolve_output_dir().join(safe_task_id)
}

pub fn task_srt_output_path(task_id: &str, audio_path: &Path) -> PathBuf {
    task_src_srt_output_path(task_id, audio_path)
}

pub fn task_artifacts_dir(task_id: &str, audio_path: &Path) -> PathBuf {
    task_output_dir(task_id, audio_path).join(ARTIFACTS_DIR_NAME)
}

pub fn task_artifacts_dir_by_id(task_id: &str) -> PathBuf {
    task_output_dir_by_id(task_id).join(ARTIFACTS_DIR_NAME)
}

pub fn task_src_srt_output_path(task_id: &str, audio_path: &Path) -> PathBuf {
    task_output_dir(task_id, audio_path).join("src.srt")
}

pub fn task_trans_srt_output_path(task_id: &str, audio_path: &Path) -> PathBuf {
    task_output_dir(task_id, audio_path).join("trans.srt")
}

pub fn task_src_trans_srt_output_path(task_id: &str, audio_path: &Path) -> PathBuf {
    task_output_dir(task_id, audio_path).join("src_trans.srt")
}

pub fn task_trans_src_srt_output_path(task_id: &str, audio_path: &Path) -> PathBuf {
    task_output_dir(task_id, audio_path).join("trans_src.srt")
}

pub fn task_log_dir(task_id: &str, media_path: Option<&Path>) -> PathBuf {
    let artifacts_root = match media_path {
        Some(path) => task_artifacts_dir(task_id, path),
        None => task_artifacts_dir_by_id(task_id),
    };
    artifacts_root.join("logs")
}

fn media_parent_under_output(audio_path: &Path) -> Option<PathBuf> {
    let parent = audio_path.parent()?.to_path_buf();
    let output_root = crate::services::output::resolve_output_dir();
    if parent.starts_with(&output_root) {
        Some(parent)
    } else {
        None
    }
}

fn is_virtual_media_path(audio_path: &Path) -> bool {
    let raw = audio_path.to_string_lossy();
    raw.starts_with("youtube://")
}

fn find_existing_task_dir_by_id(task_id: &str) -> Option<PathBuf> {
    let safe_task_id = sanitize_filename_component(task_id);
    if safe_task_id.is_empty() {
        return None;
    }
    let suffix = format!("_{safe_task_id}");
    let output_root = crate::services::output::resolve_output_dir();
    let mut candidates: Vec<PathBuf> = std::fs::read_dir(&output_root)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            if !path.is_dir() {
                return false;
            }
            let Some(name) = path.file_name().and_then(|v| v.to_str()) else {
                return false;
            };
            name.ends_with(&suffix)
        })
        .collect();
    candidates.sort();
    candidates.pop()
}
