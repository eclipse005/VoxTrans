use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::services::task_status::TaskRuntimeStatus;

pub const STAGE_INIT: &str = "init";
pub const STAGE_SEPARATE: &str = "separate";
pub const STAGE_ASR: &str = "asr";
pub const STAGE_PUNCTUATE: &str = "punctuate";
pub const STAGE_SEGMENT: &str = "segment";
pub const STAGE_SUMMARIZE: &str = "summarize";
pub const STAGE_TRANSLATE: &str = "translate";
pub const STAGE_SEGMENT_OPTIMIZE: &str = "segment_optimize";
pub const STAGE_BURNING: &str = "burning";
pub const STAGE_COMPOSE: &str = "compose";
pub const STAGE_PERSIST: &str = "persist";
pub const STAGE_DONE: &str = "done";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskContext {
    pub schema_version: String,
    pub task: TaskMeta,
    pub input: InputSnapshot,
    pub runtime: RuntimeState,
    pub stages: StageMap,
    pub artifacts: ArtifactMap,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskMeta {
    pub task_id: String,
    pub intent: String,
    pub source_lang: String,
    pub target_lang: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InputSnapshot {
    pub media_path: String,
    pub media_kind: String,
    pub media_size_bytes: u64,
    pub settings_snapshot: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeState {
    pub current_stage: String,
    pub status: TaskRuntimeStatus,
    pub progress_percent: u32,
    pub retry_count: u32,
    #[serde(default)]
    pub can_resume_from: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StageMap {
    pub init: StageEnvelope,
    pub separate: StageEnvelope,
    pub asr: StageEnvelope,
    pub punctuate: StageEnvelope,
    pub segment: StageEnvelope,
    pub summarize: StageEnvelope,
    pub translate: StageEnvelope,
    pub segment_optimize: StageEnvelope,
    pub compose: StageEnvelope,
    pub persist: StageEnvelope,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StageEnvelope {
    pub version: String,
    pub status: String,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
    pub output: Value,
    pub metrics: Value,
    pub error: Option<StageError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StageError {
    pub code: String,
    pub message: String,
    pub retriable: bool,
    pub details: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactMap {
    pub words_json: Value,
    pub source_srt: Value,
    pub target_srt: Value,
    pub bilingual_source_first: Value,
    pub bilingual_target_first: Value,
}

#[derive(Debug, Clone)]
pub struct TaskContextSeed {
    pub task_id: String,
    pub intent: String,
    pub source_lang: String,
    pub target_lang: String,
    pub media_path: String,
    pub media_kind: String,
    pub media_size_bytes: u64,
    pub settings_snapshot: Value,
    pub created_at: i64,
}

impl TaskContext {
    pub fn new(seed: TaskContextSeed) -> Self {
        let now = unix_now();
        Self {
            schema_version: "v1".to_string(),
            task: TaskMeta {
                task_id: seed.task_id,
                intent: seed.intent,
                source_lang: seed.source_lang,
                target_lang: seed.target_lang,
                created_at: seed.created_at,
                updated_at: now,
            },
            input: InputSnapshot {
                media_path: seed.media_path,
                media_kind: seed.media_kind,
                media_size_bytes: seed.media_size_bytes,
                settings_snapshot: seed.settings_snapshot,
            },
            runtime: RuntimeState {
                current_stage: STAGE_INIT.to_string(),
                status: TaskRuntimeStatus::Queued,
                progress_percent: 0,
                retry_count: 0,
                can_resume_from: STAGE_INIT.to_string(),
            },
            stages: StageMap::new(),
            artifacts: ArtifactMap {
                words_json: Value::Null,
                source_srt: Value::Null,
                target_srt: Value::Null,
                bilingual_source_first: Value::Null,
                bilingual_target_first: Value::Null,
            },
        }
    }

    pub fn mark_stage_running(&mut self, stage: &str) {
        let now = unix_now();
        self.runtime.current_stage = stage.to_string();
        self.runtime.status = TaskRuntimeStatus::Running;
        self.runtime.can_resume_from = stage.to_string();
        self.task.updated_at = now;
        if let Some(s) = self.stage_mut(stage) {
            s.status = "running".to_string();
            s.started_at = Some(now);
            s.error = None;
        }
    }

    pub fn mark_stage_done(&mut self, stage: &str, output: Value, metrics: Value) {
        let now = unix_now();
        self.task.updated_at = now;
        if let Some(s) = self.stage_mut(stage) {
            s.status = "done".to_string();
            s.finished_at = Some(now);
            s.output = output;
            s.metrics = metrics;
            s.error = None;
        }
    }

    pub fn mark_failed(&mut self, stage: &str, code: &str, message: &str, retriable: bool) {
        let now = unix_now();
        self.runtime.current_stage = stage.to_string();
        self.runtime.status = TaskRuntimeStatus::Failed;
        self.runtime.can_resume_from = stage.to_string();
        self.task.updated_at = now;
        if let Some(s) = self.stage_mut(stage) {
            s.status = "failed".to_string();
            s.finished_at = Some(now);
            s.error = Some(StageError {
                code: code.to_string(),
                message: message.to_string(),
                retriable,
                details: Value::Null,
            });
        }
    }

    pub fn mark_completed(&mut self) {
        self.runtime.current_stage = STAGE_DONE.to_string();
        self.runtime.status = TaskRuntimeStatus::Completed;
        self.runtime.progress_percent = 100;
        self.runtime.can_resume_from = String::new();
        self.task.updated_at = unix_now();
    }

    pub fn stage_status(&self, stage: &str) -> &str {
        self.stage_ref(stage)
            .map(|s| s.status.as_str())
            .unwrap_or("pending")
    }

    pub fn set_stage_snapshot(
        &mut self,
        stage: &str,
        status: String,
        started_at: Option<i64>,
        finished_at: Option<i64>,
        output: Value,
        metrics: Value,
        error_code: String,
        error_message: String,
    ) {
        if let Some(s) = self.stage_mut(stage) {
            s.status = status;
            s.started_at = started_at;
            s.finished_at = finished_at;
            s.output = output;
            s.metrics = metrics;
            if error_code.trim().is_empty() && error_message.trim().is_empty() {
                s.error = None;
            } else {
                s.error = Some(StageError {
                    code: error_code,
                    message: error_message,
                    retriable: true,
                    details: Value::Null,
                });
            }
        }
    }

    fn stage_mut(&mut self, stage: &str) -> Option<&mut StageEnvelope> {
        match stage {
            STAGE_INIT => Some(&mut self.stages.init),
            STAGE_SEPARATE => Some(&mut self.stages.separate),
            STAGE_ASR => Some(&mut self.stages.asr),
            STAGE_PUNCTUATE => Some(&mut self.stages.punctuate),
            STAGE_SEGMENT => Some(&mut self.stages.segment),
            STAGE_SUMMARIZE => Some(&mut self.stages.summarize),
            STAGE_TRANSLATE => Some(&mut self.stages.translate),
            STAGE_SEGMENT_OPTIMIZE => Some(&mut self.stages.segment_optimize),
            STAGE_COMPOSE => Some(&mut self.stages.compose),
            STAGE_PERSIST => Some(&mut self.stages.persist),
            _ => None,
        }
    }

    fn stage_ref(&self, stage: &str) -> Option<&StageEnvelope> {
        match stage {
            STAGE_INIT => Some(&self.stages.init),
            STAGE_SEPARATE => Some(&self.stages.separate),
            STAGE_ASR => Some(&self.stages.asr),
            STAGE_PUNCTUATE => Some(&self.stages.punctuate),
            STAGE_SEGMENT => Some(&self.stages.segment),
            STAGE_SUMMARIZE => Some(&self.stages.summarize),
            STAGE_TRANSLATE => Some(&self.stages.translate),
            STAGE_SEGMENT_OPTIMIZE => Some(&self.stages.segment_optimize),
            STAGE_COMPOSE => Some(&self.stages.compose),
            STAGE_PERSIST => Some(&self.stages.persist),
            _ => None,
        }
    }
}

impl StageMap {
    fn new() -> Self {
        Self {
            init: StageEnvelope::new(),
            separate: StageEnvelope::new(),
            asr: StageEnvelope::new(),
            punctuate: StageEnvelope::new(),
            segment: StageEnvelope::new(),
            summarize: StageEnvelope::new(),
            translate: StageEnvelope::new(),
            segment_optimize: StageEnvelope::new(),
            compose: StageEnvelope::new(),
            persist: StageEnvelope::new(),
        }
    }
}

impl StageEnvelope {
    fn new() -> Self {
        Self {
            version: "v1".to_string(),
            status: "pending".to_string(),
            started_at: None,
            finished_at: None,
            output: Value::Null,
            metrics: Value::Null,
            error: None,
        }
    }
}

impl Default for StageEnvelope {
    fn default() -> Self {
        Self::new()
    }
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
