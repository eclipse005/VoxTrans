use std::io::Write;
use tauri::Emitter;

const WORKER_EVENT_PREFIX: &str = "VOXTRANS_EVENT:";

#[derive(Debug, serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(super) struct WorkspaceSyncHintEvent {
    pub task_id: String,
}

/// Event sent when a task's state changes.
/// Payload contains the full QueueItem data for frontend to replace.
#[derive(Debug, serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TaskStateChangedEvent {
    pub id: String,
    pub path: String,
    pub name: String,
    pub media_kind: String,
    pub size_bytes: u64,
    pub transcribe_status: String,
    pub transcribe_progress: u32,
    pub transcribe_segment_current: u32,
    pub transcribe_segment_total: u32,
    pub transcribe_phase: String,
    pub transcribe_phase_detail: String,
    pub transcribe_error: String,
    pub result_text: String,
    pub result_srt: String,
    pub subtitle_segments_json: String,
}

#[derive(Debug, serde::Serialize)]
struct WorkerEventEnvelope<'a, T: serde::Serialize> {
    event: &'a str,
    payload: &'a T,
}

pub(super) fn emit_bridge_event<T: serde::Serialize>(
    app: Option<&tauri::AppHandle>,
    event: &str,
    payload: &T,
) {
    if let Some(app_handle) = app {
        let _ = app_handle.emit(event, payload);
        return;
    }
    if let Ok(envelope) = serde_json::to_string(&WorkerEventEnvelope { event, payload }) {
        println!("{WORKER_EVENT_PREFIX}{envelope}");
        let _ = std::io::stdout().flush();
    }
}

/// Helper to emit a task-state-changed event with full QueueItem data.
pub(super) fn emit_task_state_changed(
    app: Option<&tauri::AppHandle>,
    payload: &TaskStateChangedEvent,
) {
    emit_bridge_event(app, "task-state-changed", payload);
}
