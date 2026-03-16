use serde::{Deserialize, Serialize};
use serde_json::json;
use serde_json::Value;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{CODEC_TYPE_NULL, DecoderOptions};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::default::{get_codecs, get_probe};

use crate::services::task_log::{TaskLogTarget, append_event_best_effort};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeparateVocalsRequest {
    pub task_id: String,
    pub audio_path: String,
    pub model: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SeparateVocalsResponse {
    pub vocals_path: String,
}

pub fn separate_vocals_blocking<F>(
    request: SeparateVocalsRequest,
    mut on_progress: F,
) -> Result<SeparateVocalsResponse, String>
where
    F: FnMut(u32),
{
    let started_at = std::time::Instant::now();
    let log_target = TaskLogTarget::main(request.task_id.clone(), request.audio_path.clone());
    let demucs_model_dir = crate::services::model::resolve_engine_model_dir(
        crate::services::model::ModelTarget::Demucs,
    );
    let demucs_model_file = demucs_model_dir.join(format!("{}.safetensors", request.model));
    if !demucs_model_file.is_file() {
        let err = format!(
            "人声分离模型未就绪: {}。请先到模型中心下载后再试。",
            demucs_model_file.display()
        );
        append_event_best_effort(
            &log_target,
            "demucs.failed",
            Some(&json!({
                "error": err,
                "elapsedSec": round2(started_at.elapsed().as_secs_f64()),
            })),
        );
        return Err(err);
    }

    let input_path = PathBuf::from(&request.audio_path);
    let output_root = crate::services::task_path::task_output_dir(&request.task_id, &input_path)
        .join("demucs")
        .join(&request.model);
    std::fs::create_dir_all(&output_root).map_err(|err| err.to_string())?;
    let demucs_input = match prepare_demucs_input(&input_path, &output_root) {
        Ok(path) => path,
        Err(err) => {
            append_event_best_effort(
                &log_target,
                "demucs.failed",
                Some(&json!({
                    "error": err,
                    "elapsedSec": round2(started_at.elapsed().as_secs_f64()),
                })),
            );
            return Err(err);
        }
    };
    append_event_best_effort(
        &log_target,
        "demucs.started",
        Some(&json!({
            "model": request.model,
            "inputPath": input_path.display().to_string(),
            "demucsInputPath": demucs_input.display().to_string(),
        })),
    );

    let mut child = demucs_command()
        .arg("--model")
        .arg(&request.model)
        .arg("--model-dir")
        .arg(demucs_model_dir)
        .arg("--stems")
        .arg("vocals")
        .arg("--json-progress")
        .arg("-o")
        .arg(&output_root)
        .arg(&demucs_input)
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

        if let Ok(json_line) = serde_json::from_str::<Value>(line.trim()) {
            let event_type = json_line.get("type").and_then(Value::as_str).unwrap_or_default();
            let event = json_line
                .get("event")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if event_type == "separation" && event == "progress" {
                let percent = if let Some(v) = json_line.get("percent").and_then(Value::as_f64) {
                    v.round() as u32
                } else {
                    let current = json_line.get("current").and_then(Value::as_f64).unwrap_or(0.0);
                    let total = json_line.get("total").and_then(Value::as_f64).unwrap_or(0.0);
                    if total > 0.0 {
                        ((current / total) * 100.0).round() as u32
                    } else {
                        0
                    }
                };
                on_progress(percent.clamp(0, 100));
            } else if event_type == "separation" && event == "done" {
                on_progress(100);
            }
        }
    }

    let output = child
        .wait_with_output()
        .map_err(|err| format!("等待 demucs 结束失败: {}", err))?;
    if !output.status.success() {
        let err = format!(
            "demucs 分离失败: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        append_event_best_effort(
            &log_target,
            "demucs.failed",
            Some(&json!({
                "error": err,
                "elapsedSec": round2(started_at.elapsed().as_secs_f64()),
            })),
        );
        return Err(err);
    }

    let vocals_path = find_vocals_path(&output_root, &demucs_input)
        .ok_or_else(|| format!("未找到 vocals.wav 输出: {}", output_root.display()))?;
    append_event_best_effort(
        &log_target,
        "demucs.completed",
        Some(&json!({
            "vocalsPath": vocals_path.display().to_string(),
            "elapsedSec": round2(started_at.elapsed().as_secs_f64()),
        })),
    );
    Ok(SeparateVocalsResponse {
        vocals_path: vocals_path.display().to_string(),
    })
}

fn demucs_command() -> Command {
    let mut cmd = Command::new(resolve_demucs_program());
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW
        cmd.creation_flags(0x08000000);
    }
    cmd
}

fn resolve_demucs_program() -> PathBuf {
    if let Ok(custom) = std::env::var("VOXTRANS_DEMUCS_BIN") {
        let path = PathBuf::from(custom);
        if path.exists() {
            return path;
        }
    }

    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            #[cfg(target_os = "windows")]
            let bundled = exe_dir.join("bin").join("demucs.exe");
            #[cfg(not(target_os = "windows"))]
            let bundled = exe_dir.join("bin").join("demucs");
            if bundled.exists() {
                return bundled;
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        PathBuf::from("demucs.exe")
    }
    #[cfg(not(target_os = "windows"))]
    {
        PathBuf::from("demucs")
    }
}

fn find_vocals_path(output_root: &Path, input_path: &Path) -> Option<PathBuf> {
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

fn prepare_demucs_input(input_path: &Path, output_root: &Path) -> Result<PathBuf, String> {
    let is_wav = input_path
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.eq_ignore_ascii_case("wav"))
        .unwrap_or(false);
    if is_wav {
        return Ok(input_path.to_path_buf());
    }

    let decoded = decode_audio_with_symphonia(input_path)
        .map_err(|err| format!("提取音频失败: {}", err))?;
    let mono = downmix_to_mono(decoded.samples, decoded.channels);
    if mono.is_empty() {
        return Err("提取音频失败: 音频内容为空".to_string());
    }
    let wav_input_path = output_root.join("demucs_input.wav");
    write_wav_mono_i16(&wav_input_path, decoded.sample_rate, &mono)
        .map_err(|err| format!("写入 demucs 输入 wav 失败: {}", err))?;
    Ok(wav_input_path)
}

struct DecodedAudio {
    samples: Vec<f32>,
    sample_rate: u32,
    channels: u16,
}

fn decode_audio_with_symphonia(
    audio_path: &Path,
) -> Result<DecodedAudio, Box<dyn std::error::Error>> {
    let src = std::fs::File::open(audio_path)?;
    let mss = MediaSourceStream::new(Box::new(src), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = audio_path.extension().and_then(|v| v.to_str()) {
        hint.with_extension(ext);
    }
    let probed = get_probe().format(
        &hint,
        mss,
        &FormatOptions::default(),
        &MetadataOptions::default(),
    )?;
    let mut format = probed.format;

    let track = format
        .tracks()
        .iter()
        .find(|t| {
            t.codec_params.codec != CODEC_TYPE_NULL
                && (t.codec_params.channels.is_some() || t.codec_params.sample_rate.is_some())
        })
        .ok_or_else(|| format!("no audio track found in {}", audio_path.display()))?;

    let mut decoder = get_codecs().make(&track.codec_params, &DecoderOptions::default())?;
    let track_id = track.id;
    let mut sample_rate = track.codec_params.sample_rate.unwrap_or(44_100);
    let mut channels = track
        .codec_params
        .channels
        .map(|v| v.count() as u16)
        .unwrap_or(1);
    let mut interleaved = Vec::<f32>::new();

    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(SymphoniaError::IoError(err))
                if err.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break;
            }
            Err(err) => return Err(err.into()),
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(decoded) => decoded,
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(SymphoniaError::IoError(err))
                if err.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break;
            }
            Err(err) => return Err(err.into()),
        };
        let spec = *decoded.spec();
        sample_rate = spec.rate;
        channels = spec.channels.count() as u16;

        let mut samples = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
        samples.copy_interleaved_ref(decoded);
        interleaved.extend_from_slice(samples.samples());
    }

    if interleaved.is_empty() {
        return Err(format!("decoded audio is empty: {}", audio_path.display()).into());
    }
    Ok(DecodedAudio {
        samples: interleaved,
        sample_rate,
        channels,
    })
}

fn downmix_to_mono(samples: Vec<f32>, channels: u16) -> Vec<f32> {
    if channels <= 1 {
        return samples;
    }
    let ch = channels as usize;
    samples
        .chunks(ch)
        .map(|frame| frame.iter().copied().sum::<f32>() / frame.len() as f32)
        .collect()
}

fn write_wav_mono_i16(
    path: &Path,
    sample_rate: u32,
    samples: &[f32],
) -> Result<(), Box<dyn std::error::Error>> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: sample_rate.max(8_000),
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec)?;
    for &sample in samples {
        let s = sample.clamp(-1.0, 1.0);
        writer.write_sample((s * i16::MAX as f32) as i16)?;
    }
    writer.finalize()?;
    Ok(())
}

fn round2(value: f64) -> f64 {
    if !value.is_finite() {
        return 0.0;
    }
    (value * 100.0).round() / 100.0
}
