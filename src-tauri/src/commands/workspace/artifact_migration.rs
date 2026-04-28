use std::path::Path;

use super::{
    STEP_01_ASR_FILE, STEP_02_SEGMENTS_FILE, STEP_03_TERMINOLOGY_FILE, STEP_04_TRANSLATION_FILE,
    STEP_05_01_SOURCE_SPLIT_FILE, STEP_05_02_TRANSLATION_ALIGN_FILE,
    STEP_05_03_TRANSLATION_POLISH_FILE, STEP_06_FINAL_CHECK_FILE, TASK_META_FILE_NAME,
};

pub(super) fn migrate_legacy_artifacts(
    task_output_dir: &Path,
    artifact_dir: &Path,
) -> Result<(), String> {
    if !task_output_dir.exists() {
        return Ok(());
    }
    std::fs::create_dir_all(artifact_dir).map_err(|err| err.to_string())?;
    migrate_legacy_logs_dir(task_output_dir, artifact_dir)?;
    let entries = std::fs::read_dir(task_output_dir).map_err(|err| err.to_string())?;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|v| v.to_str()) else {
            continue;
        };
        let Some(target_name) = migrate_target_artifact_name(name) else {
            continue;
        };
        let target_path = artifact_dir.join(target_name);
        if target_path.exists() {
            let _ = std::fs::remove_file(&path);
            continue;
        }
        std::fs::rename(&path, &target_path)
            .or_else(|_| std::fs::copy(&path, &target_path).map(|_| ()))
            .map_err(|err| err.to_string())?;
        let _ = std::fs::remove_file(&path);
    }
    Ok(())
}

fn migrate_legacy_logs_dir(task_output_dir: &Path, artifact_dir: &Path) -> Result<(), String> {
    let legacy_log_dir = task_output_dir.join("logs");
    if !legacy_log_dir.is_dir() {
        return Ok(());
    }
    let target_log_dir = artifact_dir.join("logs");
    if std::fs::rename(&legacy_log_dir, &target_log_dir).is_ok() {
        return Ok(());
    }
    move_directory_contents(&legacy_log_dir, &target_log_dir)?;
    let _ = std::fs::remove_dir_all(&legacy_log_dir);
    Ok(())
}

fn move_directory_contents(source_dir: &Path, target_dir: &Path) -> Result<(), String> {
    if !source_dir.is_dir() {
        return Ok(());
    }
    std::fs::create_dir_all(target_dir).map_err(|err| err.to_string())?;
    let entries = std::fs::read_dir(source_dir).map_err(|err| err.to_string())?;
    for entry in entries.flatten() {
        let source_path = entry.path();
        let target_path = target_dir.join(entry.file_name());
        if source_path.is_dir() {
            move_directory_contents(&source_path, &target_path)?;
            let _ = std::fs::remove_dir(&source_path);
            continue;
        }
        if !source_path.is_file() {
            continue;
        }
        if target_path.exists() {
            let _ = std::fs::remove_file(&source_path);
            continue;
        }
        std::fs::rename(&source_path, &target_path)
            .or_else(|_| std::fs::copy(&source_path, &target_path).map(|_| ()))
            .map_err(|err| err.to_string())?;
        let _ = std::fs::remove_file(&source_path);
    }
    Ok(())
}

fn migrate_target_artifact_name(name: &str) -> Option<&'static str> {
    match name {
        "step_01_asr.json" => Some(STEP_01_ASR_FILE),
        "step_02_segments.json" => Some(STEP_02_SEGMENTS_FILE),
        "step_03_terminology.json" => Some(STEP_03_TERMINOLOGY_FILE),
        "step_04_translation.json" => Some(STEP_04_TRANSLATION_FILE),
        "step_05_01_source_split.json" => Some(STEP_05_01_SOURCE_SPLIT_FILE),
        "step_05_02_translation_align.json" => Some(STEP_05_02_TRANSLATION_ALIGN_FILE),
        "step_05_03_translation_polish.json" => Some(STEP_05_03_TRANSLATION_POLISH_FILE),
        "step_06_final_check.json" => Some(STEP_06_FINAL_CHECK_FILE),
        "gpt.log" => Some("gpt.log"),
        "task_meta.json" => Some(TASK_META_FILE_NAME),
        _ => None,
    }
}
