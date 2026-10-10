use std::path::{Path, PathBuf};
use std::sync::RwLock;

static OUTPUT_ROOT_OVERRIDE: RwLock<Option<PathBuf>> = RwLock::new(None);

/// Pin the output root for this process. Set once by the CLI (`--out`) so every
/// path helper built on `resolve_output_dir()` — task folders, task logs, flat
/// SRT exports, the app log — lands under the caller's directory instead of
/// next to the executable. The desktop app never calls this: unset (the
/// default) means the exe-relative behaviour below is unchanged.
pub fn set_output_root_override(path: Option<PathBuf>) {
    if let Ok(mut guard) = OUTPUT_ROOT_OVERRIDE.write() {
        *guard = path;
    }
}

pub fn resolve_output_dir() -> PathBuf {
    if let Ok(guard) = OUTPUT_ROOT_OVERRIDE.read()
        && let Some(ref path) = *guard
            && !path.as_os_str().is_empty()
    {
        return path.clone();
    }

    let base_dir = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|dir| dir.to_path_buf()))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let exe_output_dir = base_dir.join("output");
    if ensure_writable_dir(&exe_output_dir) {
        return exe_output_dir;
    }

    if let Some(fallback_dir) = platform_fallback_output_dir()
        && ensure_writable_dir(&fallback_dir)
    {
        return fallback_dir;
    }

    exe_output_dir
}

fn ensure_writable_dir(path: &Path) -> bool {
    if std::fs::create_dir_all(path).is_err() {
        return false;
    }

    let probe_path = path.join(".voxtrans_write_test");
    match std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&probe_path)
    {
        Ok(_) => {
            let _ = std::fs::remove_file(probe_path);
            true
        }
        Err(_) => false,
    }
}

fn platform_fallback_output_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
            let path = PathBuf::from(local_app_data);
            if !path.as_os_str().is_empty() {
                return Some(path.join("VoxTrans").join("output"));
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        if let Ok(home) = std::env::var("HOME") {
            let path = PathBuf::from(home);
            if !path.as_os_str().is_empty() {
                return Some(
                    path.join("Library")
                        .join("Application Support")
                        .join("VoxTrans")
                        .join("output"),
                );
            }
        }
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Ok(data_home) = std::env::var("XDG_DATA_HOME") {
            let path = PathBuf::from(data_home);
            if !path.as_os_str().is_empty() {
                return Some(path.join("voxtrans").join("output"));
            }
        }
        if let Ok(home) = std::env::var("HOME") {
            let path = PathBuf::from(home);
            if !path.as_os_str().is_empty() {
                return Some(
                    path.join(".local")
                        .join("share")
                        .join("voxtrans")
                        .join("output"),
                );
            }
        }
    }

    None
}
