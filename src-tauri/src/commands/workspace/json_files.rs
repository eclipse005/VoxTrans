use std::io::ErrorKind;
use std::path::Path;

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::domain::error::{WorkspaceError, WorkspaceResult};

pub(super) fn write_json_file<T: Serialize>(path: &Path, payload: &T) -> WorkspaceResult<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_string_pretty(payload)
        .map_err(|err| WorkspaceError::Serialization(err.to_string()))?;
    Ok(std::fs::write(path, content.as_bytes())?)
}

pub(super) fn read_json_file_if_exists<T: DeserializeOwned>(
    path: &Path,
) -> WorkspaceResult<Option<T>> {
    match std::fs::metadata(path) {
        Ok(_) => {}
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(WorkspaceError::Io(err)),
    }
    let raw = std::fs::read_to_string(path)?;
    let parsed = serde_json::from_str::<T>(&raw)
        .map_err(|err| WorkspaceError::Serialization(err.to_string()))?;
    Ok(Some(parsed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_json_file_if_exists_reports_invalid_json_as_serialization_error() {
        let path =
            std::env::temp_dir().join(format!("voxtrans-invalid-json-{}.json", std::process::id()));
        std::fs::write(&path, "{invalid json").expect("write invalid json");

        let err = read_json_file_if_exists::<serde_json::Value>(&path)
            .expect_err("invalid json should fail");

        let _ = std::fs::remove_file(path);
        assert_eq!(err.code(), "SERIALIZATION_ERROR");
    }

    #[test]
    fn read_json_file_if_exists_reports_metadata_failure_as_io_error() {
        let path =
            std::env::temp_dir().join(format!("voxtrans-invalid-path-\0-{}", std::process::id()));

        let err = read_json_file_if_exists::<serde_json::Value>(&path)
            .expect_err("invalid path should fail metadata lookup");

        assert_eq!(err.code(), "IO_ERROR");
    }

    #[test]
    fn write_json_file_reports_parent_creation_failure_as_io_error() {
        let parent =
            std::env::temp_dir().join(format!("voxtrans-parent-file-{}", std::process::id()));
        std::fs::write(&parent, "not a directory").expect("write parent file");
        let path = parent.join("task_meta.json");

        let err = write_json_file(
            &path,
            &serde_json::json!({
                "value": "ok"
            }),
        )
        .expect_err("file parent should fail directory creation");

        let _ = std::fs::remove_file(parent);
        assert_eq!(err.code(), "IO_ERROR");
    }
}
