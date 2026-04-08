use crate::services::binary::{configure_background_command, resolve_bundled_or_path};
use std::path::{Path, PathBuf};
use std::process::Command;

pub(super) fn prepare_demucs_input(
    input_path: &Path,
    output_root: &Path,
) -> Result<PathBuf, String> {
    let is_wav = input_path
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.eq_ignore_ascii_case("wav"))
        .unwrap_or(false);
    if is_wav {
        return Ok(input_path.to_path_buf());
    }

    let wav_input_path = output_root.join("demucs_input.wav");
    extract_audio_with_ffmpeg(input_path, &wav_input_path)?;
    Ok(wav_input_path)
}

fn extract_audio_with_ffmpeg(input_path: &Path, output_wav: &Path) -> Result<(), String> {
    let ffmpeg_bin = resolve_bundled_or_path("ffmpeg");
    let mut command = Command::new(&ffmpeg_bin);
    configure_background_command(&mut command);
    let output = command
        .arg("-y")
        .arg("-i")
        .arg(input_path)
        .arg("-vn")
        .arg("-ac")
        .arg("1")
        .arg("-c:a")
        .arg("pcm_s16le")
        .arg(output_wav)
        .output()
        .map_err(|err| format!("提取音频失败: 调用 ffmpeg 失败: {err}"))?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if stderr.is_empty() {
        return Err("提取音频失败: ffmpeg 执行失败".to_string());
    }
    Err(format!("提取音频失败: {stderr}"))
}
