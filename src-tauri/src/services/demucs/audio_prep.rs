use std::path::{Path, PathBuf};

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{CODEC_TYPE_NULL, DecoderOptions};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::default::{get_codecs, get_probe};

pub(super) fn prepare_demucs_input(input_path: &Path, output_root: &Path) -> Result<PathBuf, String> {
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
