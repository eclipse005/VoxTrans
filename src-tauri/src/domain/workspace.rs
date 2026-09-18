//! Workspace task/queue records.
//!
//! These describe a task's persisted state and are hydrated straight from the
//! `tasks` table by `db`, then surfaced over IPC — so the record itself is
//! shared vocabulary owned by the domain, not the transport layer. The
//! command-layer request/response envelopes around them live in
//! `commands::workspace::types`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceQueueItem {
    pub id: String,
    pub path: String,
    pub name: String,
    pub media_kind: String,
    pub size_bytes: u64,
    #[serde(default = "default_source_lang")]
    pub source_lang: String,
    #[serde(default = "default_target_lang")]
    pub target_lang: String,
    pub transcribe_status: String,
    pub task_progress: WorkspaceTaskProgressState,
    pub transcribe_error: String,
    pub result_text: String,
    pub result_srt: String,
    pub subtitle_segments_json: String,
    #[serde(default)]
    pub llm_total_tokens: u64,
    #[serde(default)]
    pub terminology_group_id: String,
    /// Pause after source cues are ready (before translation). Task-effective.
    #[serde(default)]
    pub review_source: bool,
    /// Pause after translation (before SRT/burn deliver). Task-effective.
    #[serde(default)]
    pub review_target: bool,
    /// Mid-pipeline resume marker for the worker: "" | "translate".
    #[serde(default)]
    pub resume_from: String,
}

fn default_source_lang() -> String {
    "en".to_string()
}

fn default_target_lang() -> String {
    "zh-CN".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceTaskStageState {
    #[serde(default)]
    pub code: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub order: u32,
    #[serde(default)]
    pub detail: String,
    #[serde(default)]
    pub current: u32,
    #[serde(default)]
    pub total: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceTaskProgressState {
    #[serde(default)]
    pub stage: WorkspaceTaskStageState,
}
