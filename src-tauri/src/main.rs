#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app_state;
mod commands;
mod db;
mod llm;
mod prompts;
mod services;

use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::path::PathBuf;
use tauri::Emitter;
use tauri::Manager;
use tauri::async_runtime::spawn_blocking;
use voxtrans_core::subtitle::segmenter::{
    WordToken, normalize_word_tokens, plain_text_from_segments, split_english_segments,
    words_from_timed_tokens,
};
use voxtrans_core::subtitle::srt::{
    normalize_cues, parse_srt, to_srt_from_cues, to_srt_from_segments, validate_cues,
};
use voxtrans_core::{Provider, TimestampKind, TranscribeOptions};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TranscribeRequest {
    task_id: String,
    audio_path: String,
    provider: String,
    chunk_target_seconds: u32,
    model_dir: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TranscribeResponse {
    words: Vec<WordTokenResponse>,
    segment_total: usize,
    segment_durations_sec: Vec<f64>,
    audio_duration_sec: f64,
    transcribe_elapsed_sec: f64,
    execution_provider: String,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct TranscribeProgressEvent {
    task_id: String,
    current_segment: usize,
    total_segments: usize,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct WordTokenResponse {
    start: f64,
    end: f64,
    word: String,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct SegmentWithWordsResponse {
    start: f64,
    end: f64,
    text: String,
    words: Vec<WordTokenResponse>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BuildSegmentsRequest {
    task_id: String,
    audio_path: String,
    words: Vec<WordTokenResponse>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BuildSegmentsResponse {
    text: String,
    srt: String,
    srt_output_path: String,
    segments: Vec<SegmentWithWordsResponse>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SaveSrtRequest {
    output_path: String,
    content: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SubtitleLoadRequest {
    task_id: String,
    media_path: String,
    fallback_srt: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SubtitleLoadResponse {
    srt_path: String,
    draft_path: String,
    content: String,
    using_draft: bool,
    warnings: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SubtitleSaveRequest {
    task_id: String,
    media_path: String,
    content: String,
    autosave: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SubtitleSaveResponse {
    srt_path: String,
    warnings: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ModelStatusResponse {
    model_dir: String,
    required_files: Vec<String>,
    missing_files: Vec<String>,
    ready: bool,
    download: app_state::ModelDownloadStateSnapshot,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct ModelDownloadProgressEvent {
    phase: String,
    downloaded_bytes: u64,
    total_bytes: u64,
    speed_bytes_per_sec: u64,
    message: String,
}

const MODEL_DIR_NAME: &str = "parakeet-tdt-0.6b-v2";
const REQUIRED_MODEL_FILES: [&str; 4] = [
    "encoder-model.onnx",
    "encoder-model.onnx.data",
    "decoder_joint-model.onnx",
    "vocab.txt",
];
const MODEL_DOWNLOAD_FILES: [(&str, &str, u64); 5] = [
    (
        "decoder_joint-model.onnx",
        "https://modelscope.cn/models/eclipse005/parakeet-tdt-0.6b-v2-onnx/resolve/master/decoder_joint-model.onnx",
        35_790_000,
    ),
    (
        "encoder-model.onnx",
        "https://modelscope.cn/models/eclipse005/parakeet-tdt-0.6b-v2-onnx/resolve/master/encoder-model.onnx",
        41_770_000,
    ),
    (
        "encoder-model.onnx.data",
        "https://modelscope.cn/models/eclipse005/parakeet-tdt-0.6b-v2-onnx/resolve/master/encoder-model.onnx.data",
        2_440_000_000,
    ),
    (
        "vocab.txt",
        "https://modelscope.cn/models/eclipse005/parakeet-tdt-0.6b-v2-onnx/resolve/master/vocab.txt",
        9_380,
    ),
    (
        "config.json",
        "https://modelscope.cn/models/eclipse005/parakeet-tdt-0.6b-v2-onnx/resolve/master/config.json",
        97,
    ),
];

fn compute_model_download_bytes(model_dir: &std::path::Path) -> (u64, u64) {
    let mut downloaded_bytes: u64 = 0;
    let mut total_bytes: u64 = 0;
    for (file_name, _url, expected_size) in MODEL_DOWNLOAD_FILES {
        let target = model_dir.join(file_name);
        let part = model_dir.join(format!("{}.part", file_name));
        let current = if target.exists() {
            std::fs::metadata(&target).map(|m| m.len()).unwrap_or(0)
        } else {
            std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0)
        };
        downloaded_bytes = downloaded_bytes.saturating_add(current.min(expected_size));
        total_bytes = total_bytes.saturating_add(expected_size);
    }
    (downloaded_bytes, total_bytes)
}

#[tauri::command]
fn get_model_status(state: tauri::State<app_state::AppState>) -> Result<ModelStatusResponse, String> {
    let model_dir = resolve_install_model_dir();
    let required_files: Vec<String> = REQUIRED_MODEL_FILES.iter().map(|s| s.to_string()).collect();
    let missing_files: Vec<String> = REQUIRED_MODEL_FILES
        .iter()
        .filter(|name| !model_dir.join(name).exists())
        .map(|s| s.to_string())
        .collect();
    let snapshot_in_memory = state
        .model_download
        .lock()
        .map_err(|_| "model download state lock poisoned".to_string())?
        .snapshot
        .clone();
    let (downloaded_bytes, total_bytes) = compute_model_download_bytes(&model_dir);
    let phase = if snapshot_in_memory.phase == "downloading" {
        "downloading".to_string()
    } else if missing_files.is_empty() {
        "completed".to_string()
    } else if downloaded_bytes > 0 {
        if snapshot_in_memory.phase == "cancelled" {
            "cancelled".to_string()
        } else if snapshot_in_memory.phase == "failed" {
            "failed".to_string()
        } else {
            "idle".to_string()
        }
    } else {
        "idle".to_string()
    };
    let snapshot = app_state::ModelDownloadStateSnapshot {
        phase,
        downloaded_bytes,
        total_bytes,
        speed_bytes_per_sec: if snapshot_in_memory.phase == "downloading" {
            snapshot_in_memory.speed_bytes_per_sec
        } else {
            0
        },
        message: snapshot_in_memory.message,
    };

    Ok(ModelStatusResponse {
        model_dir: model_dir.display().to_string(),
        required_files,
        missing_files: missing_files.clone(),
        ready: missing_files.is_empty(),
        download: snapshot,
    })
}

#[tauri::command]
fn start_model_download(
    app: tauri::AppHandle,
    state: tauri::State<app_state::AppState>,
) -> Result<(), String> {
    let model_download = state.model_download.clone();
    {
        let model_dir = resolve_install_model_dir();
        let (downloaded_bytes, total_bytes) = compute_model_download_bytes(&model_dir);
        let mut guard = model_download
            .lock()
            .map_err(|_| "model download state lock poisoned".to_string())?;
        if guard.snapshot.phase == "downloading" {
            return Ok(());
        }
        let cancel_flag = Arc::new(AtomicBool::new(false));
        guard.cancel_flag = Some(cancel_flag.clone());
        guard.snapshot = app_state::ModelDownloadStateSnapshot {
            phase: "downloading".to_string(),
            downloaded_bytes,
            total_bytes,
            speed_bytes_per_sec: 0,
            message: "开始下载模型".to_string(),
        };
    }

    let app_handle = app.clone();
    let model_download_for_task = model_download.clone();
    tauri::async_runtime::spawn(async move {
        match spawn_blocking(move || download_model_files(&app_handle, &model_download_for_task)).await {
            Ok(Ok(())) => {}
            Ok(Err(err)) => {
                let _ = set_model_download_snapshot(
                    &app,
                    &model_download,
                    "failed",
                    0,
                    0,
                    0,
                    &err,
                    true,
                );
            }
            Err(err) => {
                let message = format!("下载任务异常: {}", err);
                let _ = set_model_download_snapshot(
                    &app,
                    &model_download,
                    "failed",
                    0,
                    0,
                    0,
                    &message,
                    true,
                );
            }
        }
    });

    Ok(())
}

#[tauri::command]
fn cancel_model_download(state: tauri::State<app_state::AppState>) -> Result<(), String> {
    let guard = state
        .model_download
        .lock()
        .map_err(|_| "model download state lock poisoned".to_string())?;
    if let Some(flag) = guard.cancel_flag.as_ref() {
        flag.store(true, Ordering::Relaxed);
    }
    Ok(())
}

#[tauri::command]
async fn transcribe(
    app: tauri::AppHandle,
    request: TranscribeRequest,
) -> Result<TranscribeResponse, String> {
    spawn_blocking(move || {
        let task_id = request.task_id.clone();
        let app_handle = app.clone();
        let mut options = TranscribeOptions::default();
        let audio_path = PathBuf::from(&request.audio_path);
        options.audio_path = audio_path.clone();
        options.provider = match request.provider.to_ascii_lowercase().as_str() {
            "cpu" => Provider::Cpu,
            "cuda" => Provider::Cuda,
            other => return Err(format!("unsupported provider: {other}")),
        };
        options.timestamp_mode = TimestampKind::Words;
        options.chunk_target_seconds = request.chunk_target_seconds.clamp(60, 1800) as f64;
        options.model_dir = resolve_install_model_dir();

        if let Some(model_dir) = request.model_dir {
            options.model_dir = PathBuf::from(model_dir);
        }

        let output =
            voxtrans_core::transcribe_with_parakeet_v2_with_progress(&options, |current, total| {
                let _ = app_handle.emit(
                    "transcribe-progress",
                    TranscribeProgressEvent {
                        task_id: task_id.clone(),
                        current_segment: current,
                        total_segments: total,
                    },
                );
            })
            .map_err(|err| err.to_string())?;
        let words = normalize_word_tokens(words_from_timed_tokens(&output.tokens));

        Ok(TranscribeResponse {
            words: words.iter().map(word_to_response).collect(),
            segment_total: output.segment_summaries.len(),
            segment_durations_sec: output
                .segment_summaries
                .iter()
                .map(|s| (s.duration_sec * 100.0).round() / 100.0)
                .collect(),
            audio_duration_sec: output.audio_duration_sec,
            transcribe_elapsed_sec: output.transcribe_elapsed_sec,
            execution_provider: output.execution_provider.to_string(),
        })
    })
    .await
    .map_err(|err| err.to_string())?
}

fn resolve_install_model_dir() -> PathBuf {
    if let Ok(custom_dir) = std::env::var("VOXTRANS_MODEL_DIR") {
        let path = PathBuf::from(custom_dir);
        if !path.as_os_str().is_empty() {
            return path;
        }
    }

    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));
    exe_dir.join("model").join(MODEL_DIR_NAME)
}

fn emit_model_download_progress(app: &tauri::AppHandle, snapshot: &app_state::ModelDownloadStateSnapshot) {
    let _ = app.emit(
        "model-download-progress",
        ModelDownloadProgressEvent {
            phase: snapshot.phase.clone(),
            downloaded_bytes: snapshot.downloaded_bytes,
            total_bytes: snapshot.total_bytes,
            speed_bytes_per_sec: snapshot.speed_bytes_per_sec,
            message: snapshot.message.clone(),
        },
    );
}

fn set_model_download_snapshot(
    app: &tauri::AppHandle,
    runtime: &Arc<std::sync::Mutex<app_state::ModelDownloadRuntime>>,
    phase: &str,
    downloaded_bytes: u64,
    total_bytes: u64,
    speed_bytes_per_sec: u64,
    message: &str,
    clear_cancel_flag: bool,
) -> Result<(), String> {
    let snapshot = app_state::ModelDownloadStateSnapshot {
        phase: phase.to_string(),
        downloaded_bytes,
        total_bytes,
        speed_bytes_per_sec,
        message: message.to_string(),
    };
    {
        let mut guard = runtime
            .lock()
            .map_err(|_| "model download state lock poisoned".to_string())?;
        guard.snapshot = snapshot.clone();
        if clear_cancel_flag {
            guard.cancel_flag = None;
        }
    }
    emit_model_download_progress(app, &snapshot);
    Ok(())
}

fn download_model_files(
    app: &tauri::AppHandle,
    runtime: &Arc<std::sync::Mutex<app_state::ModelDownloadRuntime>>,
) -> Result<(), String> {
    let cancel_flag = {
        let guard = runtime
            .lock()
            .map_err(|_| "model download state lock poisoned".to_string())?;
        guard
            .cancel_flag
            .clone()
            .ok_or_else(|| "download task missing cancel flag".to_string())?
    };
    let model_dir = resolve_install_model_dir();
    std::fs::create_dir_all(&model_dir).map_err(|err| err.to_string())?;

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) VoxTrans/0.1")
        .build()
        .map_err(|err| err.to_string())?;

    let mut downloaded_bytes: u64 = 0;
    let mut total_bytes: u64 = 0;
    let mut estimates: Vec<(&str, &str, PathBuf, u64)> = Vec::new();
    for (file_name, url, expected_size) in MODEL_DOWNLOAD_FILES {
        let target = model_dir.join(file_name);
        let part = model_dir.join(format!("{}.part", file_name));
        let existing_len = if target.exists() {
            std::fs::metadata(&target).map(|m| m.len()).unwrap_or(0)
        } else {
            std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0)
        };
        let estimated_total = if target.exists() {
            existing_len.max(expected_size)
        } else {
            expected_size.max(existing_len)
        };
        downloaded_bytes = downloaded_bytes.saturating_add(existing_len.min(estimated_total));
        total_bytes = total_bytes.saturating_add(estimated_total);
        estimates.push((file_name, url, target, estimated_total));
    }
    estimates.sort_by_key(|(_, _, _, expected_size)| *expected_size);
    let mut last_speed_mark = std::time::Instant::now();
    let mut last_speed_bytes = downloaded_bytes;

    set_model_download_snapshot(
        app,
        runtime,
        "downloading",
        downloaded_bytes,
        total_bytes,
        0,
        "模型下载中",
        false,
    )?;

    for (file_name, url, target, _estimated_total) in estimates {
        if cancel_flag.load(Ordering::Relaxed) {
            set_model_download_snapshot(
                app,
                runtime,
                "cancelled",
                downloaded_bytes,
                total_bytes,
                0,
                "下载已取消",
                true,
            )?;
            return Ok(());
        }
        if target.exists() {
            continue;
        }
        let part_path = target.with_extension(format!(
            "{}.part",
            target.extension().and_then(|s| s.to_str()).unwrap_or("")
        ));
        let part_bytes = std::fs::metadata(&part_path).map(|m| m.len()).unwrap_or(0);
        let mut request = client
            .get(url)
            .header(reqwest::header::ACCEPT, "*/*")
            .header(reqwest::header::REFERER, "https://modelscope.cn/");
        if part_bytes > 0 {
            request = request.header(reqwest::header::RANGE, format!("bytes={}-", part_bytes));
        }
        let mut response = request.send().map_err(|err| err.to_string())?;
        if response.status() == reqwest::StatusCode::OK && part_bytes > 0 {
            let _ = std::fs::remove_file(&part_path);
            downloaded_bytes = downloaded_bytes.saturating_sub(part_bytes);
            response = client
                .get(url)
                .header(reqwest::header::ACCEPT, "*/*")
                .header(reqwest::header::REFERER, "https://modelscope.cn/")
                .send()
                .map_err(|err| err.to_string())?;
        }
        if !(response.status().is_success() || response.status() == reqwest::StatusCode::PARTIAL_CONTENT)
        {
            return Err(format!(
                "download failed: {} -> {}",
                file_name,
                response.status()
            ));
        }

        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&part_path)
            .map_err(|err| err.to_string())?;

        let mut buf = [0_u8; 64 * 1024];
        loop {
            if cancel_flag.load(Ordering::Relaxed) {
                set_model_download_snapshot(
                    app,
                    runtime,
                    "cancelled",
                    downloaded_bytes,
                    total_bytes,
                    0,
                    "下载已取消",
                    true,
                )?;
                return Ok(());
            }
            let read = response.read(&mut buf).map_err(|err| err.to_string())?;
            if read == 0 {
                break;
            }
            file.write_all(&buf[..read]).map_err(|err| err.to_string())?;
            downloaded_bytes = downloaded_bytes.saturating_add(read as u64);

            let elapsed = last_speed_mark.elapsed().as_secs_f64();
            if elapsed >= 0.5 {
                let speed = ((downloaded_bytes.saturating_sub(last_speed_bytes)) as f64 / elapsed)
                    .round() as u64;
                last_speed_bytes = downloaded_bytes;
                last_speed_mark = std::time::Instant::now();
                set_model_download_snapshot(
                    app,
                    runtime,
                    "downloading",
                    downloaded_bytes,
                    total_bytes,
                    speed,
                    "模型下载中",
                    false,
                )?;
            }
        }

        std::fs::rename(&part_path, &target).map_err(|err| err.to_string())?;
        set_model_download_snapshot(
            app,
            runtime,
            "downloading",
            downloaded_bytes,
            total_bytes,
            0,
            "模型下载中",
            false,
        )?;
    }

    let missing: Vec<&str> = REQUIRED_MODEL_FILES
        .iter()
        .copied()
        .filter(|name| !model_dir.join(name).exists())
        .collect();
    if missing.is_empty() {
        set_model_download_snapshot(
            app,
            runtime,
            "completed",
            downloaded_bytes,
            total_bytes.max(downloaded_bytes),
            0,
            "模型下载完成",
            true,
        )?;
        Ok(())
    } else {
        set_model_download_snapshot(
            app,
            runtime,
            "failed",
            downloaded_bytes,
            total_bytes.max(downloaded_bytes),
            0,
            &format!("下载完成但缺少文件: {}", missing.join(", ")),
            true,
        )?;
        Err("missing required model files after download".to_string())
    }
}

#[tauri::command]
fn build_segments_from_words(
    request: BuildSegmentsRequest,
) -> Result<BuildSegmentsResponse, String> {
    let audio_path = PathBuf::from(&request.audio_path);
    let srt_output_path = task_srt_output_path(&request.task_id, &audio_path);

    let words: Vec<WordToken> = request.words.into_iter().map(response_to_word).collect();
    let segments = split_english_segments(&words);
    let srt = to_srt_from_segments(&segments);
    let text = plain_text_from_segments(&segments);

    if let Some(parent) = srt_output_path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }

    let segments_response: Vec<SegmentWithWordsResponse> =
        segments.iter().map(segment_to_response).collect();

    Ok(BuildSegmentsResponse {
        text,
        srt,
        srt_output_path: srt_output_path.display().to_string(),
        segments: segments_response,
    })
}

fn sanitize_filename_component(raw: &str) -> String {
    raw.chars()
        .map(|ch| match ch {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            _ => ch,
        })
        .collect::<String>()
        .trim()
        .trim_matches('.')
        .to_string()
}

fn task_output_dir(task_id: &str, audio_path: &std::path::Path) -> PathBuf {
    let stem = audio_path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "transcript".to_string());
    let safe_stem = sanitize_filename_component(&stem);
    let safe_task_id = sanitize_filename_component(task_id);
    resolve_output_dir().join(format!("{}_{}", safe_stem, safe_task_id))
}

fn task_srt_output_path(task_id: &str, audio_path: &std::path::Path) -> PathBuf {
    let stem = audio_path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "transcript".to_string());
    let safe_stem = sanitize_filename_component(&stem);
    task_output_dir(task_id, audio_path).join(format!("{}_en.srt", safe_stem))
}

fn response_to_word(response: WordTokenResponse) -> WordToken {
    WordToken {
        start: response.start,
        end: response.end,
        word: response.word,
    }
}

fn word_to_response(word: &WordToken) -> WordTokenResponse {
    WordTokenResponse {
        start: word.start,
        end: word.end,
        word: word.word.clone(),
    }
}

fn segment_to_response(
    segment: &voxtrans_core::subtitle::srt::SubtitleSegment,
) -> SegmentWithWordsResponse {
    SegmentWithWordsResponse {
        start: segment.start_sec,
        end: segment.end_sec,
        text: segment.text.clone(),
        words: segment
            .words
            .iter()
            .map(|w| WordTokenResponse {
                start: w.start,
                end: w.end,
                word: w.word.clone(),
            })
            .collect(),
    }
}

fn task_srt_draft_path(task_id: &str, audio_path: &std::path::Path) -> PathBuf {
    let stem = audio_path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "transcript".to_string());
    let safe_stem = sanitize_filename_component(&stem);
    task_output_dir(task_id, audio_path).join(format!("{}_en.draft.srt", safe_stem))
}

fn resolve_output_dir() -> PathBuf {
    if let Ok(custom_dir) = std::env::var("VOXTRANS_OUTPUT_DIR") {
        let path = PathBuf::from(custom_dir);
        if !path.as_os_str().is_empty() {
            return path;
        }
    }

    let tauri_manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let project_root = tauri_manifest_dir
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or(tauri_manifest_dir);
    project_root.join("output")
}

#[tauri::command]
fn load_subtitle_editor(request: SubtitleLoadRequest) -> Result<SubtitleLoadResponse, String> {
    let media_path = PathBuf::from(&request.media_path);
    let srt_path = task_srt_output_path(&request.task_id, &media_path);
    let draft_path = task_srt_draft_path(&request.task_id, &media_path);

    if let Some(parent) = srt_path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    if let Some(parent) = draft_path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }

    let persisted = std::fs::read_to_string(&srt_path).ok();
    let fallback = request.fallback_srt.filter(|s| !s.trim().is_empty());

    let mut content = persisted
        .or(fallback)
        .unwrap_or_default()
        .replace("\r\n", "\n");

    let mut using_draft = false;
    if let Ok(draft_content) = std::fs::read_to_string(&draft_path) {
        let should_use_draft = if let (Ok(draft_meta), Ok(srt_meta)) =
            (std::fs::metadata(&draft_path), std::fs::metadata(&srt_path))
        {
            draft_meta.modified().ok() > srt_meta.modified().ok()
        } else {
            true
        };

        if should_use_draft && !draft_content.trim().is_empty() {
            content = draft_content.replace("\r\n", "\n");
            using_draft = true;
        }
    }

    let warnings = match parse_srt(&content) {
        Ok(cues) => validate_cues(&normalize_cues(&cues)),
        Err(_err) if content.trim().is_empty() => Vec::new(),
        Err(err) => vec![format!("parse warning: {}", err)],
    };

    Ok(SubtitleLoadResponse {
        srt_path: srt_path.display().to_string(),
        draft_path: draft_path.display().to_string(),
        content,
        using_draft,
        warnings,
    })
}

#[tauri::command]
fn save_subtitle_editor(request: SubtitleSaveRequest) -> Result<SubtitleSaveResponse, String> {
    let media_path = PathBuf::from(&request.media_path);
    let srt_path = task_srt_output_path(&request.task_id, &media_path);
    let draft_path = task_srt_draft_path(&request.task_id, &media_path);

    let parsed = parse_srt(&request.content).map_err(|err| err.to_string())?;
    let normalized = normalize_cues(&parsed);
    let warnings = validate_cues(&normalized);
    let normalized_srt = to_srt_from_cues(&normalized);

    if request.autosave {
        if let Some(parent) = draft_path.parent() {
            std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        }
        std::fs::write(&draft_path, normalized_srt).map_err(|err| err.to_string())?;
    } else {
        if let Some(parent) = srt_path.parent() {
            std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        }
        std::fs::write(&srt_path, normalized_srt).map_err(|err| err.to_string())?;
        let _ = std::fs::remove_file(&draft_path);
    }

    Ok(SubtitleSaveResponse {
        srt_path: srt_path.display().to_string(),
        warnings,
    })
}

#[tauri::command]
fn save_srt(request: SaveSrtRequest) -> Result<(), String> {
    if let Some(parent) = std::path::Path::new(&request.output_path).parent() {
        std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    std::fs::write(&request.output_path, request.content).map_err(|err| err.to_string())
}

#[tauri::command]
fn get_file_size(path: String) -> Result<u64, String> {
    let metadata = std::fs::metadata(&path).map_err(|err| err.to_string())?;
    Ok(metadata.len())
}

#[tauri::command]
fn open_in_explorer(path: String) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg("/select,")
            .arg(path)
            .spawn()
            .map_err(|err| err.to_string())?;
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg("-R")
            .arg(path)
            .spawn()
            .map_err(|err| err.to_string())?;
        return Ok(());
    }

    #[cfg(target_os = "linux")]
    {
        let parent = std::path::PathBuf::from(path)
            .parent()
            .map(|p| p.to_path_buf())
            .ok_or_else(|| "invalid file path".to_string())?;
        std::process::Command::new("xdg-open")
            .arg(parent)
            .spawn()
            .map_err(|err| err.to_string())?;
        return Ok(());
    }

    #[allow(unreachable_code)]
    Err("unsupported platform".to_string())
}

#[tauri::command]
fn open_output_dir() -> Result<(), String> {
    let output_dir = resolve_output_dir();
    std::fs::create_dir_all(&output_dir).map_err(|err| err.to_string())?;

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(output_dir)
            .spawn()
            .map_err(|err| err.to_string())?;
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(output_dir)
            .spawn()
            .map_err(|err| err.to_string())?;
        return Ok(());
    }

    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(output_dir)
            .spawn()
            .map_err(|err| err.to_string())?;
        return Ok(());
    }

    #[allow(unreachable_code)]
    Err("unsupported platform".to_string())
}

#[tauri::command]
fn open_model_dir() -> Result<(), String> {
    let model_dir = resolve_install_model_dir();
    std::fs::create_dir_all(&model_dir).map_err(|err| err.to_string())?;

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(model_dir)
            .spawn()
            .map_err(|err| err.to_string())?;
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(model_dir)
            .spawn()
            .map_err(|err| err.to_string())?;
        return Ok(());
    }

    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(model_dir)
            .spawn()
            .map_err(|err| err.to_string())?;
        return Ok(());
    }

    #[allow(unreachable_code)]
    Err("unsupported platform".to_string())
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let app_handle = app.handle().clone();
            let pool = tauri::async_runtime::block_on(async { db::init_pool(&app_handle).await })?;
            app.manage(app_state::AppState {
                pool,
                model_download: Arc::new(std::sync::Mutex::new(
                    app_state::ModelDownloadRuntime::default(),
                )),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            transcribe,
            build_segments_from_words,
            save_srt,
            load_subtitle_editor,
            save_subtitle_editor,
            get_file_size,
            open_in_explorer,
            open_output_dir,
            open_model_dir,
            get_model_status,
            start_model_download,
            cancel_model_download,
            commands::preferences::load_user_preferences,
            commands::preferences::save_app_settings,
            commands::preferences::save_terms,
            commands::preferences::save_hotword_correction,
            commands::workspace::load_workspace_state,
            commands::workspace::save_queue_state,
            commands::history::record_task_event,
            commands::history::list_task_events,
            commands::history::list_task_summaries,
            commands::history::clear_task_events,
            commands::history::delete_task_summaries,
            commands::logs::append_task_log,
            commands::logs::read_task_log,
            commands::logs::clear_task_logs,
            commands::usage::record_task_llm_usage,
            commands::usage::get_task_llm_usage_summary,
            prompts::build_hotword_correction_prompts,
            prompts::build_punctuation_restore_prompt,
            llm::llm_interact,
            llm::llm_test_connection
        ])
        .run(tauri::generate_context!())
        .expect("error while running voxtrans desktop");
}










