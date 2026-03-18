use super::pipeline;
use super::types::{TranslatePipelineRequest, TranslatePipelineResponse};

pub fn run_translate_pipeline(
    request: TranslatePipelineRequest,
) -> Result<TranslatePipelineResponse, String> {
    pipeline::run_translate_pipeline(request)
}
