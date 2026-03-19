use super::pipeline;
use super::types::{TranslatePipelineRequest, TranslatePipelineResponse};

pub async fn run_translate_pipeline(
    request: TranslatePipelineRequest,
) -> Result<TranslatePipelineResponse, String> {
    pipeline::run_translate_pipeline(request, |_| {}, |_, _| {}).await
}

pub async fn run_translate_pipeline_with_phase<F, G>(
    request: TranslatePipelineRequest,
    on_phase: F,
    on_batch_progress: G,
) -> Result<TranslatePipelineResponse, String>
where
    F: FnMut(&str),
    G: FnMut(usize, usize),
{
    pipeline::run_translate_pipeline(request, on_phase, on_batch_progress).await
}
