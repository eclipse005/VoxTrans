use serde_json::Value;

use super::error::LlmError;
use super::json_guard::JsonResponseValidator;

#[derive(Debug, Clone)]
pub struct LlmConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub max_retries: u32,
}

impl LlmConfig {
    pub fn new(base_url: String, api_key: String, model: String) -> Self {
        Self {
            base_url,
            api_key,
            model,
            max_retries: 3,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct LlmTokenUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

#[derive(Debug, Clone)]
pub struct LlmCallContext {
    pub task_id: String,
    pub media_path: Option<String>,
    pub phase: String,
}

#[derive(Debug, Clone)]
pub struct LlmJsonTask {
    pub id: usize,
    pub system_prompt: String,
    pub user_prompt: String,
    pub response_validator: Option<JsonResponseValidator>,
}

#[derive(Debug, Clone)]
pub struct LlmJsonResult {
    pub json: Value,
}

pub trait LlmPort {
    async fn call_json(
        &self,
        context: &LlmCallContext,
        system_prompt: &str,
        user_prompt: &str,
        response_validator: Option<&JsonResponseValidator>,
    ) -> Result<LlmJsonResult, LlmError>;

    async fn call_batch_json(
        &self,
        context: &LlmCallContext,
        tasks: Vec<LlmJsonTask>,
        concurrency: usize,
    ) -> Vec<(usize, Result<LlmJsonResult, LlmError>)>;
}
