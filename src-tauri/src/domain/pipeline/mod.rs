use std::collections::HashMap;

use async_trait::async_trait;
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::db::store::TaskStore;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckpointPolicy {
    // Artifact exists => skip directly, no validation.
    SkipIfExists,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepSource {
    Cache,
    Computed,
}

// ── Domain row structs for pipeline resume tables ──────────────────────

/// Step 1: ASR transcript for a single VAD segment.
#[derive(Debug, Clone)]
pub struct AsrTranscriptRow {
    pub segment_index: usize,
    pub text: String,
}

/// Step 4: Translation result for a single batch window.
#[derive(Debug, Clone)]
pub struct TranslationBatchRow {
    pub batch_index: usize,
    pub segment_translations: HashMap<usize, String>,
}

/// Step 5.1: Source split result for a single LLM work unit.
#[derive(Debug, Clone)]
pub struct SourceSplitRow {
    pub work_index: usize,
    pub segment_start: usize,
    pub segment_end: usize,
    pub boundary_positions: Vec<usize>,
}

/// Step 1 alignment: Cached ForcedAlignResult for a single segment.
#[derive(Debug, Clone)]
pub struct AlignmentResultRow {
    pub segment_index: usize,
    pub items: Vec<qwen_forced_aligner_rs::ForcedAlignItem>,
}

/// Step 5.2: Translation alignment result for a single parent segment.
#[derive(Debug, Clone)]
pub struct TranslationAlignRow {
    pub parent_index: usize,
    pub aligned_lines: Vec<String>,
}

// ── UnitStore: task-scoped accessor for domain tables ───────────

/// Thin handle that steps use to persist / load idempotent computation
/// results from domain-specific tables, keyed by task_id only.
#[derive(Clone)]
pub struct UnitStore {
    store: TaskStore,
    task_id: String,
}

impl std::fmt::Debug for UnitStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UnitStore")
            .field("task_id", &self.task_id)
            .finish()
    }
}

impl UnitStore {
    pub fn new(store: &TaskStore, task_id: &str) -> Self {
        Self {
            store: store.clone(),
            task_id: task_id.to_string(),
        }
    }

    #[allow(dead_code)]
    pub fn task_id(&self) -> &str {
        &self.task_id
    }

    pub fn store(&self) -> &TaskStore {
        &self.store
    }

    // ── Step 1: ASR transcripts ──

    pub async fn load_asr_transcripts(&self) -> Result<Vec<AsrTranscriptRow>, String> {
        self.store.load_asr_transcripts(&self.task_id).await
    }

    pub async fn save_asr_transcript(&self, row: &AsrTranscriptRow) -> Result<(), String> {
        self.store.save_asr_transcript(&self.task_id, row).await
    }

    // ── Step 1 alignment: Cached alignment results ──

    pub async fn load_alignment_results(&self) -> Result<Vec<AlignmentResultRow>, String> {
        self.store.load_alignment_results(&self.task_id).await
    }

    pub async fn save_alignment_result(&self, row: &AlignmentResultRow) -> Result<(), String> {
        self.store.save_alignment_result(&self.task_id, row).await
    }

    // ── Step 4: Translation batches ──

    pub async fn load_translation_batches(&self) -> Result<Vec<TranslationBatchRow>, String> {
        self.store.load_translation_batches(&self.task_id).await
    }

    pub async fn save_translation_batch(&self, row: &TranslationBatchRow) -> Result<(), String> {
        self.store.save_translation_batch(&self.task_id, row).await
    }

    // ── Step 5.1: Source splits ──

    pub async fn load_source_splits(&self) -> Result<Vec<SourceSplitRow>, String> {
        self.store.load_source_splits(&self.task_id).await
    }

    pub async fn save_source_split(&self, row: &SourceSplitRow) -> Result<(), String> {
        self.store.save_source_split(&self.task_id, row).await
    }

    // ── Step 5.2: Translation alignment ──

    pub async fn load_translation_aligns(&self) -> Result<Vec<TranslationAlignRow>, String> {
        self.store.load_translation_aligns(&self.task_id).await
    }

    pub async fn save_translation_align(&self, row: &TranslationAlignRow) -> Result<(), String> {
        self.store.save_translation_align(&self.task_id, row).await
    }
}

#[derive(Clone)]
pub struct StepContext<'a> {
    pub task_id: &'a str,
    pub store: &'a TaskStore,
}

impl<'a> StepContext<'a> {
    pub fn unit_store(&self) -> UnitStore {
        UnitStore::new(self.store, self.task_id)
    }
}

#[derive(Debug, Clone)]
pub struct StepExecution<T> {
    pub output: T,
    pub source: StepSource,
}

#[async_trait]
pub trait PipelineStep {
    type Output: Serialize + DeserializeOwned + Clone + Send + Sync + 'static;

    fn name(&self) -> &'static str;
    fn policy(&self) -> CheckpointPolicy {
        CheckpointPolicy::SkipIfExists
    }
    fn validate(&self, _output: &Self::Output) -> Result<(), String> {
        Ok(())
    }
    async fn run(&self, ctx: &StepContext<'_>) -> Result<Self::Output, String>;
}

pub async fn execute_step<S>(
    step: &S,
    ctx: &StepContext<'_>,
    store: &TaskStore,
) -> Result<StepExecution<S::Output>, String>
where
    S: PipelineStep + Send + Sync,
{
    match step.policy() {
        CheckpointPolicy::SkipIfExists => {
            if let Some(json) = store.load_artifact(ctx.task_id, step.name()).await? {
                if let Ok(cached) = serde_json::from_str::<S::Output>(&json) {
                    return Ok(StepExecution {
                        output: cached,
                        source: StepSource::Cache,
                    });
                }
            }
            run_and_persist(step, ctx, store).await
        }
    }
}

async fn run_and_persist<S>(
    step: &S,
    ctx: &StepContext<'_>,
    store: &TaskStore,
) -> Result<StepExecution<S::Output>, String>
where
    S: PipelineStep + Send + Sync,
{
    let output = step
        .run(ctx)
        .await
        .map_err(|err| format!("{} failed: {err}", step.name()))?;
    step.validate(&output)?;
    let json = serde_json::to_string(&output).map_err(|err| err.to_string())?;
    store.save_artifact(ctx.task_id, step.name(), &json).await?;
    Ok(StepExecution {
        output,
        source: StepSource::Computed,
    })
}
