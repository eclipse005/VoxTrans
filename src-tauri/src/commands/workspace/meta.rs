use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::store::{TaskStore, lock_workspace_hydrated, lock_workspace_store};
use super::{
    TASK_META_FILE_NAME, WorkspaceQueueItem, WorkspaceTaskProgressState, WorkspaceTaskRecord,
    normalize_intent, normalize_task_source_lang, normalize_task_target_lang,
};
use crate::commands::workspace::json_files::{read_json_file_if_exists, write_json_file};
use crate::domain::error::WorkspaceResult;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceTaskMetaArtifact {
    #[serde(default = "task_meta_version")]
    version: u32,
    item: WorkspaceQueueItem,
    #[serde(default)]
    intent: String,
    #[serde(default)]
    source_lang: String,
    #[serde(default)]
    target_lang: String,
    #[serde(default)]
    max_retries: u32,
    #[serde(default)]
    settings_snapshot: Value,
    #[serde(default)]
    updated_at_ms: u64,
}

pub(super) fn ensure_workspace_hydrated_from_disk() -> WorkspaceResult<()> {
    {
        let hydrated = lock_workspace_hydrated()?;
        if *hydrated {
            return Ok(());
        }
    }

    hydrate_workspace_from_disk()?;
    let mut hydrated = lock_workspace_hydrated()?;
    *hydrated = true;
    Ok(())
}

pub(super) fn persist_task_meta(record: &WorkspaceTaskRecord) -> WorkspaceResult<()> {
    let meta_path = task_meta_path_for_item(&record.item);
    let artifact = workspace_meta_from_record(record);
    write_json_file(&meta_path, &artifact)
}

pub(super) fn remove_task_meta(item: &WorkspaceQueueItem) -> WorkspaceResult<()> {
    let meta_path = task_meta_path_for_item(item);
    match std::fs::remove_file(meta_path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err.into()),
    }
}

fn hydrate_workspace_from_disk() -> WorkspaceResult<()> {
    let restored = load_task_meta_artifacts()?;
    if restored.is_empty() {
        return Ok(());
    }

    let mut store = lock_workspace_store()?;
    for artifact in restored {
        let record = workspace_record_from_meta(artifact);
        if store
            .tasks()
            .iter()
            .any(|task| task.item.id == record.item.id)
        {
            continue;
        }
        store.push_task(record);
    }
    Ok(())
}

fn load_task_meta_artifacts() -> WorkspaceResult<Vec<WorkspaceTaskMetaArtifact>> {
    let output_dir = crate::services::output::resolve_output_dir();
    load_task_meta_artifacts_from_output_dir(&output_dir)
}

fn load_task_meta_artifacts_from_output_dir(
    output_dir: &Path,
) -> WorkspaceResult<Vec<WorkspaceTaskMetaArtifact>> {
    if !output_root_is_existing_dir(output_dir)? {
        return Ok(Vec::new());
    }

    let entries = std::fs::read_dir(output_dir)?;
    load_task_meta_artifacts_from_task_dirs(entries.map(|entry| entry.map(|entry| entry.path())))
}

fn load_task_meta_artifacts_from_task_dirs<I>(
    task_dirs: I,
) -> WorkspaceResult<Vec<WorkspaceTaskMetaArtifact>>
where
    I: IntoIterator<Item = std::io::Result<PathBuf>>,
{
    let mut out = Vec::<WorkspaceTaskMetaArtifact>::new();
    for task_dir in task_dirs {
        let path = task_dir?;
        if !path_is_existing_dir(&path)? {
            continue;
        }
        let Some(mut artifact) = load_task_meta_artifact_from_task_dir(&path)? else {
            continue;
        };
        if artifact.item.transcribe_status == "processing" {
            artifact.item.transcribe_status = "error".to_string();
            artifact.item.task_progress = WorkspaceTaskProgressState::default();
            artifact.item.transcribe_error = "任务在运行中被中断，请重新开始".to_string();
        }
        out.push(artifact);
    }
    out.sort_by(|a, b| b.updated_at_ms.cmp(&a.updated_at_ms));
    Ok(out)
}

fn load_task_meta_artifact_from_task_dir(
    task_dir: &Path,
) -> WorkspaceResult<Option<WorkspaceTaskMetaArtifact>> {
    let artifact_meta_path = task_dir
        .join(crate::services::task_path::ARTIFACTS_DIR_NAME)
        .join(TASK_META_FILE_NAME);
    read_json_file_if_exists::<WorkspaceTaskMetaArtifact>(&artifact_meta_path)
}

fn path_is_existing_dir(path: &Path) -> WorkspaceResult<bool> {
    match std::fs::metadata(path) {
        Ok(metadata) => Ok(metadata.is_dir()),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err.into()),
    }
}

fn output_root_is_existing_dir(path: &Path) -> WorkspaceResult<bool> {
    match std::fs::metadata(path) {
        Ok(metadata) if metadata.is_dir() => Ok(true),
        Ok(_) => Err(std::io::Error::new(
            ErrorKind::InvalidInput,
            format!("output path is not a directory: {}", path.display()),
        )
        .into()),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err.into()),
    }
}

fn workspace_record_from_meta(artifact: WorkspaceTaskMetaArtifact) -> WorkspaceTaskRecord {
    let mut item = artifact.item;
    let source_lang = if artifact.source_lang.trim().is_empty() {
        normalize_task_source_lang(&item.source_lang)
    } else {
        normalize_task_source_lang(&artifact.source_lang)
    };
    let target_lang = if artifact.target_lang.trim().is_empty() {
        normalize_task_target_lang(&item.target_lang)
    } else {
        normalize_task_target_lang(&artifact.target_lang)
    };
    item.source_lang = source_lang.clone();
    item.target_lang = target_lang.clone();

    WorkspaceTaskRecord {
        item,
        intent: normalize_intent(&artifact.intent).to_string(),
        source_lang,
        target_lang,
        max_retries: artifact.max_retries,
        settings_snapshot: artifact.settings_snapshot,
    }
}

fn workspace_meta_from_record(record: &WorkspaceTaskRecord) -> WorkspaceTaskMetaArtifact {
    WorkspaceTaskMetaArtifact {
        version: task_meta_version(),
        item: record.item.clone(),
        intent: record.intent.clone(),
        source_lang: record.source_lang.clone(),
        target_lang: record.target_lang.clone(),
        max_retries: record.max_retries,
        settings_snapshot: record.settings_snapshot.clone(),
        updated_at_ms: now_millis(),
    }
}

fn task_meta_version() -> u32 {
    2
}

fn task_meta_path_for_item(item: &WorkspaceQueueItem) -> PathBuf {
    task_artifact_dir_for_item(item).join(TASK_META_FILE_NAME)
}

fn task_artifact_dir_for_item(item: &WorkspaceQueueItem) -> PathBuf {
    let path = item.path.trim();
    if path.is_empty() {
        crate::services::task_path::task_artifacts_dir_by_id(&item.id)
    } else {
        crate::services::task_path::task_artifacts_dir(&item.id, Path::new(path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_meta_loader_reads_artifacts_meta_without_touching_legacy_meta() {
        let task_dir =
            std::env::temp_dir().join(format!("voxtrans-meta-loader-{}", std::process::id()));
        let artifacts_dir = task_dir.join(crate::services::task_path::ARTIFACTS_DIR_NAME);
        std::fs::create_dir_all(&artifacts_dir).expect("create artifacts dir");

        let artifact = test_meta_artifact("task-artifacts");
        let artifact_path = artifacts_dir.join(TASK_META_FILE_NAME);
        std::fs::write(
            &artifact_path,
            serde_json::to_string(&artifact).expect("serialize artifact"),
        )
        .expect("write artifact meta");
        std::fs::write(task_dir.join(TASK_META_FILE_NAME), "{invalid legacy json")
            .expect("write invalid legacy meta");

        let loaded = load_task_meta_artifact_from_task_dir(&task_dir)
            .expect("artifacts meta should load")
            .expect("artifacts meta should exist");

        let _ = std::fs::remove_dir_all(task_dir);
        assert_eq!(loaded.item.id, "task-artifacts");
    }

    #[test]
    fn task_meta_loader_reports_task_dir_iteration_error() {
        let err = load_task_meta_artifacts_from_task_dirs([Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "cannot read task dir",
        ))])
        .expect_err("task dir iteration errors should surface");

        assert_eq!(err.code(), "IO_ERROR");
    }

    #[test]
    fn task_meta_loader_reports_task_dir_metadata_error() {
        let err = load_task_meta_artifacts_from_task_dirs([Ok(invalid_metadata_path())])
            .expect_err("task dir metadata errors should surface");

        assert_eq!(err.code(), "IO_ERROR");
    }

    #[test]
    fn task_meta_loader_reports_output_dir_metadata_error() {
        let err = load_task_meta_artifacts_from_output_dir(&invalid_metadata_path())
            .expect_err("output dir metadata errors should surface");

        assert_eq!(err.code(), "IO_ERROR");
    }

    #[test]
    fn task_meta_loader_reports_output_root_file_as_io_error() {
        let path =
            std::env::temp_dir().join(format!("voxtrans-output-root-file-{}", std::process::id()));
        std::fs::write(&path, "not a directory").expect("write output root file");

        let err = load_task_meta_artifacts_from_output_dir(&path)
            .expect_err("output root files should surface as IO errors");

        let _ = std::fs::remove_file(path);
        assert_eq!(err.code(), "IO_ERROR");
    }

    fn invalid_metadata_path() -> PathBuf {
        PathBuf::from(format!("bad{}path", '\0'))
    }

    fn test_meta_artifact(task_id: &str) -> WorkspaceTaskMetaArtifact {
        WorkspaceTaskMetaArtifact {
            version: task_meta_version(),
            item: WorkspaceQueueItem {
                id: task_id.to_string(),
                path: String::new(),
                name: "demo.mp4".to_string(),
                media_kind: "video".to_string(),
                size_bytes: 1,
                source_lang: "en".to_string(),
                target_lang: "zh-CN".to_string(),
                transcribe_status: "pending".to_string(),
                task_progress: WorkspaceTaskProgressState::default(),
                transcribe_error: String::new(),
                result_text: String::new(),
                result_srt: String::new(),
                subtitle_segments_json: "[]".to_string(),
                llm_total_tokens: 0,
            },
            intent: "TRANSCRIBE".to_string(),
            source_lang: "en".to_string(),
            target_lang: "zh-CN".to_string(),
            max_retries: 0,
            settings_snapshot: Value::Null,
            updated_at_ms: 1,
        }
    }
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}
