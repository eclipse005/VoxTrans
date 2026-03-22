use std::path::{Path, PathBuf};
use std::process::Command;
use std::process::Stdio;
use std::collections::HashMap;
use std::sync::mpsc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::sync::OnceLock;
use std::thread;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use tauri::Emitter;

use crate::services::task_engine::{
    RegisterTaskUploadRequest,
    TaskRunRecord,
    register_task_upload,
};
use crate::services::binary::resolve_bundled_or_path;
use crate::services::task_path::sanitize_filename_component;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadYoutubeRequest {
    pub url: String,
    #[serde(default)]
    pub task_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadYoutubeResponse {
    pub task: TaskRunRecord,
    pub output_dir: String,
    pub downloaded_path: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct YoutubeDownloadProgressEvent {
    task_id: String,
    phase: String,
    progress_percent: u32,
    title: String,
    speed: String,
    total_size: String,
    downloaded_size: String,
    eta: String,
    message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateYtDlpResponse {
    pub from_version: String,
    pub to_version: String,
    pub updated: bool,
    pub output: String,
}

static YOUTUBE_PROGRESS_SNAPSHOTS: OnceLock<std::sync::Mutex<HashMap<String, YoutubeDownloadProgressEvent>>> = OnceLock::new();
static YOUTUBE_CANCEL_FLAGS: OnceLock<std::sync::Mutex<HashMap<String, Arc<AtomicBool>>>> = OnceLock::new();

pub async fn download_youtube_to_task(
    pool: &SqlitePool,
    app: Option<tauri::AppHandle>,
    request: DownloadYoutubeRequest,
) -> Result<DownloadYoutubeResponse, String> {
    let url = request.url.trim().to_string();
    if url.is_empty() {
        return Err("url is required".to_string());
    }

    let task_id = if request.task_id.trim().is_empty() {
        new_task_id()
    } else {
        sanitize_filename_component(request.task_id.trim())
    };
    let yt_dlp = resolve_yt_dlp_binary()?;
    let descriptor = fetch_video_descriptor(&yt_dlp, &url)?;
    let output_dir = build_youtube_task_dir(&descriptor, &task_id);
    std::fs::create_dir_all(&output_dir).map_err(|err| err.to_string())?;
    let output_tpl = output_dir.join("%(title).200B.%(ext)s");

    let task_id_for_process = task_id.clone();
    let app_for_process = app.clone();
    let cancel_flag = register_youtube_cancel_flag(&task_id);
    let cancel_flag_for_process = cancel_flag.clone();
    let command_output_result: Result<YoutubeCommandOutput, String> = tokio::task::spawn_blocking(move || -> Result<YoutubeCommandOutput, String> {
        let mut command = Command::new(&yt_dlp);
        command
            .arg("--no-playlist")
            .arg("--progress")
            .arg("--newline")
            .arg("--progress-template")
            .arg("download:%(progress._percent_str)s|%(progress._total_bytes_str)s|%(progress._speed_str)s|%(progress._eta_str)s")
            .arg("-f")
            .arg("bestvideo[ext=mp4]+bestaudio[ext=m4a]/best[ext=mp4]/best")
            .arg("--merge-output-format")
            .arg("mp4")
            .arg("-o")
            .arg(&output_tpl)
            .arg(&url)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = command.spawn().map_err(|err| err.to_string())?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "failed to capture yt-dlp stdout".to_string())?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| "failed to capture yt-dlp stderr".to_string())?;

        let (tx, rx) = mpsc::channel::<(StreamKind, String)>();
        let stdout_thread = spawn_reader_thread(stdout, StreamKind::Stdout, tx.clone());
        let stderr_thread = spawn_reader_thread(stderr, StreamKind::Stderr, tx);

        let mut progress = YoutubeDownloadProgressEvent {
            task_id: task_id_for_process,
            phase: "starting".to_string(),
            progress_percent: 0,
            title: String::new(),
            speed: String::new(),
            total_size: String::new(),
            downloaded_size: String::new(),
            eta: String::new(),
            message: "准备下载".to_string(),
        };
        emit_progress_event(app_for_process.as_ref(), &progress);

        let mut stdout_lines: Vec<String> = Vec::new();
        let mut stderr_lines: Vec<String> = Vec::new();
        let status = loop {
            if cancel_flag_for_process.load(Ordering::SeqCst) {
                let _ = child.kill();
                let _ = child.wait();
                return Err("下载已取消".to_string());
            }
            while let Ok((kind, line)) = rx.try_recv() {
                if matches!(kind, StreamKind::Stdout) {
                    stdout_lines.push(line.clone());
                } else {
                    stderr_lines.push(line.clone());
                }
                update_progress_from_line(&mut progress, &line);
                emit_progress_event(app_for_process.as_ref(), &progress);
            }
            if let Some(status) = child.try_wait().map_err(|err| err.to_string())? {
                break status;
            }
            thread::sleep(Duration::from_millis(80));
        };

        let _ = stdout_thread.join();
        let _ = stderr_thread.join();
        while let Ok((kind, line)) = rx.try_recv() {
            if matches!(kind, StreamKind::Stdout) {
                stdout_lines.push(line.clone());
            } else {
                stderr_lines.push(line.clone());
            }
            update_progress_from_line(&mut progress, &line);
            emit_progress_event(app_for_process.as_ref(), &progress);
        }

        Ok(YoutubeCommandOutput {
            status,
            stdout: stdout_lines.join("\n"),
            stderr: stderr_lines.join("\n"),
        })
    })
    .await
    .map_err(|err| err.to_string())?;
    remove_youtube_cancel_flag(&task_id);
    let command_output = match command_output_result {
        Ok(v) => v,
        Err(err) => {
            emit_progress_event(
                app.as_ref(),
                &YoutubeDownloadProgressEvent {
                    task_id: task_id.clone(),
                    phase: "cancelled".to_string(),
                    progress_percent: 0,
                    title: String::new(),
                    speed: String::new(),
                    total_size: String::new(),
                    downloaded_size: String::new(),
                    eta: String::new(),
                    message: err.clone(),
                },
            );
            return Err(err);
        }
    };

    if !command_output.status.success() {
        let stderr = command_output.stderr.trim().to_string();
        let stdout = command_output.stdout.trim().to_string();
        let detail = if !stderr.is_empty() {
            stderr
        } else if !stdout.is_empty() {
            stdout
        } else {
            "yt-dlp failed".to_string()
        };
        emit_progress_event(
            app.as_ref(),
            &YoutubeDownloadProgressEvent {
                task_id: task_id.clone(),
                phase: "error".to_string(),
                progress_percent: 0,
                title: String::new(),
                speed: String::new(),
                total_size: String::new(),
                downloaded_size: String::new(),
                eta: String::new(),
                message: format!("下载失败: {detail}"),
            },
        );
        return Err(format!("YouTube 下载失败: {detail}"));
    }

    let downloaded_path = detect_downloaded_file_path(&command_output.stdout, &output_dir)
        .ok_or_else(|| "下载成功，但未找到输出文件".to_string())?;

    let metadata = std::fs::metadata(&downloaded_path).map_err(|err| err.to_string())?;
    let media_path = downloaded_path.to_string_lossy().to_string();
    let register = RegisterTaskUploadRequest {
        id: task_id.clone(),
        media_path: media_path.clone(),
        name: descriptor.display_name.clone(),
        media_kind: detect_media_kind(&downloaded_path).to_string(),
        size_bytes: metadata.len(),
    };
    let task = register_task_upload(pool, register).await?;
    emit_progress_event(
        app.as_ref(),
        &YoutubeDownloadProgressEvent {
            task_id: task_id.clone(),
            phase: "finished".to_string(),
            progress_percent: 100,
            title: task.name.clone(),
            speed: String::new(),
            total_size: human_size(metadata.len()),
            downloaded_size: human_size(metadata.len()),
            eta: String::new(),
            message: "下载完成".to_string(),
        },
    );

    Ok(DownloadYoutubeResponse {
        task,
        output_dir: output_dir.to_string_lossy().to_string(),
        downloaded_path: media_path,
    })
}

pub fn request_cancel_youtube_download(task_id: &str) -> bool {
    let guard = youtube_cancel_flags_mutex().lock();
    let Ok(guard) = guard else {
        return false;
    };
    if let Some(flag) = guard.get(task_id) {
        flag.store(true, Ordering::SeqCst);
        true
    } else {
        false
    }
}

pub fn get_yt_dlp_version() -> Result<String, String> {
    let yt_dlp = resolve_yt_dlp_binary()?;
    let output = Command::new(yt_dlp)
        .arg("--version")
        .output()
        .map_err(|err| format!("读取 yt-dlp 版本失败: {err}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let detail = if !stderr.is_empty() { stderr } else { stdout };
        return Err(if detail.is_empty() {
            "读取 yt-dlp 版本失败".to_string()
        } else {
            format!("读取 yt-dlp 版本失败: {detail}")
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub fn update_yt_dlp() -> Result<UpdateYtDlpResponse, String> {
    if has_active_youtube_downloads() {
        return Err("当前有下载任务进行中，请先取消或等待完成".to_string());
    }

    let yt_dlp = resolve_yt_dlp_binary()?;
    let from_version = get_yt_dlp_version().unwrap_or_default();
    let output = Command::new(&yt_dlp)
        .arg("-U")
        .output()
        .map_err(|err| format!("执行 yt-dlp 更新失败: {err}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let combined = format!("{stdout}\n{stderr}").trim().to_string();

    if !output.status.success() {
        let detail = if combined.is_empty() {
            "未知错误".to_string()
        } else {
            combined
        };
        return Err(format!("yt-dlp 更新失败: {detail}"));
    }

    let to_version = get_yt_dlp_version().unwrap_or_else(|_| from_version.clone());
    Ok(UpdateYtDlpResponse {
        from_version: from_version.clone(),
        to_version: to_version.clone(),
        updated: !from_version.is_empty() && !to_version.is_empty() && from_version != to_version,
        output: combined,
    })
}

fn emit_progress_event(app: Option<&tauri::AppHandle>, payload: &YoutubeDownloadProgressEvent) {
    if let Ok(mut guard) = youtube_progress_snapshots_mutex().lock() {
        guard.insert(payload.task_id.clone(), payload.clone());
    }
    if let Some(app_handle) = app {
        let _ = app_handle.emit("youtube-download-progress", payload);
    }
}

pub fn get_youtube_download_progress(task_id: &str) -> YoutubeDownloadProgressEvent {
    youtube_progress_snapshots_mutex()
        .lock()
        .ok()
        .and_then(|g| g.get(task_id).cloned())
        .unwrap_or_else(|| YoutubeDownloadProgressEvent {
            task_id: task_id.to_string(),
            phase: String::new(),
            progress_percent: 0,
            title: String::new(),
            speed: String::new(),
            total_size: String::new(),
            downloaded_size: String::new(),
            eta: String::new(),
            message: String::new(),
        })
}

pub fn list_youtube_download_progress() -> Vec<YoutubeDownloadProgressEvent> {
    let mut items = youtube_progress_snapshots_mutex()
        .lock()
        .map(|g| g.values().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    items.sort_by(|a, b| a.task_id.cmp(&b.task_id));
    items
}

fn youtube_progress_snapshots_mutex() -> &'static std::sync::Mutex<HashMap<String, YoutubeDownloadProgressEvent>> {
    YOUTUBE_PROGRESS_SNAPSHOTS.get_or_init(|| std::sync::Mutex::new(HashMap::new()))
}

fn youtube_cancel_flags_mutex() -> &'static std::sync::Mutex<HashMap<String, Arc<AtomicBool>>> {
    YOUTUBE_CANCEL_FLAGS.get_or_init(|| std::sync::Mutex::new(HashMap::new()))
}

fn register_youtube_cancel_flag(task_id: &str) -> Arc<AtomicBool> {
    let flag = Arc::new(AtomicBool::new(false));
    if let Ok(mut guard) = youtube_cancel_flags_mutex().lock() {
        guard.insert(task_id.to_string(), flag.clone());
    }
    flag
}

fn remove_youtube_cancel_flag(task_id: &str) {
    if let Ok(mut guard) = youtube_cancel_flags_mutex().lock() {
        guard.remove(task_id);
    }
}

fn has_active_youtube_downloads() -> bool {
    youtube_cancel_flags_mutex()
        .lock()
        .map(|g| !g.is_empty())
        .unwrap_or(false)
}

fn update_progress_from_line(progress: &mut YoutubeDownloadProgressEvent, raw: &str) {
    let normalized = normalize_progress_line(raw);
    let line = normalized.trim();
    if line.is_empty() {
        return;
    }

    if let Some((percent, total, speed, eta)) = parse_pipe_progress_line(line) {
        progress.progress_percent = percent;
        progress.phase = "downloading".to_string();
        if !total.is_empty() && !total.eq_ignore_ascii_case("NA") {
            progress.total_size = total;
        }
        if !speed.is_empty() && !speed.eq_ignore_ascii_case("NA") && !speed.eq_ignore_ascii_case("Unknown B/s") {
            progress.speed = speed;
        }
        if !eta.is_empty() && !eta.eq_ignore_ascii_case("NA") && !eta.eq_ignore_ascii_case("Unknown") {
            progress.eta = eta;
        }
        progress.downloaded_size = estimate_downloaded_size(progress.progress_percent, &progress.total_size);
        progress.message = if progress.progress_percent >= 100 {
            "下载完成，处理文件中".to_string()
        } else {
            format!("下载中 {}%", progress.progress_percent)
        };
        return;
    }

    if let Some(title) = line.strip_prefix("[download] Destination: ") {
        progress.title = title.trim().to_string();
        progress.phase = "downloading".to_string();
        progress.message = "开始下载".to_string();
        return;
    }

    if line.starts_with("[Merger]") {
        progress.phase = "merging".to_string();
        progress.progress_percent = 99;
        progress.message = "合并音视频".to_string();
        return;
    }

}

fn parse_pipe_progress_line(line: &str) -> Option<(u32, String, String, String)> {
    let mut parts = line.split('|');
    let percent_raw = parts.next()?.trim();
    if !percent_raw.contains('%') {
        return None;
    }
    let percent = parse_percent(percent_raw)?;
    let total = parts.next().unwrap_or_default().trim().to_string();
    let speed = parts.next().unwrap_or_default().trim().to_string();
    let eta = parts.next().unwrap_or_default().trim().to_string();
    Some((percent, total, speed, eta))
}

fn normalize_progress_line(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut in_ansi = false;
    for ch in raw.chars() {
        if in_ansi {
            if ch.is_ascii_alphabetic() {
                in_ansi = false;
            }
            continue;
        }
        if ch == '\u{1b}' {
            in_ansi = true;
            continue;
        }
        if ch.is_control() && ch != '\t' && ch != ' ' {
            continue;
        }
        out.push(ch);
    }
    out
}

fn parse_percent(raw: &str) -> Option<u32> {
    let value = raw.trim().trim_end_matches('%').parse::<f64>().ok()?;
    if !value.is_finite() {
        return None;
    }
    Some(value.round().clamp(0.0, 100.0) as u32)
}

fn estimate_downloaded_size(percent: u32, total: &str) -> String {
    let cleaned = total.trim();
    if cleaned.is_empty() {
        return String::new();
    }
    let number_part = cleaned
        .chars()
        .take_while(|ch| ch.is_ascii_digit() || *ch == '.')
        .collect::<String>();
    if number_part.is_empty() {
        return String::new();
    }
    let unit_part = cleaned[number_part.len()..].trim();
    let Ok(total_number) = number_part.parse::<f64>() else {
        return String::new();
    };
    let current = total_number * (percent as f64 / 100.0);
    if unit_part.is_empty() {
        return format!("{current:.1}");
    }
    format!("{current:.1}{unit_part}")
}

fn human_size(bytes: u64) -> String {
    if bytes == 0 {
        return "0B".to_string();
    }
    let units = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut idx = 0usize;
    while value >= 1024.0 && idx < units.len() - 1 {
        value /= 1024.0;
        idx += 1;
    }
    if idx == 0 {
        format!("{value:.0}{}", units[idx])
    } else {
        format!("{value:.1}{}", units[idx])
    }
}

struct YoutubeVideoDescriptor {
    display_name: String,
    dir_name: String,
}

struct YoutubeCommandOutput {
    status: std::process::ExitStatus,
    stdout: String,
    stderr: String,
}

#[derive(Clone, Copy)]
enum StreamKind {
    Stdout,
    Stderr,
}

fn spawn_reader_thread(
    stream: impl std::io::Read + Send + 'static,
    kind: StreamKind,
    tx: mpsc::Sender<(StreamKind, String)>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut reader = stream;
        let mut buffer = [0u8; 4096];
        let mut current = Vec::<u8>::new();
        loop {
            let read = match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(n) => n,
                Err(_) => break,
            };
            for byte in &buffer[..read] {
                if *byte == b'\n' || *byte == b'\r' {
                    if !current.is_empty() {
                        if let Ok(line) = String::from_utf8(current.clone()) {
                            let text = line.trim().to_string();
                            if !text.is_empty() {
                                let _ = tx.send((kind, text));
                            }
                        }
                        current.clear();
                    }
                } else {
                    current.push(*byte);
                }
            }
        }
        if !current.is_empty() {
            if let Ok(line) = String::from_utf8(current) {
                let text = line.trim().to_string();
                if !text.is_empty() {
                    let _ = tx.send((kind, text));
                }
            }
        }
    })
}

fn fetch_video_descriptor(yt_dlp: &Path, url: &str) -> Result<YoutubeVideoDescriptor, String> {
    let output = Command::new(yt_dlp)
        .arg("--no-playlist")
        .arg("--skip-download")
        .arg("--print")
        .arg("%(title).200B")
        .arg(url)
        .output()
        .map_err(|err| err.to_string())?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let detail = if !stderr.is_empty() { stderr } else { stdout };
        return Err(if detail.is_empty() {
            "获取视频信息失败".to_string()
        } else {
            format!("获取视频信息失败: {detail}")
        });
    }

    let stdout_text = String::from_utf8_lossy(&output.stdout).to_string();
    let raw = stdout_text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("youtube_task");
    let safe = sanitize_filename_component(raw);
    let display = if safe.is_empty() {
        "youtube_task".to_string()
    } else {
        safe
    };
    Ok(YoutubeVideoDescriptor {
        display_name: display.clone(),
        dir_name: display,
    })
}

fn build_youtube_task_dir(descriptor: &YoutubeVideoDescriptor, task_id: &str) -> PathBuf {
    let base = crate::services::output::resolve_output_dir();
    base.join(format!("{}_{}", descriptor.dir_name, task_id))
}

fn resolve_yt_dlp_binary() -> Result<PathBuf, String> {
    Ok(resolve_bundled_or_path("yt-dlp"))
}

fn detect_downloaded_file_path(stdout: &str, output_dir: &Path) -> Option<PathBuf> {
    for line in stdout.lines().map(str::trim).filter(|line| !line.is_empty()) {
        if let Some(path) = parse_merged_output_path(line) {
            if path.is_file() && is_media_file(&path) {
                return Some(path);
            }
        }
    }

    for line in stdout.lines().map(str::trim).filter(|line| !line.is_empty()) {
        if let Some(path) = line.strip_prefix("[download] Destination: ").map(|v| PathBuf::from(v.trim())) {
            if path.is_file() && is_media_file(&path) {
                return Some(path);
            }
        }
    }

    let mut files: Vec<PathBuf> = std::fs::read_dir(output_dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && is_media_file(path))
        .collect();

    files.sort_by(|a, b| {
        let a_meta = a.metadata().ok();
        let b_meta = b.metadata().ok();
        let a_size = a_meta.as_ref().map(|m| m.len()).unwrap_or(0);
        let b_size = b_meta.as_ref().map(|m| m.len()).unwrap_or(0);
        if a_size != b_size {
            return a_size.cmp(&b_size);
        }
        let a_time = a_meta
            .and_then(|m| m.modified().ok())
            .unwrap_or(UNIX_EPOCH);
        let b_time = b_meta
            .and_then(|m| m.modified().ok())
            .unwrap_or(UNIX_EPOCH);
        a_time.cmp(&b_time)
    });

    files.pop()
}

fn parse_merged_output_path(line: &str) -> Option<PathBuf> {
    let marker = "[Merger] Merging formats into ";
    let rest = line.strip_prefix(marker)?.trim();
    let raw = rest
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .unwrap_or(rest);
    Some(PathBuf::from(raw))
}

fn is_media_file(path: &Path) -> bool {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    matches!(
        ext.as_str(),
        "mp4" | "mkv" | "mov" | "avi" | "webm" | "m4v" | "mp3" | "wav" | "m4a" | "flac" | "aac"
    )
}

fn detect_media_kind(path: &Path) -> &'static str {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match ext.as_str() {
        "mp4" | "mkv" | "mov" | "avi" | "webm" | "m4v" => "video",
        _ => "audio",
    }
}

fn new_task_id() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed) % 10_000;
    sanitize_filename_component(&format!("yt-{millis}-{seq:04}"))
}
