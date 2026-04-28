use std::path::Path;

use serde::Serialize;
use serde::de::DeserializeOwned;

pub(super) fn write_json_file<T: Serialize>(path: &Path, payload: &T) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let content = serde_json::to_string_pretty(payload).map_err(|err| err.to_string())?;
    std::fs::write(path, content.as_bytes()).map_err(|err| err.to_string())
}

pub(super) fn read_json_file_if_exists<T: DeserializeOwned>(
    path: &Path,
) -> Result<Option<T>, String> {
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(path).map_err(|err| err.to_string())?;
    let parsed = serde_json::from_str::<T>(&raw).ok();
    Ok(parsed)
}
