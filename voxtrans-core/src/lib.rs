use std::path::{Path, PathBuf};
use std::time::Instant;
use std::{fs, process::Command};

use parakeet_rs::{
    ExecutionConfig, ExecutionProvider, ParakeetTDT, TimedToken, TimestampMode, Transcriber,
    TranscriptionResult,
};
use serde::Deserialize;

pub mod subtitle;

pub use subtitle::srt::to_srt_from_sentence_tokens as to_srt;

const DEFAULT_CHUNK_TARGET_SECONDS: f64 = 300.0;
const TARGET_SAMPLE_RATE: u32 = 16_000;

#[derive(Debug, Clone)]
pub struct TranscribeOptions {
    pub model_dir: PathBuf,
    pub audio_path: PathBuf,
    pub provider: Provider,
    pub timestamp_mode: TimestampKind,
    pub intra_threads: usize,
    pub inter_threads: usize,
    pub chunk_target_seconds: f64,
}

impl Default for TranscribeOptions {
    fn default() -> Self {
        let intra_threads = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);

        Self {
            model_dir: default_model_dir(),
            audio_path: PathBuf::new(),
            provider: Provider::Cuda,
            timestamp_mode: TimestampKind::Sentences,
            intra_threads,
            inter_threads: 1,
            chunk_target_seconds: DEFAULT_CHUNK_TARGET_SECONDS,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Provider {
    Cpu,
    Cuda,
}

#[derive(Debug, Clone, Copy)]
pub enum TimestampKind {
    Words,
    Sentences,
    Tokens,
}

#[derive(Debug, Clone)]
pub struct SegmentSummary {
    pub index: usize,
    pub duration_sec: f64,
}

#[derive(Debug, Clone)]
pub struct TranscribeOutput {
    pub text: String,
    pub tokens: Vec<TimedToken>,
    pub audio_duration_sec: f64,
    pub vad_elapsed_sec: f64,
    pub transcribe_elapsed_sec: f64,
    pub execution_provider: &'static str,
    pub segment_summaries: Vec<SegmentSummary>,
}

pub fn transcribe_with_parakeet_v2(
    options: &TranscribeOptions,
) -> Result<TranscribeOutput, Box<dyn std::error::Error>> {
    transcribe_with_parakeet_v2_with_progress(options, |_current, _total| {})
}

pub fn transcribe_with_parakeet_v2_with_progress<F>(
    options: &TranscribeOptions,
    mut on_segment_progress: F,
) -> Result<TranscribeOutput, Box<dyn std::error::Error>>
where
    F: FnMut(usize, usize),
{
    if options.audio_path.as_os_str().is_empty() {
        return Err("audio_path is required".into());
    }

    let execution_provider = to_execution_provider(options.provider);
    let timestamp_mode = to_timestamp_mode(options.timestamp_mode);
    let prepared_audio = prepare_audio_for_transcription(&options.audio_path)?;
    let audio_duration_sec = wav_duration_seconds(prepared_audio.path())?;
    let mono_samples = load_wav_mono_f32(prepared_audio.path())?;
    let (segments, vad_elapsed_sec) = build_segments_from_vad(
        prepared_audio.path(),
        audio_duration_sec,
        options.chunk_target_seconds,
    )?;

    let started_at = Instant::now();
    let result = transcribe_in_segments(
        &options.model_dir,
        &mono_samples,
        execution_provider,
        timestamp_mode,
        options.intra_threads,
        options.inter_threads,
        &segments,
        &mut on_segment_progress,
    )?;
    let result = match options.timestamp_mode {
        TimestampKind::Words => result,
        _ => merge_punctuation_tokens(result),
    };

    let elapsed_sec = started_at.elapsed().as_secs_f64();
    let execution_provider = match options.provider {
        Provider::Cuda => "cuda",
        _ => "cpu",
    };

    let segment_summaries = segments
        .iter()
        .map(|s| SegmentSummary {
            index: s.index + 1,
            duration_sec: s.duration_sec(),
        })
        .collect();

    Ok(TranscribeOutput {
        text: result.text,
        tokens: result.tokens,
        audio_duration_sec,
        vad_elapsed_sec,
        transcribe_elapsed_sec: elapsed_sec,
        execution_provider,
        segment_summaries,
    })
}

fn to_execution_provider(provider: Provider) -> ExecutionProvider {
    match provider {
        Provider::Cpu => ExecutionProvider::Cpu,
        Provider::Cuda => ExecutionProvider::Cuda,
    }
}

fn to_timestamp_mode(mode: TimestampKind) -> TimestampMode {
    match mode {
        TimestampKind::Words => TimestampMode::Words,
        TimestampKind::Sentences => TimestampMode::Sentences,
        TimestampKind::Tokens => TimestampMode::Tokens,
    }
}

fn default_model_dir() -> PathBuf {
    if let Ok(custom) = std::env::var("VOXTRANS_MODEL_DIR") {
        let path = PathBuf::from(custom);
        if path.exists() {
            return path;
        }
    }

    // Installed app: prefer model folder beside the executable.
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            let candidate = exe_dir.join("model").join("parakeet-tdt-0.6b-v2");
            if candidate.exists() {
                return candidate;
            }
        }
    }

    // Dev mode fallback (e.g. `npm run tauri dev` from project root).
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("model")
        .join("parakeet-tdt-0.6b-v2")
}

fn transcribe_in_segments(
    model_dir: &Path,
    full_audio_samples: &[f32],
    provider: ExecutionProvider,
    timestamp_mode: TimestampMode,
    intra_threads: usize,
    inter_threads: usize,
    segments: &[AudioSegment],
    on_segment_progress: &mut dyn FnMut(usize, usize),
) -> Result<TranscriptionResult, Box<dyn std::error::Error>> {
    let mut model = ParakeetTDT::from_pretrained(
        model_dir,
        Some(
            ExecutionConfig::new()
                .with_execution_provider(provider)
                .with_intra_threads(intra_threads)
                .with_inter_threads(inter_threads),
        ),
    )?;

    let mut all_tokens: Vec<TimedToken> = Vec::new();
    let mut text_parts: Vec<String> = Vec::new();

    let total_segments = segments.len();
    let sample_len = full_audio_samples.len();
    for segment in segments {
        on_segment_progress(segment.index + 1, total_segments);
        let start_index =
            ((segment.start_sec * TARGET_SAMPLE_RATE as f64).floor() as usize).min(sample_len);
        let end_index =
            ((segment.end_sec * TARGET_SAMPLE_RATE as f64).ceil() as usize).min(sample_len);
        if end_index <= start_index {
            continue;
        }
        let mut segment_result = model.transcribe_samples(
            full_audio_samples[start_index..end_index].to_vec(),
            TARGET_SAMPLE_RATE,
            1,
            Some(timestamp_mode),
        )?;

        if !segment_result.text.trim().is_empty() {
            text_parts.push(segment_result.text.trim().to_string());
        }

        for token in &mut segment_result.tokens {
            token.start += segment.start_sec as f32;
            token.end += segment.start_sec as f32;
        }

        all_tokens.extend(segment_result.tokens);
    }

    Ok(TranscriptionResult {
        text: text_parts.join(" "),
        tokens: all_tokens,
    })
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

fn ffmpeg_command() -> Command {
    let mut cmd = Command::new(resolve_ffmpeg_program());
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW
        cmd.creation_flags(0x08000000);
    }
    cmd
}

fn resolve_ffmpeg_program() -> PathBuf {
    if let Ok(custom) = std::env::var("VOXTRANS_FFMPEG_PATH") {
        let custom_path = PathBuf::from(custom);
        if custom_path.exists() {
            return custom_path;
        }
    }

    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            #[cfg(target_os = "windows")]
            let bundled = exe_dir.join("bin").join("ffmpeg.exe");
            #[cfg(not(target_os = "windows"))]
            let bundled = exe_dir.join("bin").join("ffmpeg");
            if bundled.exists() {
                return bundled;
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        PathBuf::from("ffmpeg.exe")
    }
    #[cfg(not(target_os = "windows"))]
    {
        PathBuf::from("ffmpeg")
    }
}

fn fireredvad_command() -> Command {
    let mut cmd = Command::new(resolve_fireredvad_program());
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW
        cmd.creation_flags(0x08000000);
    }
    cmd
}

fn resolve_fireredvad_program() -> PathBuf {
    if let Ok(custom) = std::env::var("VOXTRANS_VAD_PATH") {
        let custom_path = PathBuf::from(custom);
        if custom_path.exists() {
            return custom_path;
        }
    }

    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            #[cfg(target_os = "windows")]
            let bundled = exe_dir.join("bin").join("fireredvad.exe");
            #[cfg(not(target_os = "windows"))]
            let bundled = exe_dir.join("bin").join("fireredvad");
            if bundled.exists() {
                return bundled;
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        PathBuf::from("fireredvad.exe")
    }
    #[cfg(not(target_os = "windows"))]
    {
        PathBuf::from("fireredvad")
    }
}

fn prepare_audio_for_transcription(
    input_path: &PathBuf,
) -> Result<PreparedAudio, Box<dyn std::error::Error>> {
    if is_wav_16k_mono(input_path)? {
        return Ok(PreparedAudio::Original(input_path.clone()));
    }

    let temp_path = build_temp_wav_path("prepared");
    let status = ffmpeg_command()
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
        return Err(format!(
            "ffmpeg conversion failed for input: {}",
            input_path.display()
        )
        .into());
    }

    Ok(PreparedAudio::Temporary(TemporaryAudioFile {
        path: temp_path,
    }))
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

#[derive(Debug, Deserialize)]
struct VadOutput {
    dur: f64,
    timestamps: Vec<[f64; 2]>,
}

fn build_segments_from_vad(
    audio_path: &Path,
    total_duration_sec: f64,
    chunk_target_seconds: f64,
) -> Result<(Vec<AudioSegment>, f64), Box<dyn std::error::Error>> {
    let vad_started_at = Instant::now();
    let chunk_target_seconds = chunk_target_seconds.max(30.0);
    let vad = detect_speech_with_fireredvad(audio_path)?;
    let vad_elapsed_sec = vad_started_at.elapsed().as_secs_f64();
    let effective_total_duration = if total_duration_sec > 0.0 {
        total_duration_sec
    } else {
        vad.dur
    };
    if effective_total_duration <= chunk_target_seconds {
        return Ok((
            vec![AudioSegment {
                index: 0,
                start_sec: 0.0,
                end_sec: effective_total_duration,
            }],
            vad_elapsed_sec,
        ));
    }

    let speech_ranges = normalize_ranges(&vad.timestamps, effective_total_duration);
    let silence_midpoints = silence_midpoints_from_vad(&speech_ranges, effective_total_duration);

    let mut split_points = Vec::new();
    let mut last = 0.0_f64;
    while last + chunk_target_seconds < effective_total_duration {
        let boundary = last + chunk_target_seconds;
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
        end_sec: effective_total_duration,
    });
    Ok((segments, vad_elapsed_sec))
}

fn detect_speech_with_fireredvad(
    audio_path: &Path,
) -> Result<VadOutput, Box<dyn std::error::Error>> {
    let output = fireredvad_command().arg("--wav").arg(audio_path).output()?;
    if !output.status.success() {
        return Err(format!(
            "fireredvad failed for {}: {}",
            audio_path.display(),
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }

    let stdout = String::from_utf8(output.stdout)?;
    let parsed: VadOutput = serde_json::from_str(stdout.trim())?;
    Ok(parsed)
}

fn normalize_ranges(ranges: &[[f64; 2]], total_duration_sec: f64) -> Vec<(f64, f64)> {
    if total_duration_sec <= 0.0 {
        return Vec::new();
    }

    let mut normalized: Vec<(f64, f64)> = ranges
        .iter()
        .map(|pair| (pair[0].max(0.0), pair[1].min(total_duration_sec)))
        .filter(|(start, end)| *end > *start)
        .collect();
    normalized.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    let mut merged: Vec<(f64, f64)> = Vec::with_capacity(normalized.len());
    for (start, end) in normalized {
        if let Some(last) = merged.last_mut() {
            if start <= last.1 {
                last.1 = last.1.max(end);
                continue;
            }
        }
        merged.push((start, end));
    }
    merged
}

fn silence_midpoints_from_vad(speech_ranges: &[(f64, f64)], total_duration_sec: f64) -> Vec<f64> {
    if total_duration_sec <= 0.0 {
        return Vec::new();
    }

    if speech_ranges.is_empty() {
        return Vec::new();
    }

    let mut midpoints = Vec::new();
    let mut cursor = 0.0_f64;

    for &(speech_start, speech_end) in speech_ranges {
        if speech_start > cursor {
            midpoints.push((cursor + speech_start) / 2.0);
        }
        cursor = cursor.max(speech_end);
    }

    if cursor < total_duration_sec {
        midpoints.push((cursor + total_duration_sec) / 2.0);
    }

    midpoints
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
    trimmed.chars().all(|c| {
        matches!(
            c,
            ',' | '.' | '!' | '?' | ';' | ':' | '，' | '。' | '！' | '？' | '；' | '：'
        )
    })
}
