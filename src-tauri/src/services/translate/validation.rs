use super::types::TranslatePipelineRequest;
use crate::services::translate::adapters::llm_client::JsonResponseValidator;

pub fn validate_request(_request: &TranslatePipelineRequest) -> Result<(), String> {
    // TODO: Enforce schema/coverage/ordering invariants for translation pipeline inputs.
    Ok(())
}

pub fn validate_llm_segments() {
    // TODO: Validate LLM JSON output and enforce GAP_1500MS hard boundaries.
}

pub fn punctuation_response_validator() -> JsonResponseValidator {
    JsonResponseValidator::with_required_keys(&["punctuatedText"])
}
