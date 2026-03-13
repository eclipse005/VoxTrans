use serde_json::Value;

pub fn parse_llm_json_response(raw: &str) -> Result<Value, String> {
    crate::services::llm::json::parse_llm_json_response(raw)
}
