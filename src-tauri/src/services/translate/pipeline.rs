use super::types::{TranslatePipelineRequest, TranslatePipelineResponse};

pub fn run_translate_pipeline(
    _request: TranslatePipelineRequest,
) -> Result<TranslatePipelineResponse, String> {
    Err("translate pipeline not implemented".to_string())
}
