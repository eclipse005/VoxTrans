use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::fs::OpenOptions;
use std::hash::{Hash, Hasher};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::json_guard::JsonResponseValidator;
use super::port::{LlmCallContext, LlmConfig, LlmTokenUsage};

const CACHE_FILE_NAME: &str = "gpt.log";
const CACHE_VERSION: u32 = 1;
const MAX_CACHE_ENTRIES: usize = 4000;

/// Use RwLock for better concurrent read access
static CACHE_FILE_STATE: OnceLock<RwLock<HashMap<PathBuf, Arc<CacheFileState>>>> = OnceLock::new();

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CacheEntry {
    version: u32,
    model: String,
    base_url: String,
    phase: String,
    cache_key: String,
    response_text: String,
    response_json: Value,
    validator_required_keys: Vec<String>,
    prompt_tokens: u64,
    completion_tokens: u64,
    total_tokens: u64,
    created_at_unix_sec: u64,
}

#[derive(Debug, Clone)]
pub struct CacheHit {
    pub json: Value,
}

#[derive(Debug, Clone, Default)]
struct CacheFileState {
    loaded: bool,
    by_validator_key: HashMap<(String, String), CacheEntry>,
}

/// 计算缓存键：hash(phase + model + base_url + normalized_prompts)
pub fn compute_cache_key(
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

/// 规范化 prompt 用于缓存匹配：
/// - 如果是 JSON，解析后重新序列化（排序键、移除空白）
/// - 否则去除首尾空白
fn normalize_prompt(prompt: &str) -> String {
    let trimmed = prompt.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    // 尝试解析为 JSON，如果是则规范化
    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        serde_json::to_string(&value).unwrap_or_else(|_| trimmed.to_string())
    } else {
        trimmed.to_string()
    }
}

pub fn read_cache_hit(
    context: &LlmCallContext,
    config: &LlmConfig,
    system_prompt: &str,
    user_prompt: &str,
    response_validator: Option<&JsonResponseValidator>,
) -> Option<CacheHit> {
    let path = cache_file_path(context)?;
    // load_or_get_cache_state uses read lock internally
    let state = load_or_get_cache_state(&path).ok()?;
    let required_keys = normalized_required_keys(response_validator);
    let cache_key = compute_cache_key(context, config, system_prompt, user_prompt);
    let matched = state
        .by_validator_key
        .get(&(cache_key, validator_key(&required_keys)))?;
    Some(CacheHit {
        json: matched.response_json.clone(),
    })
}

pub fn append_cache_entry(
    context: &LlmCallContext,
    config: &LlmConfig,
    system_prompt: &str,
    user_prompt: &str,
    response_validator: Option<&JsonResponseValidator>,
    response_text: &str,
    response_json: &Value,
    usage: &LlmTokenUsage,
) {
    let Some(path) = cache_file_path(context) else {
        return;
    };

    // Acquire write lock for the entire operation
    let mut guard = match cache_state_map().write() {
        Ok(v) => v,
        Err(_) => return,
    };

    // Get or create state with write lock held
    let state = guard
        .entry(path.clone())
        .or_insert_with(|| Arc::new(CacheFileState::default()));
    let required_keys = normalized_required_keys(response_validator);
    let cache_key = compute_cache_key(context, config, system_prompt, user_prompt);
    let validator_cache_key = validator_key(&required_keys);
    let entry = CacheEntry {
        version: CACHE_VERSION,
        model: config.model.clone(),
        base_url: normalize_base_url(&config.base_url),
        phase: context.phase.clone(),
        cache_key: cache_key.clone(),
        response_text: response_text.to_string(),
        response_json: response_json.clone(),
        validator_required_keys: required_keys.clone(),
        prompt_tokens: usage.prompt_tokens,
        completion_tokens: usage.completion_tokens,
        total_tokens: usage.total_tokens,
        created_at_unix_sec: unix_now_sec(),
    };

    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if append_entry_line(&path, &entry).is_err() {
        return;
    }

    // Clone to get mutable reference since we can't mutate Arc directly
    let mut state_inner = (**state).clone();
    state_inner
        .by_validator_key
        .insert((cache_key, validator_cache_key), entry);
    prune_if_needed(&path, &mut state_inner);
    *state = Arc::new(state_inner);
}

fn cache_file_path(context: &LlmCallContext) -> Option<std::path::PathBuf> {
    let media_path = context.media_path.as_deref().map(Path::new);
    let task_root = match media_path {
        Some(path) => crate::services::task_path::task_output_dir(&context.task_id, path),
        None => crate::services::task_path::task_output_dir_by_id(&context.task_id),
    };
    Some(task_root.join(CACHE_FILE_NAME))
}

fn read_entries(path: &Path) -> Result<Vec<CacheEntry>, String> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = std::fs::read_to_string(path).map_err(|err| err.to_string())?;
    if content.trim().is_empty() {
        return Ok(Vec::new());
    }
    let trimmed = content.trim_start();
    if trimmed.starts_with('[') {
        return serde_json::from_str::<Vec<CacheEntry>>(&content).map_err(|err| err.to_string());
    }

    let file = std::fs::File::open(path).map_err(|err| err.to_string())?;
    let reader = BufReader::new(file);
    let mut entries = Vec::new();
    for line in reader.lines() {
        let line = line.map_err(|err| err.to_string())?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        match serde_json::from_str::<CacheEntry>(trimmed) {
            Ok(entry) => entries.push(entry),
            Err(_) => continue,
        }
    }
    Ok(entries)
}

fn normalized_required_keys(response_validator: Option<&JsonResponseValidator>) -> Vec<String> {
    let mut keys = response_validator
        .map(|validator| validator.required_top_level_keys.clone())
        .unwrap_or_default();
    keys.sort();
    keys
}

fn normalize_base_url(base_url: &str) -> String {
    base_url.trim().trim_end_matches('/').to_string()
}

fn unix_now_sec() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn cache_state_map() -> &'static RwLock<HashMap<PathBuf, Arc<CacheFileState>>> {
    CACHE_FILE_STATE.get_or_init(|| RwLock::new(HashMap::new()))
}

fn load_or_get_cache_state(path: &Path) -> Result<Arc<CacheFileState>, String> {
    // First try with read lock
    {
        let guard = cache_state_map()
            .read()
            .map_err(|_| "cache state lock poisoned".to_string())?;
        if let Some(state) = guard.get(path) {
            if state.loaded {
                return Ok(Arc::clone(state));
            }
        }
    }

    // Not in cache or not loaded, need to write
    let mut guard = cache_state_map()
        .write()
        .map_err(|_| "cache state lock poisoned".to_string())?;

    // Double-check after acquiring write lock
    if let Some(state) = guard.get(path) {
        if state.loaded {
            return Ok(Arc::clone(state));
        }
    }

    let mut entries = read_entries(path)?;
    if entries.len() > MAX_CACHE_ENTRIES {
        let keep_from = entries.len().saturating_sub(MAX_CACHE_ENTRIES);
        entries = entries.split_off(keep_from);
    }
    let migrated = rewrite_entries_as_ndjson(path, &entries).is_ok();
    let state = Arc::new(CacheFileState {
        loaded: true,
        by_validator_key: build_entry_index(entries),
    });
    guard.insert(path.to_path_buf(), Arc::clone(&state));
    if !migrated && path.exists() {
        // Keep serving from memory even if rewrite failed.
    }
    Ok(state)
}

fn build_entry_index(entries: Vec<CacheEntry>) -> HashMap<(String, String), CacheEntry> {
    let mut map = HashMap::new();
    for entry in entries {
        if entry.version != CACHE_VERSION {
            continue;
        }
        map.insert(
            (
                entry.cache_key.clone(),
                validator_key(&entry.validator_required_keys),
            ),
            entry,
        );
    }
    map
}

fn validator_key(keys: &[String]) -> String {
    if keys.is_empty() {
        String::new()
    } else {
        keys.join("\u{1f}")
    }
}

fn append_entry_line(path: &Path, entry: &CacheEntry) -> Result<(), String> {
    let serialized = serde_json::to_string(entry).map_err(|err| err.to_string())?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|err| err.to_string())?;
    file.write_all(serialized.as_bytes())
        .and_then(|_| file.write_all(b"\n"))
        .map_err(|err| err.to_string())
}

fn rewrite_entries_as_ndjson(path: &Path, entries: &[CacheEntry]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path)
        .map_err(|err| err.to_string())?;
    for entry in entries {
        let serialized = serde_json::to_string(entry).map_err(|err| err.to_string())?;
        file.write_all(serialized.as_bytes())
            .and_then(|_| file.write_all(b"\n"))
            .map_err(|err| err.to_string())?;
    }
    Ok(())
}

fn prune_if_needed(path: &Path, state: &mut CacheFileState) {
    if state.by_validator_key.len() <= MAX_CACHE_ENTRIES {
        return;
    }
    let mut entries = state.by_validator_key.values().cloned().collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.created_at_unix_sec);
    if entries.len() > MAX_CACHE_ENTRIES {
        let keep_from = entries.len().saturating_sub(MAX_CACHE_ENTRIES);
        entries = entries.split_off(keep_from);
    }
    if rewrite_entries_as_ndjson(path, &entries).is_ok() {
        state.by_validator_key = build_entry_index(entries);
    }
}
