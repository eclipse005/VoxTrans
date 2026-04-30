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
    input_path: &Path,
) -> Result<PreparedAudio, Box<dyn std::error::Error>> {
    let (mono_samples, _ffmpeg_input) =
        load_input_as_16k_mono_f32_with_extractor(input_path, extract_audio_to_wav)?;
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

fn load_input_as_16k_mono_f32_with_extractor<F>(
    input_path: &Path,
    mut extract_audio: F,
) -> Result<(Vec<f32>, Option<TemporaryAudioFile>), Box<dyn std::error::Error>>
where
    F: FnMut(&Path) -> Result<TemporaryAudioFile, Box<dyn std::error::Error>>,
{
    if is_wav_path(input_path)
        && let Ok(samples) = load_wav_as_16k_mono_f32(input_path)
    {
        return Ok((samples, None));
    }

    let ffmpeg_input = extract_audio(input_path)?;
    let samples = load_wav_as_16k_mono_f32(&ffmpeg_input.path)?;
    Ok((samples, Some(ffmpeg_input)))
}

fn load_wav_as_16k_mono_f32(audio_path: &Path) -> Result<Vec<f32>, Box<dyn std::error::Error>> {
    let mut reader = hound::WavReader::open(audio_path)?;
    let spec = reader.spec();
    let channels = spec.channels as usize;
    if channels == 0 {
        return Err(format!("invalid wav channels=0 ({})", audio_path.display()).into());
    }

    let mono_samples = match spec.sample_format {
        hound::SampleFormat::Float => {
            let samples = reader.samples::<f32>().collect::<Result<Vec<_>, _>>()?;
            downmix_to_mono(&samples, channels)
        }
        hound::SampleFormat::Int => {
            let bits = spec.bits_per_sample.clamp(1, 32);
            if bits <= 16 {
                let max = (1_i64 << (bits - 1)) as f32;
                let samples = reader
                    .samples::<i16>()
                    .map(|sample| sample.map(|value| value as f32 / max))
                    .collect::<Result<Vec<_>, _>>()?;
                downmix_to_mono(&samples, channels)
            } else {
                let max = (1_i64 << (bits - 1)) as f32;
                let samples = reader
                    .samples::<i32>()
                    .map(|sample| sample.map(|value| value as f32 / max))
                    .collect::<Result<Vec<_>, _>>()?;
                downmix_to_mono(&samples, channels)
            }
        }
    };

    if spec.sample_rate == TARGET_SAMPLE_RATE {
        Ok(mono_samples)
    } else {
        Ok(resample_linear(
            &mono_samples,
            spec.sample_rate as usize,
            TARGET_SAMPLE_RATE as usize,
        ))
    }
}

fn extract_audio_to_wav(
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

fn downmix_to_mono(samples: &[f32], channels: usize) -> Vec<f32> {
    if channels == 1 {
        return samples.to_vec();
    }
    samples
        .chunks_exact(channels)
        .map(|frame| frame.iter().copied().sum::<f32>() / channels as f32)
        .collect()
}

fn resample_linear(samples: &[f32], src_sr: usize, dst_sr: usize) -> Vec<f32> {
    if samples.is_empty() || src_sr == dst_sr {
        return samples.to_vec();
    }

    let new_len = ((samples.len() as f64) * (dst_sr as f64) / (src_sr as f64)).round() as usize;
    let ratio = src_sr as f64 / dst_sr as f64;
    let mut out = Vec::with_capacity(new_len);
    for i in 0..new_len {
        let pos = i as f64 * ratio;
        let idx = pos.floor() as usize;
        let frac = (pos - idx as f64) as f32;
        let a = samples.get(idx).copied().unwrap_or(0.0);
        let b = samples.get(idx + 1).copied().unwrap_or(a);
        out.push(a + (b - a) * frac);
    }
    out
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

fn is_wav_path(input_path: &Path) -> bool {
    let ext = input_path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    ext == "wav"
}

fn build_temp_wav_path(tag: &str) -> PathBuf {
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("voxtrans_tmp_{}_{}_{}.wav", pid, tag, nanos))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_loader_downmixes_and_resamples_wav_to_16k_mono() {
        let path = build_temp_wav_path("stereo_44k_test");
        write_stereo_44k_test_wav(&path);

        let samples = load_wav_as_16k_mono_f32(&path).unwrap();

        let _ = fs::remove_file(&path);
        assert_eq!(samples.len(), 1600);
        assert!((samples[100] - 0.375).abs() < 0.01);
    }

    #[test]
    fn wav_path_falls_back_to_extractor_when_hound_cannot_read_it() {
        let bad_wav_path = build_temp_wav_path("bad_wav_test");
        fs::write(&bad_wav_path, b"not a readable wav").unwrap();
        let converted_wav_path = build_temp_wav_path("converted_wav_test");
        write_stereo_44k_test_wav(&converted_wav_path);

        let mut extractor_called = false;
        let (samples, _converted) =
            load_input_as_16k_mono_f32_with_extractor(&bad_wav_path, |input| {
                assert_eq!(input, bad_wav_path.as_path());
                extractor_called = true;
                Ok(TemporaryAudioFile {
                    path: converted_wav_path.clone(),
                })
            })
            .unwrap();

        let _ = fs::remove_file(&bad_wav_path);
        assert!(extractor_called);
        assert_eq!(samples.len(), 1600);
    }

    fn write_stereo_44k_test_wav(path: &Path) {
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: 44_100,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(path, spec).unwrap();
        for _ in 0..4410 {
            writer.write_sample(8192_i16).unwrap();
            writer.write_sample(16384_i16).unwrap();
        }
        writer.finalize().unwrap();
    }
}
