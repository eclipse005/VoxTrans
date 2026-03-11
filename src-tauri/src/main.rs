#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::async_runtime::spawn_blocking;
use tauri::Emitter;
use voxtrans_core::subtitle::srt::{
    normalize_cues, parse_srt, to_srt_from_cues, to_srt_from_segments, validate_cues,
};
use voxtrans_core::subtitle::segmenter::{
    normalize_word_tokens, plain_text_from_segments, split_english_segments, words_from_timed_tokens, WordToken,
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
    ort_dir: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TranscribeResponse {
    words: Vec<WordTokenResponse>,
    segment_total: usize,
    audio_duration_sec: f64,
    transcribe_elapsed_sec: f64,
    rtfx: f64,
    execution_provider: String,
    ort_runtime: String,
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

#[tauri::command]
async fn transcribe(app: tauri::AppHandle, request: TranscribeRequest) -> Result<TranscribeResponse, String> {
    spawn_blocking(move || {
        let task_id = request.task_id.clone();
        let app_handle = app.clone();
        let mut options = TranscribeOptions::default();
        let audio_path = PathBuf::from(&request.audio_path);
        options.audio_path = audio_path.clone();
        options.provider = match request.provider.to_ascii_lowercase().as_str() {
            "cpu" => Provider::Cpu,
            "cuda" => Provider::Cuda,
            "directml" => Provider::DirectMl,
            other => return Err(format!("unsupported provider: {other}")),
        };
        options.timestamp_mode = TimestampKind::Words;
        options.chunk_target_seconds = request.chunk_target_seconds.clamp(60, 1800) as f64;

        if let Some(model_dir) = request.model_dir {
            options.model_dir = PathBuf::from(model_dir);
        }
        if let Some(ort_dir) = request.ort_dir {
            options.ort_dir = Some(PathBuf::from(ort_dir));
        }

        let output = voxtrans_core::transcribe_with_parakeet_v2_with_progress(&options, |current, total| {
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
            audio_duration_sec: output.audio_duration_sec,
            transcribe_elapsed_sec: output.transcribe_elapsed_sec,
            rtfx: output.rtfx,
            execution_provider: output.execution_provider.to_string(),
            ort_runtime: output.ort_runtime,
        })
    })
    .await
    .map_err(|err| err.to_string())?
}

#[tauri::command]
fn build_segments_from_words(request: BuildSegmentsRequest) -> Result<BuildSegmentsResponse, String> {
    let audio_path = PathBuf::from(&request.audio_path);
    let srt_output_path = default_srt_output_path(&audio_path);

    let words: Vec<WordToken> = request
        .words
        .into_iter()
        .map(response_to_word)
        .collect();
    let segments = split_english_segments(&words);
    let srt = to_srt_from_segments(&segments);
    let text = plain_text_from_segments(&segments);

    if let Some(parent) = srt_output_path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }

    let segments_response: Vec<SegmentWithWordsResponse> = segments
        .iter()
        .map(segment_to_response)
        .collect();

    std::fs::write(&srt_output_path, &srt).map_err(|err| err.to_string())?;

    Ok(BuildSegmentsResponse {
        text,
        srt,
        srt_output_path: srt_output_path.display().to_string(),
        segments: segments_response,
    })
}

fn default_srt_output_path(audio_path: &std::path::Path) -> PathBuf {
    let stem = audio_path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "transcript".to_string());

    resolve_output_dir().join(format!("{stem}_en.srt"))
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

fn segment_to_response(segment: &voxtrans_core::subtitle::srt::SubtitleSegment) -> SegmentWithWordsResponse {
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

fn default_srt_draft_path(audio_path: &std::path::Path) -> PathBuf {
    let stem = audio_path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "transcript".to_string());

    resolve_output_dir()
        .join(".draft")
        .join(format!("{stem}_en.draft.srt"))
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
    let media_path = PathBuf::from(request.media_path);
    let srt_path = default_srt_output_path(&media_path);
    let draft_path = default_srt_draft_path(&media_path);

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
    let media_path = PathBuf::from(request.media_path);
    let srt_path = default_srt_output_path(&media_path);
    let draft_path = default_srt_draft_path(&media_path);

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

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            transcribe,
            build_segments_from_words,
            save_srt,
            load_subtitle_editor,
            save_subtitle_editor,
            get_file_size,
            open_in_explorer,
            open_output_dir
        ])
        .run(tauri::generate_context!())
        .expect("error while running voxtrans desktop");
}
