use serde_json::Value;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::db::store::TaskStore;
use super::error::LlmError;
use super::json_guard::JsonResponseValidator;

static LLM_REQUEST_SEQ: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone)]
pub struct LlmConfig {
    pub base_url: String,
    api_keys: Arc<[String]>,
    key_cursor: Arc<AtomicUsize>,
    pub model: String,
    pub max_retries: u32,
}

impl LlmConfig {
    pub fn new(base_url: String, api_key: String, model: String) -> Self {
        Self {
            base_url,
            api_keys: parse_api_keys(&api_key),
            key_cursor: Arc::new(AtomicUsize::new(0)),
            model,
            max_retries: 3,
        }
    }

    /// Next key in round-robin order. Concurrent calls share the cursor so
    /// `aaa|bbb|ccc` is spread across in-flight requests, not pinned to the
    /// first key.
    pub fn next_api_key(&self) -> &str {
        match self.api_keys.as_ref() {
            [] => "",
            [only] => only.as_str(),
            keys => {
                let i = self.key_cursor.fetch_add(1, Ordering::Relaxed);
                keys[i % keys.len()].as_str()
            }
        }
    }
}

/// Split `aaa|bbb|ccc` into trimmed non-empty keys. A single key (no `|`) is
/// unchanged. Empty slots (`a||b`) are dropped.
pub fn parse_api_keys(raw: &str) -> Arc<[String]> {
    raw.split('|')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>()
        .into()
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
    pub user_prompt: Arc<str>,
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
    #[allow(async_fn_in_trait)]
    async fn call_json(
        &self,
        context: &LlmCallContext,
        request_id: &str,
        user_prompt: &str,
        response_validator: Option<&JsonResponseValidator>,
    ) -> Result<LlmJsonResult, LlmError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;
    use std::thread;

    #[test]
    fn parse_api_keys_splits_on_pipe_and_drops_empty() {
        assert_eq!(parse_api_keys("aaa").as_ref(), &["aaa".to_string()]);
        assert_eq!(
            parse_api_keys("aaa|bbb|ccc").as_ref(),
            &["aaa".to_string(), "bbb".to_string(), "ccc".to_string()]
        );
        assert_eq!(
            parse_api_keys(" aaa | bbb | ").as_ref(),
            &["aaa".to_string(), "bbb".to_string()]
        );
        assert!(parse_api_keys("").is_empty());
        assert!(parse_api_keys("|||").is_empty());
    }

    #[test]
    fn next_api_key_rotates_round_robin() {
        let cfg = LlmConfig::new("u".into(), "aaa|bbb|ccc".into(), "m".into());
        assert_eq!(cfg.next_api_key(), "aaa");
        assert_eq!(cfg.next_api_key(), "bbb");
        assert_eq!(cfg.next_api_key(), "ccc");
        assert_eq!(cfg.next_api_key(), "aaa");
    }

    #[test]
    fn next_api_key_single_key_never_moves() {
        let cfg = LlmConfig::new("u".into(), "only".into(), "m".into());
        assert_eq!(cfg.next_api_key(), "only");
        assert_eq!(cfg.next_api_key(), "only");
    }

    #[test]
    fn cloned_config_shares_cursor() {
        let cfg = LlmConfig::new("u".into(), "a|b".into(), "m".into());
        let clone = cfg.clone();
        assert_eq!(cfg.next_api_key(), "a");
        assert_eq!(clone.next_api_key(), "b");
        assert_eq!(cfg.next_api_key(), "a");
    }

    #[test]
    fn round_robin_is_balanced_under_concurrency() {
        let cfg = Arc::new(LlmConfig::new("u".into(), "a|b|c".into(), "m".into()));
        let counts = Arc::new(Mutex::new(HashMap::<String, usize>::new()));
        let mut handles = Vec::new();
        for _ in 0..30 {
            let cfg = cfg.clone();
            let counts = counts.clone();
            handles.push(thread::spawn(move || {
                let key = cfg.next_api_key().to_string();
                *counts.lock().unwrap().entry(key).or_insert(0) += 1;
            }));
        }
        for handle in handles {
            handle.join().expect("thread");
        }
        let counts = counts.lock().unwrap();
        assert_eq!(counts.get("a"), Some(&10));
        assert_eq!(counts.get("b"), Some(&10));
        assert_eq!(counts.get("c"), Some(&10));
    }
}
