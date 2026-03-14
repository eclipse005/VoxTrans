use std::path::PathBuf;

pub fn resolve_model_dir() -> PathBuf {
    if let Ok(custom_dir) = std::env::var("VOXTRANS_MODEL_DIR") {
        let path = PathBuf::from(custom_dir);
        if !path.as_os_str().is_empty() {
            return path;
        }
    }

    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));
    exe_dir.join("model")
}

pub fn open_model_dir() -> Result<(), String> {
    let model_dir = resolve_model_dir();
    std::fs::create_dir_all(&model_dir).map_err(|err| err.to_string())?;
    crate::services::system::open_path(&model_dir)
}
