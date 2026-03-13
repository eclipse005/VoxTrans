use super::{
    MODEL_DOWNLOAD_FILES, REQUIRED_MODEL_FILES, compute_model_download_bytes, resolve_model_dir,
};
use crate::app_state::{AppState, ModelDownloadRuntime, ModelDownloadStateSnapshot};
use serde::Serialize;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::Emitter;

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct ModelDownloadProgressEvent {
    phase: String,
    downloaded_bytes: u64,
    total_bytes: u64,
    speed_bytes_per_sec: u64,
    message: String,
}

pub fn start_model_download(app: tauri::AppHandle, state: &AppState) -> Result<(), String> {
    let model_download = state.model_download.clone();
    {
        let model_dir = resolve_model_dir();
        let (downloaded_bytes, total_bytes) = compute_model_download_bytes(&model_dir);
        let mut guard = model_download
            .lock()
            .map_err(|_| "model download state lock poisoned".to_string())?;
        if guard.snapshot.phase == "downloading" {
            return Ok(());
        }
        guard.cancel_flag = Some(Arc::new(AtomicBool::new(false)));
        guard.snapshot = ModelDownloadStateSnapshot {
            phase: "downloading".to_string(),
            downloaded_bytes,
            total_bytes,
            speed_bytes_per_sec: 0,
            message: "开始下载模型".to_string(),
        };
    }

    let app_for_thread = app.clone();
    let runtime_for_thread = model_download.clone();
    tauri::async_runtime::spawn(async move {
        match tauri::async_runtime::spawn_blocking(move || {
            download_model_files(&app_for_thread, &runtime_for_thread)
        })
        .await
        {
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
                let _ = set_model_download_snapshot(
                    &app,
                    &model_download,
                    "failed",
                    0,
                    0,
                    0,
                    &format!("下载任务异常: {}", err),
                    true,
                );
            }
        }
    });

    Ok(())
}

pub fn cancel_model_download(state: &AppState) -> Result<(), String> {
    let guard = state
        .model_download
        .lock()
        .map_err(|_| "model download state lock poisoned".to_string())?;
    if let Some(flag) = guard.cancel_flag.as_ref() {
        flag.store(true, Ordering::Relaxed);
    }
    Ok(())
}

fn emit_model_download_progress(app: &tauri::AppHandle, snapshot: &ModelDownloadStateSnapshot) {
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
    runtime: &Arc<Mutex<ModelDownloadRuntime>>,
    phase: &str,
    downloaded_bytes: u64,
    total_bytes: u64,
    speed_bytes_per_sec: u64,
    message: &str,
    clear_cancel_flag: bool,
) -> Result<(), String> {
    let snapshot = ModelDownloadStateSnapshot {
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
    runtime: &Arc<Mutex<ModelDownloadRuntime>>,
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
    let model_dir = resolve_model_dir();
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
        if !(response.status().is_success()
            || response.status() == reqwest::StatusCode::PARTIAL_CONTENT)
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
            file.write_all(&buf[..read])
                .map_err(|err| err.to_string())?;
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
