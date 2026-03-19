use super::pipeline;
use super::types::{TranslatePipelineRequest, TranslatePipelineResponse};

pub async fn run_translate_pipeline(
    request: TranslatePipelineRequest,
) -> Result<TranslatePipelineResponse, String> {
    pipeline::run_translate_pipeline(request, |_| {}).await
}

pub async fn run_translate_pipeline_with_phase<F>(
    request: TranslatePipelineRequest,
    on_phase: F,
) -> Result<TranslatePipelineResponse, String>
where
    F: FnMut(&str),
{
    pipeline::run_translate_pipeline(request, on_phase).await
}
