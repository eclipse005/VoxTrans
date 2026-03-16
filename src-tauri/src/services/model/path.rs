use std::path::PathBuf;

use super::ModelTarget;

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

pub fn open_model_dir(target: ModelTarget) -> Result<(), String> {
    let model_dir = resolve_engine_model_dir(target);
    std::fs::create_dir_all(&model_dir).map_err(|err| err.to_string())?;
    crate::services::system::open_path(&model_dir)
}
