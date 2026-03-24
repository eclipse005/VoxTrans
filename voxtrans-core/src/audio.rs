use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::TARGET_SAMPLE_RATE;
use crate::binary::{configure_background_command, resolve_bundled_or_path};

pub(crate) struct PreparedAudio {
    pub mono_samples: Vec<f32>,
    pub duration_sec: f64,
    pub vad_wav: TemporaryAudioFile,
}

pub(crate) struct TemporaryAudioFile {
    pub path: PathBuf,
}

impl Drop for TemporaryAudioFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

pub(crate) fn prepare_audio_for_transcription(
    input_path: &PathBuf,
) -> Result<PreparedAudio, Box<dyn std::error::Error>> {
    let ffmpeg_input = if is_wav_16k_mono(input_path)? {
        None
    } else {
        Some(convert_audio_to_wav_16k_mono(input_path)?)
    };
    let wav_path = ffmpeg_input
        .as_ref()
        .map(|file| &file.path)
        .unwrap_or(input_path);
    let mono_samples = load_wav_mono_f32(wav_path)?;
    if mono_samples.is_empty() {
        return Err(format!("no audio samples decoded from {}", input_path.display()).into());
    }

    let duration_sec = mono_samples.len() as f64 / TARGET_SAMPLE_RATE as f64;
    let vad_wav_path = build_temp_wav_path("vad_input");
    write_wav_mono_16k_i16(&vad_wav_path, &mono_samples)?;

    Ok(PreparedAudio {
        mono_samples,
        duration_sec,
        vad_wav: TemporaryAudioFile { path: vad_wav_path },
    })
}

fn load_wav_mono_f32(audio_path: &Path) -> Result<Vec<f32>, Box<dyn std::error::Error>> {
    let mut reader = hound::WavReader::open(audio_path)?;
    let spec = reader.spec();

    if spec.sample_rate != TARGET_SAMPLE_RATE || spec.channels != 1 {
        return Err(format!(
            "expected {}Hz mono wav, got {}Hz {}ch ({})",
            TARGET_SAMPLE_RATE,
            spec.sample_rate,
            spec.channels,
            audio_path.display()
        )
        .into());
    }

    let samples = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().collect::<Result<Vec<_>, _>>()?,
        hound::SampleFormat::Int => {
            let bits = spec.bits_per_sample.clamp(1, 32);
            let max = (1_i64 << (bits - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.map(|v| v as f32 / max))
                .collect::<Result<Vec<_>, _>>()?
        }
    };
    Ok(samples)
}

fn convert_audio_to_wav_16k_mono(
    input_path: &Path,
) -> Result<TemporaryAudioFile, Box<dyn std::error::Error>> {
    let output_path = build_temp_wav_path("ffmpeg_input");
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
        .arg("-ar")
        .arg(TARGET_SAMPLE_RATE.to_string())
        .arg("-c:a")
        .arg("pcm_s16le")
        .arg(&output_path)
        .output()?;

    if output.status.success() {
        return Ok(TemporaryAudioFile { path: output_path });
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if stderr.is_empty() {
        return Err("ffmpeg failed to extract audio".into());
    }
    Err(format!("ffmpeg failed to extract audio: {stderr}").into())
}

fn write_wav_mono_16k_i16(path: &Path, samples: &[f32]) -> Result<(), Box<dyn std::error::Error>> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: TARGET_SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec)?;
    for &sample in samples {
        let s = sample.clamp(-1.0, 1.0);
        let v = (s * i16::MAX as f32) as i16;
        writer.write_sample(v)?;
    }
    writer.finalize()?;
    Ok(())
}

fn is_wav_16k_mono(input_path: &PathBuf) -> Result<bool, Box<dyn std::error::Error>> {
    let ext = input_path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if ext != "wav" {
        return Ok(false);
    }

    let reader = match hound::WavReader::open(input_path) {
        Ok(reader) => reader,
        Err(_) => return Ok(false),
    };
    let spec = reader.spec();
    Ok(spec.sample_rate == 16_000 && spec.channels == 1)
}

fn build_temp_wav_path(tag: &str) -> PathBuf {
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("voxtrans_tmp_{}_{}_{}.wav", pid, tag, nanos))
}
