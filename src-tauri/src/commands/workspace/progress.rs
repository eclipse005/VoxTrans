use tauri::AppHandle;

use crate::domain::error::{WorkspaceError, WorkspaceResult};

use super::{
    TaskStage, WorkspaceTaskProgressState, WorkspaceTaskStageState, lock_workspace_store,
    patch_task_item,
};
use super::store::TaskStore as _;

pub(super) fn task_progress_state(
    stage: TaskStage,
    detail: impl Into<String>,
    current: u32,
    total: u32,
) -> WorkspaceTaskProgressState {
    WorkspaceTaskProgressState {
        stage: WorkspaceTaskStageState {
            code: stage.code().to_string(),
            label: stage.label().to_string(),
            order: stage.order(),
            detail: detail.into(),
            current,
            total,
        },
    }
}

pub(super) fn done_task_progress_state() -> WorkspaceTaskProgressState {
    WorkspaceTaskProgressState {
        stage: WorkspaceTaskStageState::default(),
    }
}

/// The percentage a user actually reads off the progress bar.
fn visible_percent(state: &WorkspaceTaskProgressState) -> u32 {
    if state.stage.total == 0 {
        return 0;
    }
    state.stage.current.saturating_mul(100) / state.stage.total
}

/// Whether `next` can change what the user sees, compared with what is already
/// persisted in `previous`.
///
/// Progress ticks arrive once per segment, but the visible value is a percentage,
/// so most ticks cannot change anything. Persisting and emitting every one of
/// them costs a full meta UPSERT plus an IPC event — hundreds per task — for a
/// counter that visibly moves far less often.
///
/// Deliberately *not* compared: `detail` (it is the `current/total` text, so it
/// changes on every tick and would defeat the whole check). Deliberately always
/// passed through: a stage transition, because the stage label is user-visible
/// and must never be missed, and the final tick, so a finished task cannot be
/// left displaying an intermediate count.
pub(super) fn is_visible_progress(
    previous: &WorkspaceTaskProgressState,
    next: &WorkspaceTaskProgressState,
) -> bool {
    if previous.stage.code != next.stage.code {
        return true;
    }
    if next.stage.total == 0 || next.stage.current >= next.stage.total {
        return true;
    }
    previous.stage.total != next.stage.total
        || visible_percent(previous) != visible_percent(next)
}

/// The progress already persisted for `task_id`, without cloning or writing.
fn persisted_progress(task_id: &str) -> WorkspaceResult<WorkspaceTaskProgressState> {
    let store = lock_workspace_store()?;
    let Some(task) = store.tasks().iter().find(|entry| entry.item.id == task_id) else {
        return Err(WorkspaceError::TaskNotFound(task_id.to_string()));
    };
    Ok(task.item.task_progress.clone())
}

pub(super) async fn report_task_stage(
    app: &AppHandle,
    task_id: &str,
    stage: TaskStage,
    detail: impl Into<String>,
    current: u32,
    total: u32,
) -> WorkspaceResult<()> {
    // Phase trail for the app log's abnormal-exit report: if this process dies
    // without ever running again, the next start can still say which stage it
    // died in. A no-op when the stage has not changed, so the per-tick callers
    // pay nothing for it.
    crate::services::app_log::set_phase(&format!("{task_id}:{}", stage.code()));

    let progress = task_progress_state(stage, detail, current, total);

    // Check before doing any of the expensive work (clone, UPSERT, event): a tick
    // that cannot change the displayed value should cost a lookup, nothing more.
    if !is_visible_progress(&persisted_progress(task_id)?, &progress) {
        return Ok(());
    }

    patch_task_item(app, task_id, |task| {
        task.item.transcribe_status = "processing".to_string();
        task.item.task_progress = progress;
        task.item.transcribe_error = String::new();
    })
    .await
}

pub(super) async fn mark_task_failed(
    app: &AppHandle,
    task_id: &str,
    error: &str,
) -> WorkspaceResult<()> {
    patch_task_item(app, task_id, |task| {
        task.item.transcribe_status = "error".to_string();
        task.item.task_progress = WorkspaceTaskProgressState::default();
        task.item.transcribe_error = error.to_string();
    })
    .await
}

#[cfg(test)]
mod tests {
    //! Regression cover for the progress gate: the coalescing rule is the whole
    //! point of `is_visible_progress`, so its two invariants — that ticks which
    //! cannot move the displayed percentage are dropped, and that the signals a
    //! user depends on always get through — are pinned here.

    use super::*;

    /// Roughly a two-hour video at ~9s per VAD segment.
    const SEGMENTS: u32 = 800;

    fn progress(current: u32) -> WorkspaceTaskProgressState {
        task_progress_state(
            TaskStage::Recognizing,
            format!("{current}/{SEGMENTS}"),
            current,
            SEGMENTS,
        )
    }

    /// Ticks that survive the gate, threading the last *reported* state the way
    /// the real caller does.
    fn reported_ticks() -> u32 {
        let mut reported = 0;
        let mut last: Option<WorkspaceTaskProgressState> = None;
        for current in 1..=SEGMENTS {
            let next = progress(current);
            let visible = last
                .as_ref()
                .is_none_or(|previous| is_visible_progress(previous, &next));
            if visible {
                reported += 1;
                last = Some(next);
            }
        }
        reported
    }

    #[test]
    fn ticks_that_cannot_move_the_percentage_are_dropped() {
        let reported = reported_ticks();

        assert!(
            reported * 6 <= SEGMENTS,
            "expected at least a 6x reduction; got {reported} reports for {SEGMENTS} ticks"
        );
        assert!(
            reported >= 100,
            "the displayed percentage still has to move; got only {reported} reports"
        );
    }

    #[test]
    fn stage_changes_and_the_final_tick_always_get_through() {
        let previous = progress(10);

        // A stage transition is user-visible even at identical counts.
        let switched = task_progress_state(TaskStage::Translating, "10/800", 10, SEGMENTS);
        assert!(
            is_visible_progress(&previous, &switched),
            "a stage change must never be swallowed"
        );

        // The last tick has to land, or a finished task could show an early count.
        assert!(
            is_visible_progress(&previous, &progress(SEGMENTS)),
            "the final tick must never be swallowed"
        );

        // A re-chunked total has to land as well.
        let retotaled = task_progress_state(TaskStage::Recognizing, "10/900", 10, 900);
        assert!(
            is_visible_progress(&previous, &retotaled),
            "a changed total must never be swallowed"
        );

        // …while a tick inside the same whole percentage does not.
        assert!(
            !is_visible_progress(&progress(8), &progress(9)),
            "ticks inside one percentage point are exactly what should be skipped"
        );
    }
}
