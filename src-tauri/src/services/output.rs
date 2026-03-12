use std::path::PathBuf;

pub fn resolve_output_dir() -> PathBuf {
    if let Ok(custom_dir) = std::env::var("VOXTRANS_OUTPUT_DIR") {
        let path = PathBuf::from(custom_dir);
        if !path.as_os_str().is_empty() {
            return path;
        }
    }

    let tauri_manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let project_root = tauri_manifest_dir
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or(tauri_manifest_dir);
    project_root.join("output")
}
