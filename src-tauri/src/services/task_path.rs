use std::path::{Path, PathBuf};

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
    task_srt_output_path_for_lang(task_id, audio_path, "en")
}

pub fn task_srt_output_path_for_lang(task_id: &str, audio_path: &Path, lang: &str) -> PathBuf {
    let stem = audio_path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "transcript".to_string());
    let safe_stem = sanitize_filename_component(&stem);
    let lang_suffix = normalize_lang_suffix(lang);
    task_output_dir(task_id, audio_path).join(format!("{}_{}.srt", safe_stem, lang_suffix))
}

pub fn task_words_output_path(task_id: &str, audio_path: &Path) -> PathBuf {
    let stem = audio_path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "transcript".to_string());
    let safe_stem = sanitize_filename_component(&stem);
    task_output_dir(task_id, audio_path).join(format!("{}_words.json", safe_stem))
}

pub fn task_srt_draft_path(task_id: &str, audio_path: &Path) -> PathBuf {
    let stem = audio_path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "transcript".to_string());
    let safe_stem = sanitize_filename_component(&stem);
    task_output_dir(task_id, audio_path).join(format!("{}_en.draft.srt", safe_stem))
}

fn normalize_lang_suffix(lang: &str) -> String {
    let lowered = lang.trim().to_lowercase();
    if lowered.is_empty() {
        return "en".to_string();
    }
    if lowered.starts_with("zh") {
        return "zh".to_string();
    }
    let prefix = lowered
        .split(|ch: char| ch == '-' || ch == '_' || !ch.is_ascii_alphanumeric())
        .find(|part| !part.is_empty())
        .unwrap_or("en");
    sanitize_filename_component(prefix)
}

pub fn task_log_dir(task_id: &str, media_path: Option<&Path>) -> PathBuf {
    let task_root = match media_path {
        Some(path) => task_output_dir(task_id, path),
        None => task_output_dir_by_id(task_id),
    };
    task_root.join("logs")
}
