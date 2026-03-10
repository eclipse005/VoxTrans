use std::path::{Path, PathBuf};
use std::time::Instant;
use std::{fs, process::Command};

use parakeet_rs::{
    ExecutionConfig, ExecutionProvider, ParakeetTDT, TimedToken, TimestampMode, Transcriber,
    TranscriptionResult,
};

const CHUNK_TARGET_SECONDS: f64 = 300.0;
const SILENCE_NOISE_DB: &str = "-35dB";
const SILENCE_MIN_SECONDS: &str = "0.5";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = parse_args(std::env::args().skip(1).collect())?;

    if matches!(args.provider, ExecutionProvider::Cuda) {
        let ort_dir = args
            .ort_dir
            .clone()
            .unwrap_or_else(|| PathBuf::from(r"D:\voxtrans\runtime\onnxruntime-sm61"));
        let ort_dll = ort_dir.join("onnxruntime.dll");
        let old_path = std::env::var("PATH").unwrap_or_default();
        let merged_path = format!("{};{}", ort_dir.display(), old_path);
        unsafe {
            std::env::set_var("ORT_DYLIB_PATH", ort_dll.as_os_str());
            std::env::set_var("PATH", merged_path);
        }
    }

    let prepared_audio = prepare_audio_for_transcription(&args.audio_path)?;
    let audio_duration_sec = wav_duration_seconds(prepared_audio.path())?;
    let segments = build_segments_from_silence(prepared_audio.path(), audio_duration_sec)?;

    println!(
        "segment_count: {} (target_chunk_sec={:.0})",
        segments.len(),
        CHUNK_TARGET_SECONDS
    );
    for segment in &segments {
        println!(
            "segment_{}: duration={:.3}s",
            segment.index + 1,
            segment.duration_sec()
        );
    }

    let started_at = Instant::now();
    let result = transcribe_in_segments(
        &args.model_dir,
        prepared_audio.path(),
        args.provider,
        args.timestamp_mode,
        &segments,
    )?;
    let result = merge_punctuation_tokens(result);
    let elapsed_sec = started_at.elapsed().as_secs_f64();
    let rtfx = if elapsed_sec > 0.0 {
        audio_duration_sec / elapsed_sec
    } else {
        0.0
    };

    println!(
        "execution_provider: {}",
        match args.provider {
            ExecutionProvider::Cuda => "cuda",
            _ => "cpu",
        }
    );
    println!("{}", result.text);
    for token in result.tokens {
        println!("[{:.3}s - {:.3}s] {}", token.start, token.end, token.text);
    }
    println!("audio_duration_sec: {:.3}", audio_duration_sec);
    println!("transcribe_elapsed_sec: {:.3}", elapsed_sec);
    println!("RTFx: {:.4}", rtfx);

    Ok(())
}

fn transcribe_in_segments(
    model_dir: &PathBuf,
    full_audio_path: &Path,
    provider: ExecutionProvider,
    timestamp_mode: TimestampMode,
    segments: &[AudioSegment],
) -> Result<TranscriptionResult, Box<dyn std::error::Error>> {
    let mut model = ParakeetTDT::from_pretrained(
        model_dir,
        Some(ExecutionConfig::new().with_execution_provider(provider)),
    )?;
    let mut all_tokens: Vec<TimedToken> = Vec::new();
    let mut text_parts: Vec<String> = Vec::new();

    for segment in segments {
        let segment_file = extract_segment_to_temp(full_audio_path, segment)?;
        let mut segment_result = model.transcribe_file(segment_file.path.as_path(), Some(timestamp_mode))?;

        if !segment_result.text.trim().is_empty() {
            text_parts.push(segment_result.text.trim().to_string());
        }
        for token in &mut segment_result.tokens {
            token.start += segment.start_sec as f32;
            token.end += segment.start_sec as f32;
        }
        all_tokens.extend(segment_result.tokens);
    }

    let merged_text = text_parts.join(" ");
    Ok(TranscriptionResult {
        text: merged_text,
        tokens: all_tokens,
    })
}

#[derive(Debug)]
struct CliArgs {
    model_dir: PathBuf,
    audio_path: PathBuf,
    provider: ExecutionProvider,
    timestamp_mode: TimestampMode,
    ort_dir: Option<PathBuf>,
}

#[derive(Debug, Clone)]
struct AudioSegment {
    index: usize,
    start_sec: f64,
    end_sec: f64,
}

impl AudioSegment {
    fn duration_sec(&self) -> f64 {
        self.end_sec - self.start_sec
    }
}

fn parse_args(args: Vec<String>) -> Result<CliArgs, Box<dyn std::error::Error>> {
    let mut model_dir = PathBuf::from(r"D:\voxtrans\model\parakeet-tdt-0.6b-v2");
    let mut audio_path: Option<PathBuf> = None;
    let mut provider = ExecutionProvider::Cuda;
    let mut timestamp_mode = TimestampMode::Words;
    let mut ort_dir: Option<PathBuf> = None;

    let mut i = 0usize;
    while i < args.len() {
        match args[i].as_str() {
            "--model-dir" => {
                i += 1;
                model_dir = PathBuf::from(args.get(i).ok_or("--model-dir requires a value")?.as_str());
            }
            "--audio" => {
                i += 1;
                audio_path = Some(PathBuf::from(args.get(i).ok_or("--audio requires a value")?.as_str()));
            }
            "--provider" => {
                i += 1;
                let value = args.get(i).ok_or("--provider requires a value")?;
                provider = match value.as_str() {
                    "cuda" => ExecutionProvider::Cuda,
                    "cpu" => ExecutionProvider::Cpu,
                    _ => return Err("--provider must be 'cuda' or 'cpu'".into()),
                };
            }
            "--timestamp" => {
                i += 1;
                let value = args.get(i).ok_or("--timestamp requires a value")?;
                timestamp_mode = match value.as_str() {
                    "words" => TimestampMode::Words,
                    "sentences" => TimestampMode::Sentences,
                    "tokens" => TimestampMode::Tokens,
                    _ => return Err("--timestamp must be 'words', 'sentences', or 'tokens'".into()),
                };
            }
            "--ort-dir" => {
                i += 1;
                ort_dir = Some(PathBuf::from(args.get(i).ok_or("--ort-dir requires a value")?.as_str()));
            }
            "--help" | "-h" => {
                print_usage();
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument: {other}").into()),
        }
        i += 1;
    }

    let audio_path = audio_path.ok_or("missing required --audio <path>")?;
    Ok(CliArgs {
        model_dir,
        audio_path,
        provider,
        timestamp_mode,
        ort_dir,
    })
}

fn print_usage() {
    println!("Usage:");
    println!("  parakeet-rs-upstream-test --audio <path> [options]");
    println!();
    println!("Options:");
    println!("  --model-dir <path>      TDT model directory (default: D:\\voxtrans\\model\\parakeet-tdt-0.6b-v2)");
    println!("  --provider <cuda|cpu>   Execution provider (default: cuda)");
    println!("  --timestamp <words|sentences|tokens>  Timestamp mode (default: words)");
    println!("  --ort-dir <path>        Directory containing custom onnxruntime.dll");
    println!("  -h, --help              Show this help");
}

fn wav_duration_seconds(audio_path: &Path) -> Result<f64, Box<dyn std::error::Error>> {
    let reader = hound::WavReader::open(audio_path)?;
    let spec = reader.spec();
    let total_samples = reader.duration() as f64;
    let channel_count = spec.channels as f64;
    let sample_rate = spec.sample_rate as f64;
    if channel_count <= 0.0 || sample_rate <= 0.0 {
        return Err("invalid wav metadata for duration calculation".into());
    }
    Ok(total_samples / channel_count / sample_rate)
}

enum PreparedAudio {
    Original(PathBuf),
    Temporary(TemporaryAudioFile),
}

impl PreparedAudio {
    fn path(&self) -> &Path {
        match self {
            PreparedAudio::Original(path) => path.as_path(),
            PreparedAudio::Temporary(temp) => temp.path.as_path(),
        }
    }
}

struct TemporaryAudioFile {
    path: PathBuf,
}

impl Drop for TemporaryAudioFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn prepare_audio_for_transcription(input_path: &PathBuf) -> Result<PreparedAudio, Box<dyn std::error::Error>> {
    if is_wav_16k_mono(input_path)? {
        return Ok(PreparedAudio::Original(input_path.clone()));
    }

    let temp_path = build_temp_wav_path("prepared");
    let status = Command::new("ffmpeg")
        .arg("-loglevel")
        .arg("error")
        .arg("-y")
        .arg("-i")
        .arg(input_path)
        .arg("-vn")
        .arg("-ac")
        .arg("1")
        .arg("-ar")
        .arg("16000")
        .arg("-c:a")
        .arg("pcm_s16le")
        .arg(&temp_path)
        .status()?;

    if !status.success() {
        return Err(format!("ffmpeg conversion failed for input: {}", input_path.display()).into());
    }

    Ok(PreparedAudio::Temporary(TemporaryAudioFile { path: temp_path }))
}

fn build_segments_from_silence(
    audio_path: &Path,
    total_duration_sec: f64,
) -> Result<Vec<AudioSegment>, Box<dyn std::error::Error>> {
    if total_duration_sec <= CHUNK_TARGET_SECONDS {
        return Ok(vec![AudioSegment {
            index: 0,
            start_sec: 0.0,
            end_sec: total_duration_sec,
        }]);
    }

    let silence_midpoints = detect_silence_midpoints(audio_path)?;
    let mut split_points = Vec::new();
    let mut last = 0.0_f64;

    while last + CHUNK_TARGET_SECONDS < total_duration_sec {
        let boundary = last + CHUNK_TARGET_SECONDS;
        let candidate = silence_midpoints
            .iter()
            .copied()
            .filter(|mid| *mid > last + 0.2 && *mid < boundary)
            .fold(None, |acc: Option<f64>, cur| match acc {
                Some(prev) if prev > cur => Some(prev),
                _ => Some(cur),
            });
        let mut split = candidate.unwrap_or(boundary);
        if split <= last + 0.2 {
            split = boundary;
        }
        split_points.push(split);
        last = split;
    }

    let mut segments = Vec::new();
    let mut start = 0.0_f64;
    for (idx, end) in split_points.iter().enumerate() {
        segments.push(AudioSegment {
            index: idx,
            start_sec: start,
            end_sec: *end,
        });
        start = *end;
    }
    segments.push(AudioSegment {
        index: segments.len(),
        start_sec: start,
        end_sec: total_duration_sec,
    });
    Ok(segments)
}

fn detect_silence_midpoints(audio_path: &Path) -> Result<Vec<f64>, Box<dyn std::error::Error>> {
    let output = Command::new("ffmpeg")
        .arg("-hide_banner")
        .arg("-i")
        .arg(audio_path)
        .arg("-af")
        .arg(format!(
            "silencedetect=noise={}:d={}",
            SILENCE_NOISE_DB, SILENCE_MIN_SECONDS
        ))
        .arg("-f")
        .arg("null")
        .arg("-")
        .output()?;

    let stderr_text = String::from_utf8_lossy(&output.stderr);
    let mut current_start: Option<f64> = None;
    let mut midpoints = Vec::new();

    for line in stderr_text.lines() {
        if let Some(start) = parse_value_after(line, "silence_start:") {
            current_start = Some(start);
            continue;
        }
        if let Some(end) = parse_value_after(line, "silence_end:") {
            if let Some(start) = current_start.take() {
                midpoints.push((start + end) / 2.0);
            }
        }
    }

    midpoints.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    Ok(midpoints)
}

fn parse_value_after(line: &str, marker: &str) -> Option<f64> {
    let idx = line.find(marker)?;
    let value_part = &line[idx + marker.len()..];
    let token = value_part.trim().split_whitespace().next()?;
    token.parse::<f64>().ok()
}

fn extract_segment_to_temp(
    full_audio_path: &Path,
    segment: &AudioSegment,
) -> Result<TemporaryAudioFile, Box<dyn std::error::Error>> {
    let temp_path = build_temp_wav_path(&format!("segment_{}", segment.index + 1));
    let status = Command::new("ffmpeg")
        .arg("-loglevel")
        .arg("error")
        .arg("-y")
        .arg("-ss")
        .arg(format!("{:.6}", segment.start_sec))
        .arg("-to")
        .arg(format!("{:.6}", segment.end_sec))
        .arg("-i")
        .arg(full_audio_path)
        .arg("-vn")
        .arg("-ac")
        .arg("1")
        .arg("-ar")
        .arg("16000")
        .arg("-c:a")
        .arg("pcm_s16le")
        .arg(&temp_path)
        .status()?;

    if !status.success() {
        return Err(format!(
            "ffmpeg split failed for segment {} [{:.3}, {:.3}]",
            segment.index + 1,
            segment.start_sec,
            segment.end_sec
        )
        .into());
    }
    Ok(TemporaryAudioFile { path: temp_path })
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
    std::env::temp_dir().join(format!("parakeet_rs_tmp_{}_{}_{}.wav", pid, tag, nanos))
}

fn merge_punctuation_tokens(mut result: TranscriptionResult) -> TranscriptionResult {
    let mut merged: Vec<TimedToken> = Vec::with_capacity(result.tokens.len());
    for token in result.tokens {
        if is_standalone_punctuation(&token.text) {
            if let Some(prev) = merged.last_mut() {
                prev.text.push_str(&token.text);
                prev.end = token.end;
            } else {
                merged.push(token);
            }
        } else {
            merged.push(token);
        }
    }
    result.tokens = merged;
    result
}

fn is_standalone_punctuation(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }
    trimmed
        .chars()
        .all(|c| matches!(c, ',' | '.' | '!' | '?' | ';' | ':' | '，' | '。' | '！' | '？' | '；' | '：'))
}
