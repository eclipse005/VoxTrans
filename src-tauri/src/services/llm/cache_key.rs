use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use serde_json::Value;

use super::base_url::normalize_base_url;
use super::json_guard::JsonResponseValidator;
use super::port::{LlmCallContext, LlmConfig};

pub(super) fn compute_cache_key(
    context: &LlmCallContext,
    config: &LlmConfig,
    system_prompt: &str,
    user_prompt: &str,
) -> String {
    let mut hasher = DefaultHasher::new();
    context.phase.hash(&mut hasher);
    config.model.hash(&mut hasher);
    normalize_base_url(&config.base_url).hash(&mut hasher);
    normalize_prompt(system_prompt).hash(&mut hasher);
    normalize_prompt(user_prompt).hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

pub(super) fn normalized_required_keys(
    response_validator: Option<&JsonResponseValidator>,
) -> Vec<String> {
    let mut keys = response_validator
        .map(|validator| validator.required_top_level_keys.clone())
        .unwrap_or_default();
    keys.sort();
    keys
}

pub(super) fn validator_key(keys: &[String]) -> String {
    if keys.is_empty() {
        String::new()
    } else {
        keys.join("\u{1f}")
    }
}

fn normalize_prompt(prompt: &str) -> String {
    let trimmed = prompt.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        serde_json::to_string(&value).unwrap_or_else(|_| trimmed.to_string())
    } else {
        trimmed.to_string()
    }
}
