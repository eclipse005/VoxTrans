use super::{
    ASR_MODEL_DOWNLOAD_FILES, DEMUCS_MODEL_DOWNLOAD_FILES, ModelTarget, REQUIRED_ASR_MODEL_FILES,
    compute_asr_download_bytes, resolve_engine_model_dir, runtime_for_target,
};
use crate::app_state::{
    AppState, ModelDownloadPhase, ModelDownloadRuntime, ModelDownloadStateSnapshot,
};
use serde::Serialize;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tauri::Emitter;

const DEMUCS_READY_MARKER: &str = ".demucs_ready";

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct ModelDownloadProgressEvent {
    target: ModelTarget,
    model: String,
    phase: ModelDownloadPhase,
    downloaded_bytes: u64,
    total_bytes: u64,
    speed_bytes_per_sec: u64,
    message: String,
}

pub fn start_model_download(
    app: tauri::AppHandle,
    state: &AppState,
    target: ModelTarget,
    model: Option<String>,
) -> Result<(), String> {
    let model = normalize_model_name(target, model);
    let model_download = runtime_for_target(state, target);
    {
        let model_dir = resolve_engine_model_dir(target);
        let (downloaded_bytes, total_bytes) = initial_bytes_for_target(target, &model_dir, &model);
        let mut guard = model_download
            .lock()
            .map_err(|_| "model download state lock poisoned".to_string())?;
        if guard.snapshot.phase == ModelDownloadPhase::Downloading {
            return Ok(());
        }
        guard.cancel_flag = Some(Arc::new(AtomicBool::new(false)));
        guard.active_model = Some(model.clone());
        guard.snapshot = ModelDownloadStateSnapshot {
            phase: ModelDownloadPhase::Downloading,
            downloaded_bytes,
            total_bytes,
            speed_bytes_per_sec: 0,
            message: "开始下载模型".to_string(),
        };
    }

    let app_for_thread = app.clone();
    let runtime_for_thread = model_download.clone();
    let model_for_thread = model.clone();
    tauri::async_runtime::spawn(async move {
        match tauri::async_runtime::spawn_blocking(move || {
            download_model_files(
                &app_for_thread,
                &runtime_for_thread,
                target,
                &model_for_thread,
            )
            .map_err(|err| format!("{} (model: {})", err, model_for_thread))
        })
        .await
        {
            Ok(Ok(())) => {}
            Ok(Err(err)) => {
                let _ = set_model_download_snapshot(
                    &app,
                    &model_download,
                    target,
                    &model,
                    ModelDownloadPhase::Failed,
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
                    target,
                    &model,
                    ModelDownloadPhase::Failed,
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

pub fn cancel_model_download(
    state: &AppState,
    target: ModelTarget,
    _model: Option<String>,
) -> Result<(), String> {
    let runtime = runtime_for_target(state, target);
    let guard = runtime
        .lock()
        .map_err(|_| "model download state lock poisoned".to_string())?;
    if let Some(flag) = guard.cancel_flag.as_ref() {
        flag.store(true, Ordering::Relaxed);
    }
    Ok(())
}

fn initial_bytes_for_target(target: ModelTarget, model_dir: &Path, model: &str) -> (u64, u64) {
    match target {
        ModelTarget::Asr => compute_asr_download_bytes(model_dir),
        ModelTarget::Demucs => {
            let file_name = format!("{}.safetensors", model);
            let expected = DEMUCS_MODEL_DOWNLOAD_FILES
                .iter()
                .find_map(|(name, _, size)| {
                    if *name == file_name {
                        Some(*size)
                    } else {
                        None
                    }
                })
                .unwrap_or(0);
            let target = model_dir.join(&file_name);
            let part = model_dir.join(format!("{}.part", file_name));
            let downloaded = if target.exists() {
                std::fs::metadata(&target).map(|m| m.len()).unwrap_or(0)
            } else {
                std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0)
            };
            (downloaded, expected.max(downloaded))
        }
    }
}

fn emit_model_download_progress(
    app: &tauri::AppHandle,
    target: ModelTarget,
    model: &str,
    snapshot: &ModelDownloadStateSnapshot,
) {
    let _ = app.emit(
        "model-download-progress",
        ModelDownloadProgressEvent {
            target,
            model: model.to_string(),
            phase: snapshot.phase,
            downloaded_bytes: snapshot.downloaded_bytes,
            total_bytes: snapshot.total_bytes,
            speed_bytes_per_sec: snapshot.speed_bytes_per_sec,
            message: snapshot.message.clone(),
        },
    );
}

#[allow(clippy::too_many_arguments)]
fn set_model_download_snapshot(
    app: &tauri::AppHandle,
    runtime: &Arc<Mutex<ModelDownloadRuntime>>,
    target: ModelTarget,
    model: &str,
    phase: ModelDownloadPhase,
    downloaded_bytes: u64,
    total_bytes: u64,
    speed_bytes_per_sec: u64,
    message: &str,
    clear_cancel_flag: bool,
) -> Result<(), String> {
    let snapshot = ModelDownloadStateSnapshot {
        phase,
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
        guard.active_model = if clear_cancel_flag {
            None
        } else {
            Some(model.to_string())
        };
        if clear_cancel_flag {
            guard.cancel_flag = None;
        }
    }
    emit_model_download_progress(app, target, model, &snapshot);
    Ok(())
}

fn download_model_files(
    app: &tauri::AppHandle,
    runtime: &Arc<Mutex<ModelDownloadRuntime>>,
    target: ModelTarget,
    model: &str,
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
    match target {
        ModelTarget::Asr => download_asr_model_files(app, runtime, &cancel_flag),
        ModelTarget::Demucs => download_demucs_model_files(app, runtime, &cancel_flag, model),
    }
}

fn download_asr_model_files(
    app: &tauri::AppHandle,
    runtime: &Arc<Mutex<ModelDownloadRuntime>>,
    cancel_flag: &Arc<AtomicBool>,
) -> Result<(), String> {
    let model_dir = resolve_engine_model_dir(ModelTarget::Asr);
    std::fs::create_dir_all(&model_dir).map_err(|err| err.to_string())?;

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) VoxTrans/0.1")
        .build()
        .map_err(|err| err.to_string())?;

    let mut downloaded_bytes: u64 = 0;
    let mut total_bytes: u64 = 0;
    let mut estimates: Vec<(&str, &str, PathBuf, u64)> = Vec::new();
    for (file_name, url, expected_size) in ASR_MODEL_DOWNLOAD_FILES {
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
    let mut last_speed_mark = Instant::now();
    let mut last_speed_bytes = downloaded_bytes;

    set_model_download_snapshot(
        app,
        runtime,
        ModelTarget::Asr,
        "parakeet-tdt-0.6b-v2",
        ModelDownloadPhase::Downloading,
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
                ModelTarget::Asr,
                "parakeet-tdt-0.6b-v2",
                ModelDownloadPhase::Cancelled,
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
                    ModelTarget::Asr,
                    "parakeet-tdt-0.6b-v2",
                    ModelDownloadPhase::Cancelled,
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
                last_speed_mark = Instant::now();
                set_model_download_snapshot(
                    app,
                    runtime,
                    ModelTarget::Asr,
                    "parakeet-tdt-0.6b-v2",
                    ModelDownloadPhase::Downloading,
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
            ModelTarget::Asr,
            "parakeet-tdt-0.6b-v2",
            ModelDownloadPhase::Downloading,
            downloaded_bytes,
            total_bytes,
            0,
            "模型下载中",
            false,
        )?;
    }

    let missing: Vec<&str> = REQUIRED_ASR_MODEL_FILES
        .iter()
        .copied()
        .filter(|name| !model_dir.join(name).exists())
        .collect();
    if missing.is_empty() {
        set_model_download_snapshot(
            app,
            runtime,
            ModelTarget::Asr,
            "parakeet-tdt-0.6b-v2",
            ModelDownloadPhase::Completed,
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
            ModelTarget::Asr,
            "parakeet-tdt-0.6b-v2",
            ModelDownloadPhase::Failed,
            downloaded_bytes,
            total_bytes.max(downloaded_bytes),
            0,
            &format!("下载完成但缺少文件: {}", missing.join(", ")),
            true,
        )?;
        Err("missing required model files after download".to_string())
    }
}

fn download_demucs_model_files(
    app: &tauri::AppHandle,
    runtime: &Arc<Mutex<ModelDownloadRuntime>>,
    cancel_flag: &Arc<AtomicBool>,
    model: &str,
) -> Result<(), String> {
    let model_dir = resolve_engine_model_dir(ModelTarget::Demucs);
    std::fs::create_dir_all(&model_dir).map_err(|err| err.to_string())?;
    let file_name = format!("{}.safetensors", model);
    let (url, expected_size) = DEMUCS_MODEL_DOWNLOAD_FILES
        .iter()
        .find_map(|(name, url, size)| {
            if *name == file_name {
                Some((*url, *size))
            } else {
                None
            }
        })
        .ok_or_else(|| format!("unknown demucs model: {}", model))?;
    let target = model_dir.join(&file_name);
    let part_path = model_dir.join(format!("{}.part", file_name));

    let mut downloaded_bytes = if target.exists() {
        std::fs::metadata(&target).map(|m| m.len()).unwrap_or(0)
    } else {
        std::fs::metadata(&part_path).map(|m| m.len()).unwrap_or(0)
    };
    let mut total_bytes = expected_size.max(downloaded_bytes);
    let mut speed_bytes_per_sec = 0_u64;
    let mut last_speed_mark = Instant::now();
    let mut last_speed_bytes = downloaded_bytes;

    set_model_download_snapshot(
        app,
        runtime,
        ModelTarget::Demucs,
        model,
        ModelDownloadPhase::Downloading,
        downloaded_bytes,
        total_bytes,
        speed_bytes_per_sec,
        "模型下载中",
        false,
    )?;

    if target.exists() && downloaded_bytes >= expected_size {
        let marker_path = model_dir.join(format!("{}_{}", DEMUCS_READY_MARKER, model));
        std::fs::write(marker_path, b"ready").map_err(|err| err.to_string())?;
        set_model_download_snapshot(
            app,
            runtime,
            ModelTarget::Demucs,
            model,
            ModelDownloadPhase::Completed,
            expected_size,
            expected_size,
            0,
            "模型下载完成",
            true,
        )?;
        return Ok(());
    }

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) VoxTrans/0.1")
        .build()
        .map_err(|err| err.to_string())?;

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
        downloaded_bytes = 0;
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
                ModelTarget::Demucs,
                model,
                ModelDownloadPhase::Cancelled,
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
        if elapsed >= 0.3 {
            speed_bytes_per_sec = ((downloaded_bytes.saturating_sub(last_speed_bytes)) as f64
                / elapsed)
                .round() as u64;
            last_speed_mark = Instant::now();
            last_speed_bytes = downloaded_bytes;
            set_model_download_snapshot(
                app,
                runtime,
                ModelTarget::Demucs,
                model,
                ModelDownloadPhase::Downloading,
                downloaded_bytes,
                total_bytes,
                speed_bytes_per_sec,
                "模型下载中",
                false,
            )?;
        }
    }

    std::fs::rename(&part_path, &target).map_err(|err| err.to_string())?;
    downloaded_bytes = std::fs::metadata(&target)
        .map(|m| m.len())
        .unwrap_or(downloaded_bytes);
    total_bytes = total_bytes.max(downloaded_bytes);
    let marker_path = model_dir.join(format!("{}_{}", DEMUCS_READY_MARKER, model));
    std::fs::write(marker_path, b"ready").map_err(|err| err.to_string())?;
    set_model_download_snapshot(
        app,
        runtime,
        ModelTarget::Demucs,
        model,
        ModelDownloadPhase::Completed,
        downloaded_bytes,
        total_bytes,
        0,
        "模型下载完成",
        true,
    )?;
    Ok(())
}

fn normalize_model_name(target: ModelTarget, model: Option<String>) -> String {
    match target {
        ModelTarget::Asr => "parakeet-tdt-0.6b-v2".to_string(),
        ModelTarget::Demucs => model.unwrap_or_else(|| "htdemucs_ft".to_string()),
    }
}
