use std::path::PathBuf;

use super::{DEFAULT_ALIGN_MODEL, ModelTarget, model_definition};

pub fn resolve_models_root() -> PathBuf {
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

#[allow(dead_code)]
pub fn default_aligner_model_dir() -> PathBuf {
    resolve_aligner_model_dir(DEFAULT_ALIGN_MODEL)
}
