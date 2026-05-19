use std::path::{Path, PathBuf};

use async_trait::async_trait;
use serde::Serialize;
use serde::de::DeserializeOwned;

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

#[derive(Debug, Clone)]
pub struct StepContext<'a> {
    pub output_dir: &'a Path,
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
    fn artifact_file(&self) -> &'static str;
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
) -> Result<StepExecution<S::Output>, String>
where
    S: PipelineStep + Send + Sync,
{
    let artifact_path = ctx.output_dir.join(step.artifact_file());
    match step.policy() {
        CheckpointPolicy::SkipIfExists => {
            if let Some(cached) = read_json_if_exists::<S::Output>(&artifact_path)? {
                return Ok(StepExecution {
                    output: cached,
                    source: StepSource::Cache,
                });
            }
            run_and_persist(step, ctx, artifact_path).await
        }
    }
}

async fn run_and_persist<S>(
    step: &S,
    ctx: &StepContext<'_>,
    artifact_path: PathBuf,
) -> Result<StepExecution<S::Output>, String>
where
    S: PipelineStep + Send + Sync,
{
    let output = step
        .run(ctx)
        .await
        .map_err(|err| format!("{} failed: {err}", step.name()))?;
    step.validate(&output)?;
    write_json(&artifact_path, &output)?;
    Ok(StepExecution {
        output,
        source: StepSource::Computed,
    })
}

pub fn read_json_if_exists<T: DeserializeOwned>(path: &Path) -> Result<Option<T>, String> {
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(path).map_err(|err| err.to_string())?;
    let parsed = serde_json::from_str::<T>(&raw)
        .map_err(|err| format!("failed to parse {}: {}", path.display(), err))?;
    Ok(Some(parsed))
}

pub fn write_json<T: Serialize>(path: &Path, payload: &T) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let content = serde_json::to_string_pretty(payload).map_err(|err| err.to_string())?;
    std::fs::write(path, content.as_bytes()).map_err(|err| err.to_string())
}
