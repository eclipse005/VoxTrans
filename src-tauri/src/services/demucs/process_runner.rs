use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::progress_parse::parse_progress_percent;
use crate::services::binary::{configure_background_command, resolve_bundled_or_path};

pub(super) fn run_demucs_with_progress<F>(
    model: &str,
    model_dir: &Path,
    output_root: &Path,
    demucs_input: &Path,
    mut on_progress: F,
) -> Result<(), String>
where
    F: FnMut(u32),
{
    let mut child = demucs_command()
        .arg("--model")
        .arg(model)
        .arg("--model-dir")
        .arg(model_dir)
        .arg("--stems")
        .arg("vocals")
        .arg("--json-progress")
        .arg("-o")
        .arg(output_root)
        .arg(demucs_input)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| format!("启动 demucs 失败: {}", err))?;

    on_progress(0);
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "读取 demucs 标准输出失败".to_string())?;
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();

    loop {
        line.clear();
        let read = reader.read_line(&mut line).map_err(|err| err.to_string())?;
        if read == 0 {
            break;
        }

        if let Some(percent) = parse_progress_percent(line.trim()) {
            on_progress(percent);
        }
    }

    let output = child
        .wait_with_output()
        .map_err(|err| format!("等待 demucs 结束失败: {}", err))?;
    if !output.status.success() {
        return Err(format!(
            "demucs 分离失败: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    Ok(())
}

fn demucs_command() -> Command {
    let mut cmd = Command::new(resolve_demucs_program());
    configure_background_command(&mut cmd);
    cmd
}

fn resolve_demucs_program() -> PathBuf {
    if let Ok(custom) = std::env::var("VOXTRANS_DEMUCS_BIN") {
        let path = PathBuf::from(custom);
        if path.exists() {
            return path;
        }
    }

    resolve_bundled_or_path("demucs")
}
