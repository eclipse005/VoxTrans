use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{
    TASK_META_FILE_NAME, WorkspaceQueueItem, WorkspaceTaskProgressState, WorkspaceTaskRecord,
    lock_workspace_hydrated, lock_workspace_store, normalize_intent, normalize_task_source_lang,
    normalize_task_target_lang,
};
use crate::commands::workspace::json_files::{read_json_file_if_exists, write_json_file};

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

pub(super) fn ensure_workspace_hydrated_from_disk() -> Result<(), String> {
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

pub(super) fn persist_task_meta(record: &WorkspaceTaskRecord) -> Result<(), String> {
    let meta_path = task_meta_path_for_item(&record.item);
    let artifact = workspace_meta_from_record(record);
    write_json_file(&meta_path, &artifact)
}

pub(super) fn remove_task_meta(item: &WorkspaceQueueItem) {
    let meta_path = task_meta_path_for_item(item);
    let _ = std::fs::remove_file(meta_path);
    let legacy_meta_path = task_output_dir_for_item(item).join(TASK_META_FILE_NAME);
    let _ = std::fs::remove_file(legacy_meta_path);
}

fn hydrate_workspace_from_disk() -> Result<(), String> {
    let restored = load_task_meta_artifacts()?;
    if restored.is_empty() {
        return Ok(());
    }

    let mut store = lock_workspace_store()?;
    for artifact in restored {
        let record = workspace_record_from_meta(artifact);
        if store
            .tasks
            .iter()
            .any(|task| task.item.id == record.item.id)
        {
            continue;
        }
        store.tasks.push(record);
    }
    Ok(())
}

fn load_task_meta_artifacts() -> Result<Vec<WorkspaceTaskMetaArtifact>, String> {
    let output_dir = crate::services::output::resolve_output_dir();
    if !output_dir.exists() {
        return Ok(Vec::new());
    }

    let mut out = Vec::<WorkspaceTaskMetaArtifact>::new();
    let entries = std::fs::read_dir(&output_dir).map_err(|err| err.to_string())?;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let artifact_meta_path = path
            .join(crate::services::task_path::ARTIFACTS_DIR_NAME)
            .join(TASK_META_FILE_NAME);
        let legacy_meta_path = path.join(TASK_META_FILE_NAME);
        let Some(mut artifact) =
            read_json_file_if_exists::<WorkspaceTaskMetaArtifact>(&artifact_meta_path)?.or(
                read_json_file_if_exists::<WorkspaceTaskMetaArtifact>(&legacy_meta_path)?,
            )
        else {
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

fn task_output_dir_for_item(item: &WorkspaceQueueItem) -> PathBuf {
    let path = item.path.trim();
    if path.is_empty() {
        crate::services::task_path::task_output_dir_by_id(&item.id)
    } else {
        crate::services::task_path::task_output_dir(&item.id, Path::new(path))
    }
}

fn task_artifact_dir_for_item(item: &WorkspaceQueueItem) -> PathBuf {
    let path = item.path.trim();
    if path.is_empty() {
        crate::services::task_path::task_artifacts_dir_by_id(&item.id)
    } else {
        crate::services::task_path::task_artifacts_dir(&item.id, Path::new(path))
    }
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}
