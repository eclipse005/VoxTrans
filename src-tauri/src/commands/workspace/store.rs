use std::sync::{Mutex, OnceLock};

use crate::domain::error::{WorkspaceError, WorkspaceResult};

use super::WorkspaceTaskRecord;

pub(super) trait TaskStore {
    fn tasks(&self) -> &[WorkspaceTaskRecord];

    fn tasks_mut(&mut self) -> &mut Vec<WorkspaceTaskRecord>;

    fn find_task_mut(&mut self, task_id: &str) -> Option<&mut WorkspaceTaskRecord> {
        self.tasks_mut()
            .iter_mut()
            .find(|entry| entry.item.id == task_id)
    }

    fn push_task(&mut self, record: WorkspaceTaskRecord) {
        self.tasks_mut().push(record);
    }

    fn retain_tasks(&mut self, mut keep: impl FnMut(&WorkspaceTaskRecord) -> bool) {
        self.tasks_mut().retain(|task| keep(task));
    }
}

#[derive(Debug, Default)]
pub(super) struct WorkspaceStore {
    tasks: Vec<WorkspaceTaskRecord>,
}

impl TaskStore for WorkspaceStore {
    fn tasks(&self) -> &[WorkspaceTaskRecord] {
        &self.tasks
    }

    fn tasks_mut(&mut self) -> &mut Vec<WorkspaceTaskRecord> {
        &mut self.tasks
    }
}

static WORKSPACE_STORE: OnceLock<Mutex<WorkspaceStore>> = OnceLock::new();
static WORKSPACE_HYDRATED: OnceLock<Mutex<bool>> = OnceLock::new();

type WorkspaceStoreGuard = std::sync::MutexGuard<'static, WorkspaceStore>;
type WorkspaceHydratedGuard = std::sync::MutexGuard<'static, bool>;

fn workspace_store() -> &'static Mutex<WorkspaceStore> {
    WORKSPACE_STORE.get_or_init(|| Mutex::new(WorkspaceStore::default()))
}

pub(super) fn lock_workspace_store() -> WorkspaceResult<WorkspaceStoreGuard> {
    workspace_store()
        .lock()
        .map_err(|_| WorkspaceError::LockPoisoned)
}

pub(super) fn lock_workspace_hydrated() -> WorkspaceResult<WorkspaceHydratedGuard> {
    WORKSPACE_HYDRATED
        .get_or_init(|| Mutex::new(false))
        .lock()
        .map_err(|_| WorkspaceError::LockPoisoned)
}

pub(super) fn find_task_mut<'a>(
    store: &'a mut WorkspaceStore,
    task_id: &str,
) -> Option<&'a mut WorkspaceTaskRecord> {
    store.find_task_mut(task_id)
}
