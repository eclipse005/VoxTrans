use std::path::PathBuf;
use std::sync::RwLock;

use super::{ModelTarget, model_definition};

static MODELS_ROOT_OVERRIDE: RwLock<Option<PathBuf>> = RwLock::new(None);

/// Update the runtime model-root override. Called when settings are loaded or
/// saved, so `resolve_models_root()` always reflects the user's configured path.
pub fn set_models_root_override(path: Option<PathBuf>) {
    if let Ok(mut guard) = MODELS_ROOT_OVERRIDE.write() {
        *guard = path;
    }
}

/// Resolve the root model storage directory. Priority:
/// 1. Runtime override (from user settings → `set_models_root_override`)
/// 2. `VOXTRANS_MODELS_DIR` environment variable
/// 3. `<exe_dir>/models` (default)
pub fn resolve_models_root() -> PathBuf {
    if let Ok(guard) = MODELS_ROOT_OVERRIDE.read() {
        if let Some(ref path) = *guard {
            if !path.as_os_str().is_empty() {
                return path.clone();
            }
        }
    }

    if let Ok(custom_dir) = std::env::var("VOXTRANS_MODELS_DIR") {
        let path = PathBuf::from(custom_dir);
        if !path.as_os_str().is_empty() {
            return path;
        }
    }

    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));
    exe_dir.join("models")
}

pub fn resolve_engine_model_dir(target: ModelTarget) -> PathBuf {
    resolve_models_root().join(target.dir_name())
}

pub fn resolve_asr_model_dir(model: &str) -> PathBuf {
    resolve_model_dir(ModelTarget::Asr, model)
}

pub fn resolve_aligner_model_dir(model: &str) -> PathBuf {
    resolve_model_dir(ModelTarget::Align, model)
}

pub fn resolve_model_dir(target: ModelTarget, model: &str) -> PathBuf {
    match target {
        ModelTarget::Asr | ModelTarget::Align => resolve_models_root().join(model),
        ModelTarget::Demucs => resolve_engine_model_dir(target),
    }
}

pub fn open_model_dir(target: ModelTarget, model: Option<String>) -> Result<(), String> {
    let definition = model_definition(target, model.as_deref())?;
    let model_dir = definition.model_dir;
    std::fs::create_dir_all(&model_dir).map_err(|err| err.to_string())?;
    crate::services::system::open_path(&model_dir)
}
