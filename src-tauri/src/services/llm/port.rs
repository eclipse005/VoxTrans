use serde_json::Value;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::db::store::TaskStore;
use super::error::LlmError;
use super::json_guard::JsonResponseValidator;

static LLM_REQUEST_SEQ: AtomicU64 = AtomicU64::new(1);

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
    pub store: Option<TaskStore>,
}

#[derive(Debug, Clone)]
pub struct LlmJsonTask {
    pub id: usize,
    pub request_id: String,
    pub user_prompt: String,
    pub response_validator: Option<JsonResponseValidator>,
}

#[derive(Debug, Clone)]
pub struct LlmJsonResult {
    pub json: Value,
}

pub fn next_llm_request_id() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    let seq = LLM_REQUEST_SEQ.fetch_add(1, Ordering::Relaxed);
    format!("{millis}-{:04x}", seq & 0xffff)
}

pub trait LlmPort {
    async fn call_json(
        &self,
        context: &LlmCallContext,
        request_id: &str,
        user_prompt: &str,
        response_validator: Option<&JsonResponseValidator>,
    ) -> Result<LlmJsonResult, LlmError>;
}
