use super::download_http::{
    build_download_client, is_download_success_status, start_modelscope_download,
};
use super::download_progress::set_model_download_snapshot;
use super::{ModelDefinition, ModelTarget, model_definition, runtime_for_target};
use crate::app_state::{
    AppState, ModelDownloadPhase, ModelDownloadRuntime, ModelDownloadStateSnapshot,
};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

pub fn start_model_download(
    app: tauri::AppHandle,
    state: &AppState,
    target: ModelTarget,
    model: Option<String>,
) -> Result<(), String> {
    let definition = model_definition(target, model.as_deref())?;
    let model_download = runtime_for_target(state, target);
    {
        let (downloaded_bytes, total_bytes) = initial_bytes_for_definition(&definition);
        let mut guard = model_download
            .lock()
            .map_err(|_| "model download state lock poisoned".to_string())?;
        if guard.snapshot.phase == ModelDownloadPhase::Downloading {
            let active_model = guard
                .active_model
                .as_deref()
                .unwrap_or("unknown model")
                .to_string();
            return Err(format!("Model already downloading: {active_model}"));
        }
        guard.cancel_flag = Some(Arc::new(AtomicBool::new(false)));
        guard.active_model = Some(definition.model.clone());
        guard.snapshot = ModelDownloadStateSnapshot {
            phase: ModelDownloadPhase::Downloading,
            downloaded_bytes,
            total_bytes,
            speed_bytes_per_sec: 0,
            message: "starting_download".to_string(),
        };
    }

    let app_for_thread = app.clone();
    let runtime_for_thread = model_download.clone();
    let target_for_thread = target;
    let model_for_error = definition.model.clone();
    tauri::async_runtime::spawn(async move {
        match tauri::async_runtime::spawn_blocking(move || {
            download_model_files(&app_for_thread, &runtime_for_thread, &definition)
                .map_err(|err| format!("{} (model: {})", err, definition.model))
        })
        .await
        {
            Ok(Ok(())) => {}
            Ok(Err(err)) => {
                let _ = set_model_download_snapshot(
                    &app,
                    &model_download,
                    target_for_thread,
                    &model_for_error,
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
                    target_for_thread,
                    &model_for_error,
                    ModelDownloadPhase::Failed,
                    0,
                    0,
                    0,
                    &format!("Download task error: {}", err),
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

fn initial_bytes_for_definition(definition: &ModelDefinition) -> (u64, u64) {
    let mut downloaded_bytes = 0_u64;
    let mut total_bytes = 0_u64;
    for file in &definition.download_files {
        let target = definition.model_dir.join(&file.file_name);
        let part = definition
            .model_dir
            .join(format!("{}.part", file.file_name));
        let downloaded = if target.exists() {
            std::fs::metadata(&target).map(|m| m.len()).unwrap_or(0)
        } else {
            std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0)
        };
        downloaded_bytes = downloaded_bytes.saturating_add(downloaded.min(file.expected_size));
        total_bytes = total_bytes.saturating_add(file.expected_size.max(downloaded));
    }
    (downloaded_bytes, total_bytes)
}

fn download_model_files(
    app: &tauri::AppHandle,
    runtime: &Arc<Mutex<ModelDownloadRuntime>>,
    definition: &ModelDefinition,
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

    std::fs::create_dir_all(&definition.model_dir).map_err(|err| err.to_string())?;
    let (mut downloaded_bytes, mut total_bytes) = initial_bytes_for_definition(definition);
    let mut speed_bytes_per_sec = 0_u64;
    let mut last_speed_mark = Instant::now();
    let mut last_speed_bytes = downloaded_bytes;

    set_model_download_snapshot(
        app,
        runtime,
        definition.target,
        &definition.model,
        ModelDownloadPhase::Downloading,
        downloaded_bytes,
        total_bytes,
        speed_bytes_per_sec,
        "downloading",
        false,
    )?;

    let client = build_download_client()?;
    for file in &definition.download_files {
        let target = definition.model_dir.join(&file.file_name);
        if target.exists() {
            continue;
        }

        let part_path = definition
            .model_dir
            .join(format!("{}.part", file.file_name));
        let part_bytes = std::fs::metadata(&part_path).map(|m| m.len()).unwrap_or(0);
        let download_start = start_modelscope_download(&client, &file.url, &part_path, part_bytes)?;
        if download_start.restarted {
            downloaded_bytes = downloaded_bytes.saturating_sub(part_bytes);
        }
        let mut response = download_start.response;
        if !is_download_success_status(response.status()) {
            return Err(format!(
                "download failed: {} -> {}",
                file.file_name,
                response.status()
            ));
        }

        let mut output = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&part_path)
            .map_err(|err| err.to_string())?;

        read_response_to_part_file(
            app,
            runtime,
            definition,
            &cancel_flag,
            &mut response,
            &mut output,
            &mut downloaded_bytes,
            &mut total_bytes,
            &mut speed_bytes_per_sec,
            &mut last_speed_mark,
            &mut last_speed_bytes,
        )?;
        if cancel_flag.load(Ordering::Relaxed) {
            // Clean up the partial file so the next retry starts fresh.
            // Otherwise a server that returns 200 (full content) on retry
            // would append to a stale .part, inflating total_bytes past
            // 100% and corrupting progress. A 206 server would resume
            // correctly, but we can't tell in advance which one we'll
            // hit — deleting is the safe, predictable choice.
            let _ = std::fs::remove_file(&part_path);
            return Ok(());
        }
        std::fs::rename(&part_path, &target).map_err(|err| err.to_string())?;
        let file_bytes = std::fs::metadata(&target)
            .map(|m| m.len())
            .unwrap_or(file.expected_size);
        total_bytes = total_bytes.max(downloaded_bytes).max(file_bytes);
    }

    let missing_files = definition
        .required_files
        .iter()
        .filter(|name| !definition.model_dir.join(name).exists())
        .cloned()
        .collect::<Vec<_>>();
    if !missing_files.is_empty() {
        return Err(format!("Download finished but files missing: {}", missing_files.join(", ")));
    }

    set_model_download_snapshot(
        app,
        runtime,
        definition.target,
        &definition.model,
        ModelDownloadPhase::Completed,
        downloaded_bytes,
        total_bytes.max(downloaded_bytes),
        0,
        "download_complete",
        true,
    )?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn read_response_to_part_file(
    app: &tauri::AppHandle,
    runtime: &Arc<Mutex<ModelDownloadRuntime>>,
    definition: &ModelDefinition,
    cancel_flag: &Arc<AtomicBool>,
    response: &mut impl Read,
    output: &mut impl Write,
    downloaded_bytes: &mut u64,
    total_bytes: &mut u64,
    speed_bytes_per_sec: &mut u64,
    last_speed_mark: &mut Instant,
    last_speed_bytes: &mut u64,
) -> Result<(), String> {
    let mut buf = [0_u8; 64 * 1024];
    loop {
        if cancel_flag.load(Ordering::Relaxed) {
            set_model_download_snapshot(
                app,
                runtime,
                definition.target,
                &definition.model,
                ModelDownloadPhase::Cancelled,
                *downloaded_bytes,
                *total_bytes,
                0,
                "download_cancelled",
                true,
            )?;
            return Ok(());
        }
        let read = response.read(&mut buf).map_err(|err| err.to_string())?;
        if read == 0 {
            break;
        }
        output
            .write_all(&buf[..read])
            .map_err(|err| err.to_string())?;
        *downloaded_bytes = downloaded_bytes.saturating_add(read as u64);
        let elapsed = last_speed_mark.elapsed().as_secs_f64();
        if elapsed >= 0.3 {
            *speed_bytes_per_sec = ((*downloaded_bytes).saturating_sub(*last_speed_bytes) as f64
                / elapsed)
                .round() as u64;
            *last_speed_mark = Instant::now();
            *last_speed_bytes = *downloaded_bytes;
            set_model_download_snapshot(
                app,
                runtime,
                definition.target,
                &definition.model,
                ModelDownloadPhase::Downloading,
                *downloaded_bytes,
                *total_bytes,
                *speed_bytes_per_sec,
                "downloading",
                false,
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholder_downloads_keep_asr_and_align_entrypoints_active() {
        let asr = model_definition(ModelTarget::Asr, None)
            .expect("ASR model definition should be valid");
        let align = model_definition(ModelTarget::Align, None)
            .expect("align model definition should be valid");

        assert_eq!(asr.download_files.len(), asr.required_files.len());
        assert_eq!(align.download_files.len(), align.required_files.len());
        assert!(asr.download_files.iter().all(|file| !file.url.is_empty()));
        assert!(align.download_files.iter().all(|file| !file.url.is_empty()));
    }
}
