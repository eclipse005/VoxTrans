use std::path::{Path, PathBuf};

pub(super) fn find_vocals_path(output_root: &Path, input_path: &Path) -> Option<PathBuf> {
    let direct = output_root.join("vocals.wav");
    if direct.is_file() {
        return Some(direct);
    }

    if let Some(stem) = input_path.file_stem().and_then(|s| s.to_str()) {
        let nested = output_root.join(stem).join("vocals.wav");
        if nested.is_file() {
            return Some(nested);
        }
    }

    let mut stack = vec![output_root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(v) => v,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path
                .file_name()
                .and_then(|name| name.to_str())
                .map(|name| name.eq_ignore_ascii_case("vocals.wav"))
                .unwrap_or(false)
            {
                return Some(path);
            }
        }
    }

    None
}
