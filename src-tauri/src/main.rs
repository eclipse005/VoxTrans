#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use voxtrans_core::subtitle::srt::to_srt_from_sentence_tokens;
use voxtrans_core::{transcribe_with_parakeet_v2, Provider, TimestampKind, TranscribeOptions};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TranscribeRequest {
    audio_path: String,
    provider: String,
    chunk_target_seconds: u32,
    model_dir: Option<String>,
    ort_dir: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TranscribeResponse {
    text: String,
    srt: String,
    audio_duration_sec: f64,
    transcribe_elapsed_sec: f64,
    rtfx: f64,
    execution_provider: String,
    ort_runtime: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SaveSrtRequest {
    output_path: String,
    content: String,
}

#[tauri::command]
fn transcribe(request: TranscribeRequest) -> Result<TranscribeResponse, String> {
    let mut options = TranscribeOptions::default();
    options.audio_path = PathBuf::from(request.audio_path);
    options.provider = match request.provider.to_ascii_lowercase().as_str() {
        "cpu" => Provider::Cpu,
        "cuda" => Provider::Cuda,
        "directml" => Provider::DirectMl,
        other => return Err(format!("unsupported provider: {other}")),
    };
    options.timestamp_mode = TimestampKind::Sentences;
    options.chunk_target_seconds = request.chunk_target_seconds.clamp(60, 1800) as f64;

    if let Some(model_dir) = request.model_dir {
        options.model_dir = PathBuf::from(model_dir);
    }
    if let Some(ort_dir) = request.ort_dir {
        options.ort_dir = Some(PathBuf::from(ort_dir));
    }

    let output = transcribe_with_parakeet_v2(&options).map_err(|err| err.to_string())?;
    let srt = to_srt_from_sentence_tokens(&output.tokens);

    Ok(TranscribeResponse {
        text: output.text,
        srt,
        audio_duration_sec: output.audio_duration_sec,
        transcribe_elapsed_sec: output.transcribe_elapsed_sec,
        rtfx: output.rtfx,
        execution_provider: output.execution_provider.to_string(),
        ort_runtime: output.ort_runtime,
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

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            transcribe,
            save_srt,
            get_file_size,
            open_in_explorer
        ])
        .run(tauri::generate_context!())
        .expect("error while running voxtrans desktop");
}
