//! What we have learned about an endpoint: which dialect it actually answers
//! on, and the `max_tokens` it will tolerate after a rejection. Both were
//! previously scattered process-wide `OnceLock<Mutex<..>>` statics; they are
//! one concern — a memo of endpoint capabilities keyed by (normalized base_url,
//! model) — so they live together and, crucially, become an *injected
//! dependency* (`LlmConfig` holds one). Production shares a single
//! process-wide instance (memoization spans every request, exactly as before);
//! tests construct a fresh one and get an isolated slate, retiring the old
//! unique-URL `line!()` hacks.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use super::base_url::normalize_base_url;
use super::max_tokens::resolve_max_tokens;
use super::protocol::{LlmProtocol, heuristic};

/// Shared across every `LlmConfig` built through [`LlmConfig::new`], so the
/// app keeps one capability cache for the process lifetime.
static GLOBAL: OnceLock<std::sync::Arc<CapabilityMemo>> = OnceLock::new();

pub(crate) fn global_capabilities() -> std::sync::Arc<CapabilityMemo> {
    GLOBAL.get_or_init(|| std::sync::Arc::new(CapabilityMemo::new())).clone()
}

#[derive(Debug)]
pub struct CapabilityMemo {
    /// `{base_url}\u{1}{model}` -> the dialect this endpoint+model answered on.
    protocol: Mutex<HashMap<String, LlmProtocol>>,
    /// `{base_url}\u{1}{model}` -> a `max_tokens` the endpoint accepted after
    /// rejecting a larger one.
    max_tokens_override: Mutex<HashMap<String, u32>>,
}

impl CapabilityMemo {
    pub(crate) fn new() -> Self {
        Self {
            protocol: Mutex::new(HashMap::new()),
            max_tokens_override: Mutex::new(HashMap::new()),
        }
    }

    fn endpoint_key(base_url: &str, model: &str) -> String {
        format!("{}\u{1}{}", normalize_base_url(base_url), model.trim())
    }

    /// The protocol to try first: what this endpoint already answered on, else
    /// the URL-only heuristic (see [`heuristic`]).
    pub(crate) fn resolve_protocol(&self, base_url: &str, model: &str) -> LlmProtocol {
        let key = Self::endpoint_key(base_url, model);
        self.protocol
            .lock()
            .ok()
            .and_then(|map| map.get(&key).copied())
            .unwrap_or_else(|| heuristic(base_url))
    }

    /// Remember which dialect this endpoint+model actually answered on, so
    /// later calls skip the probe entirely.
    pub(crate) fn remember_protocol(&self, base_url: &str, model: &str, protocol: LlmProtocol) {
        let key = Self::endpoint_key(base_url, model);
        if let Ok(mut map) = self.protocol.lock() {
            map.insert(key, protocol);
        }
    }

    /// The `max_tokens` a request should carry: the recorded degrade value when
    /// this endpoint+model rejected a larger one before, the family ceiling
    /// otherwise.
    pub(crate) fn effective_max_tokens(&self, base_url: &str, model: &str) -> u32 {
        let key = Self::endpoint_key(base_url, model);
        self.max_tokens_override
            .lock()
            .ok()
            .and_then(|map| map.get(&key).copied())
            .unwrap_or_else(|| resolve_max_tokens(model))
    }

    pub(crate) fn record_max_tokens_override(&self, base_url: &str, model: &str, value: u32) {
        let key = Self::endpoint_key(base_url, model);
        if let Ok(mut map) = self.max_tokens_override.lock() {
            map.insert(key, value);
        }
    }
}

impl Default for CapabilityMemo {
    fn default() -> Self {
        Self::new()
    }
}
