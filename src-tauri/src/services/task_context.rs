use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const STAGE_INIT: &str = "init";
pub const STAGE_SEPARATE: &str = "separate";
pub const STAGE_ASR: &str = "asr";
pub const STAGE_PUNCTUATE: &str = "punctuate";
pub const STAGE_CORRECT: &str = "correct";
pub const STAGE_SEGMENT: &str = "segment";
pub const STAGE_SUMMARIZE: &str = "summarize";
pub const STAGE_TRANSLATE: &str = "translate";
pub const STAGE_QA: &str = "qa";
pub const STAGE_QA_LAYOUT: &str = "segment_optimize";
pub const STAGE_QA_QUALITY: &str = "qa_quality";
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
    pub projections: ProjectionState,
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
    pub status: String,
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
    pub correct: StageEnvelope,
    pub segment: StageEnvelope,
    pub summarize: StageEnvelope,
    pub translate: StageEnvelope,
    pub qa: StageEnvelope,
    #[serde(default)]
    pub qa_layout: StageEnvelope,
    #[serde(default)]
    pub qa_quality: StageEnvelope,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectionState {
    pub queue: ProjectionQueueState,
    pub editor: ProjectionEditorState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectionQueueState {
    pub transcribe_status: String,
    pub phase: String,
    #[serde(default)]
    pub phase_detail: String,
    pub progress_percent: u32,
    pub transcribe_segment_current: u32,
    pub transcribe_segment_total: u32,
    pub transcribe_error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectionEditorState {
    pub subtitle_segments_json: String,
    pub result_text: String,
    pub result_srt: String,
    pub translated_srt: String,
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
                status: "queued".to_string(),
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
            projections: ProjectionState {
                queue: ProjectionQueueState {
                    transcribe_status: "queued".to_string(),
                    phase: String::new(),
                    phase_detail: String::new(),
                    progress_percent: 0,
                    transcribe_segment_current: 0,
                    transcribe_segment_total: 0,
                    transcribe_error: String::new(),
                },
                editor: ProjectionEditorState {
                    subtitle_segments_json: "[]".to_string(),
                    result_text: String::new(),
                    result_srt: String::new(),
                    translated_srt: String::new(),
                },
            },
        }
    }

    pub fn mark_stage_running(&mut self, stage: &str) {
        let now = unix_now();
        self.runtime.current_stage = stage.to_string();
        self.runtime.status = "running".to_string();
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
        self.runtime.status = "failed".to_string();
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
        self.runtime.status = "completed".to_string();
        self.runtime.progress_percent = 100;
        self.runtime.can_resume_from = String::new();
        self.task.updated_at = unix_now();
    }

    pub fn set_queue_projection(
        &mut self,
        status: &str,
        phase: &str,
        phase_detail: &str,
        progress_percent: u32,
        current: u32,
        total: u32,
        error: &str,
    ) {
        self.projections.queue = ProjectionQueueState {
            transcribe_status: status.to_string(),
            phase: phase.to_string(),
            phase_detail: phase_detail.to_string(),
            progress_percent,
            transcribe_segment_current: current,
            transcribe_segment_total: total,
            transcribe_error: error.to_string(),
        };
        self.runtime.progress_percent = progress_percent.clamp(0, 100);
    }

    pub fn set_editor_projection(
        &mut self,
        subtitle_segments_json: String,
        result_text: String,
        result_srt: String,
        translated_srt: String,
    ) {
        self.projections.editor = ProjectionEditorState {
            subtitle_segments_json,
            result_text,
            result_srt,
            translated_srt,
        };
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
            STAGE_CORRECT => Some(&mut self.stages.correct),
            STAGE_SEGMENT => Some(&mut self.stages.segment),
            STAGE_SUMMARIZE => Some(&mut self.stages.summarize),
            STAGE_TRANSLATE => Some(&mut self.stages.translate),
            STAGE_QA => Some(&mut self.stages.qa),
            STAGE_QA_LAYOUT => Some(&mut self.stages.qa_layout),
            STAGE_QA_QUALITY => Some(&mut self.stages.qa_quality),
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
            STAGE_CORRECT => Some(&self.stages.correct),
            STAGE_SEGMENT => Some(&self.stages.segment),
            STAGE_SUMMARIZE => Some(&self.stages.summarize),
            STAGE_TRANSLATE => Some(&self.stages.translate),
            STAGE_QA => Some(&self.stages.qa),
            STAGE_QA_LAYOUT => Some(&self.stages.qa_layout),
            STAGE_QA_QUALITY => Some(&self.stages.qa_quality),
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
            correct: StageEnvelope::new(),
            segment: StageEnvelope::new(),
            summarize: StageEnvelope::new(),
            translate: StageEnvelope::new(),
            qa: StageEnvelope::new(),
            qa_layout: StageEnvelope::new(),
            qa_quality: StageEnvelope::new(),
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
