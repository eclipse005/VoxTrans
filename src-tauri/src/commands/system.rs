use std::path::PathBuf;

#[tauri::command]
pub fn open_in_explorer(path: String) -> Result<(), String> {
    let target = PathBuf::from(path);
    crate::services::system::open_path(&target)
}

#[tauri::command]
pub fn open_output_dir() -> Result<(), String> {
    let output_dir = crate::services::output::resolve_output_dir();
    std::fs::create_dir_all(&output_dir).map_err(|err| err.to_string())?;
    crate::services::system::open_path(&output_dir)
}
