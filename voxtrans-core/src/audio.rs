use std::fs;
use std::path::{Path, PathBuf};

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{CODEC_TYPE_NULL, DecoderOptions};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::default::{get_codecs, get_probe};

use crate::TARGET_SAMPLE_RATE;

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
    let mono_samples = if is_wav_16k_mono(input_path)? {
        load_wav_mono_f32(input_path)?
    } else {
        let decoded = decode_audio_with_symphonia(input_path)?;
        normalize_audio_for_asr(decoded.samples, decoded.sample_rate, decoded.channels)
    };
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

struct DecodedAudio {
    samples: Vec<f32>,
    sample_rate: u32,
    channels: u16,
}

fn decode_audio_with_symphonia(audio_path: &Path) -> Result<DecodedAudio, Box<dyn std::error::Error>> {
    let src = fs::File::open(audio_path)?;
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
    let mut sample_rate = track.codec_params.sample_rate.unwrap_or(TARGET_SAMPLE_RATE);
    let mut channels = track
        .codec_params
        .channels
        .map(|v| v.count() as u16)
        .unwrap_or(1);
    let mut interleaved = Vec::<f32>::new();

    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(SymphoniaError::IoError(err)) if err.kind() == std::io::ErrorKind::UnexpectedEof => {
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
            Err(SymphoniaError::IoError(err)) if err.kind() == std::io::ErrorKind::UnexpectedEof => {
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

fn normalize_audio_for_asr(samples: Vec<f32>, sample_rate: u32, channels: u16) -> Vec<f32> {
    let mono = downmix_to_mono(samples, channels);
    resample_linear(&mono, sample_rate, TARGET_SAMPLE_RATE)
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

fn resample_linear(samples: &[f32], src_rate: u32, dst_rate: u32) -> Vec<f32> {
    if src_rate == dst_rate {
        return samples.to_vec();
    }
    if samples.is_empty() || src_rate == 0 || dst_rate == 0 {
        return Vec::new();
    }

    let dst_len =
        ((samples.len() as u64 * dst_rate as u64 + src_rate as u64 - 1) / src_rate as u64) as usize;
    let mut out = Vec::with_capacity(dst_len);
    let scale = src_rate as f64 / dst_rate as f64;

    for i in 0..dst_len {
        let src_pos = i as f64 * scale;
        let left = src_pos.floor() as usize;
        let frac = (src_pos - left as f64) as f32;
        let a = samples[left.min(samples.len() - 1)];
        let b = samples[(left + 1).min(samples.len() - 1)];
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
