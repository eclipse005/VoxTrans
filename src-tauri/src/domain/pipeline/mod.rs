//! Pipeline step + checkpoint framework.
//!
//! # Two-tier checkpoint model
//!
//! voxtrans steps are checkpointed at two granularities; **both are
//! required** when a step does sub-step parallelism, and they cover
//! different failure modes:
//!
//! 1. **Coarse-grained: `task_artifacts`** — `execute_step` calls
//!    `store.save_artifact(task_id, step.name(), <step output JSON>)`
//!    once, AFTER `PipelineStep::run` returns. Saves a successful, fully
//!    composed step output so the *next* run short-circuits the entire
//!    step. Only fires on success — does nothing for in-flight work.
//!
//! 2. **Fine-grained: `UnitStore::save_*`** — sub-steps (one ASR
//!    segment, one translation batch, one step5 overlong work) MUST
//!    call the matching `save_*` method on `UnitStore` **immediately
//!    when that unit completes** — not after a batch or round
//!    finishes, not in a "persist all at the end" loop. If the process
//!    is killed mid-step, the next run will see only the units that
//!    were persisted; `load_*` is the resume source of truth.
//!
//! ## The invariant (what the framework guarantees)
//!
//! For any sub-step `unit` of a step `S`:
//!
//! - If `unit` was persisted via `UnitStore::save_*` before the process
//!   exited, the next run will load it back through `UnitStore::load_*`
//!   and **must skip recomputation** for it.
//! - If `unit` was NOT persisted, `load_*` reports it as missing — the
//!   step **must** recompute it.
//!
//! Violating this invariant produces user-visible regressions like
//! "progress jumped backward from 14/21 to 3/21 after restart" — the
//! UI showed real LLM work that was never written to SQLite and is
//! therefore not recoverable.
//!
//! ## When in doubt
//!
//! Wire the `save_*` call inside the same `await` block / callback
//! that produces the unit result, BEFORE the result is propagated to
//! anything else. Look at `services/translation/mod.rs::on_item_done`
//! (step4) or `commands/workspace/pipeline_steps/recognition.rs`
//! (step1's `fresh_result` handler) for the canonical shape.
//!
//! The contract is exercised by `terminology_frozen_contract_no_cross_writes`
//! and `step5_resume_skips_cached_works`; if you add a new sub-step
//! resume table, add a matching test or this invariant will silently
//! rot.

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

/// Step 1 alignment: Cached ForcedAlignResult for a single segment.
#[derive(Debug, Clone)]
pub struct AlignmentResultRow {
    pub segment_index: usize,
    pub items: Vec<qwen_forced_aligner_rs::ForcedAlignItem>,
}

/// Step 5 (combined): Split + align result for a single segment.
#[derive(Debug, Clone)]
pub struct Step5SplitAlignRow {
    pub segment_index: usize,
    pub parent_json: String,
}

// ── UnitStore: task-scoped accessor for domain tables ───────────

/// Thin handle that steps use to persist / load idempotent computation
/// results from domain-specific tables, keyed by task_id only.
///
/// **Resume contract**: every `save_*` here MUST be called from inside
/// the per-unit completion handler — NOT batched after the round /
/// group finishes. The matching `load_*` is the resume source of
/// truth; anything not yet persisted will be recomputed on the next
/// run. See the module-level doc for the full rationale.
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

    // ── Step 5 combined: Split + align per segment ──

    pub async fn load_step5_split_aligns(&self) -> Result<Vec<Step5SplitAlignRow>, String> {
        self.store.load_step5_split_aligns(&self.task_id).await
    }

    pub async fn save_step5_split_align(&self, row: &Step5SplitAlignRow) -> Result<(), String> {
        self.store.save_step5_split_align(&self.task_id, row).await
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
    /// Run the step.
    ///
    /// **If this step does sub-step parallelism** (one ASR segment, one
    /// translation batch, one step5 overlong work), each sub-step MUST
    /// persist its own result through `UnitStore::save_*` the instant
    /// it completes. Saving only after the whole step returns will lose
    /// every in-flight unit when the process exits mid-step — even
    /// though the coarse `task_artifacts` record is written by the
    /// framework once `run` returns, that record never exists for
    /// killed runs, so resume falls back to the per-unit tables.
    /// See the module-level doc for the contract.
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
