use tauri::{AppHandle, Manager};

use super::store::{TaskStore as _, lock_workspace_hydrated, lock_workspace_store};
use super::{WorkspaceQueueItem, WorkspaceTaskRecord};
use crate::db::conversion::TaskMetaExtras;
use crate::db::store::TaskStore;
use crate::domain::error::{WorkspaceError, WorkspaceResult};
use crate::domain::task::runtime_settings::FrozenSettings;
use crate::services::workspace_subtitle::serialize_segments;

pub(super) async fn ensure_workspace_hydrated_from_db(app: &AppHandle) -> WorkspaceResult<()> {
    let store = app.state::<TaskStore>().inner().clone();
    ensure_workspace_hydrated_from_store(&store).await
}

pub(super) async fn ensure_workspace_hydrated_from_store(store: &TaskStore) -> WorkspaceResult<()> {
    {
        let hydrated = lock_workspace_hydrated()?;
        if *hydrated {
            return Ok(());
        }
    }

    store
        .recover_orphan_processing()
        .await
        .map_err(|e| WorkspaceError::TaskFailed(format!("recover orphans: {e}")))?;
    hydrate_workspace_from_db(store).await?;
    let mut hydrated = lock_workspace_hydrated()?;
    *hydrated = true;
    Ok(())
}

pub(super) async fn persist_task_meta(
    store: &TaskStore,
    record: &WorkspaceTaskRecord,
) -> WorkspaceResult<()> {
    let terminology_groups_json = serde_json::to_string(&record.frozen.terminology_groups)
        .unwrap_or_else(|_| "[]".to_string());
    let extras = TaskMetaExtras {
        intent: record.intent.clone(),
        max_retries: record.max_retries,
        subtitle_length_preset: record.frozen.subtitle_length_preset.as_str().to_string(),
        enable_subtitle_beautify: record.frozen.enable_subtitle_beautify,
        terminology_groups_json,
        enqueue_seq: record.enqueue_seq,
    };
    store
        .upsert_task(&record.item, &extras)
        .await
        .map_err(|e| WorkspaceError::TaskFailed(format!("persist task: {e}")))
}

pub(super) async fn remove_task_meta(
    store: &TaskStore,
    item: &WorkspaceQueueItem,
) -> WorkspaceResult<()> {
    store
        .delete_task(&item.id)
        .await
        .map_err(|e| WorkspaceError::TaskFailed(format!("delete task: {e}")))
}

async fn hydrate_workspace_from_db(store: &TaskStore) -> WorkspaceResult<()> {
    let mut loaded = store
        .load_all_tasks()
        .await
        .map_err(|e| WorkspaceError::TaskFailed(format!("load tasks: {e}")))?;

    let mut records = Vec::with_capacity(loaded.len());
    for (mut item, extras) in loaded.drain(..) {
        let segments = store
            .load_segments(&item.id)
            .await
            .map_err(|e| WorkspaceError::TaskFailed(format!("load segments {}: {e}", item.id)))?;
        let source_lang = item.source_lang.clone();
        let target_lang = item.target_lang.clone();
        item.subtitle_segments_json = serialize_segments(&segments);
        let frozen = FrozenSettings {
            subtitle_length_preset: crate::services::preferences_types::SubtitleLengthPreset::parse(&extras.subtitle_length_preset),
            enable_subtitle_beautify: extras.enable_subtitle_beautify,
            terminology_groups: serde_json::from_str(&extras.terminology_groups_json)
                .unwrap_or_default(),
        };
        records.push(WorkspaceTaskRecord {
            item,
            intent: extras.intent,
            source_lang,
            target_lang,
            max_retries: extras.max_retries,
            frozen,
            enqueue_seq: extras.enqueue_seq,
        });
    }

    let mut guard = lock_workspace_store()?;
    guard.tasks_mut().clear();
    for record in records {
        guard.push_task(record);
    }
    Ok(())
}
