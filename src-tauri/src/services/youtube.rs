use serde::{Deserialize, Serialize};
use dashmap::DashMap;
use std::collections::VecDeque;
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::Emitter;

use super::binary::{configure_background_command, resolve_bundled_or_path};
use super::output::resolve_output_dir;
use super::task_path::sanitize_filename_component;

/// Proxy handling is deliberately yt-dlp's own business: it already reads the
/// environment and, on Windows, the WinINET system proxy that Clash/V2Ray-style
/// clients register. Inventing a proxy here would break the setups that expose
/// no local port at all (TUN / LSP / WFP based clients). What this module adds
/// instead is a deterministic working directory, hard deadlines, and a log, so
/// a stall is reported instead of quietly hanging forever.
///
/// Resolving video info must come back or die — without a deadline a network
/// stall leaves the command pending and the UI never reacts.
const METADATA_TIMEOUT: Duration = Duration::from_secs(120);
/// A running yt-dlp prints something at least once a second, so a long silence
/// means it is wedged on a socket rather than working.
const DOWNLOAD_STALL_TIMEOUT: Duration = Duration::from_secs(300);
/// Prefer H.264 + AAC. The default `bestvideo*+bestaudio` selector picks the
/// AV1/VP9 DASH streams, which YouTube frequently refuses with HTTP 403.
const MP4_FORMAT_SELECTOR: &str =
    "bestvideo[vcodec*=avc1]+bestaudio[ext=m4a]/bestvideo[ext=mp4]+bestaudio[ext=m4a]/best[ext=mp4]/best";
/// Diagnostics live in the output root, next to the per-task folders.
const YTDLP_LOG_FILE: &str = "yt-dlp.log";
/// Keep a few verbose lines per run: enough to see which proxy yt-dlp picked
/// and which config files it loaded, without dumping an entire session.
const DEBUG_LINE_LIMIT: usize = 12;
/// Environment variables that carry a proxy yt-dlp should use.
const PROXY_ENV_KEYS: [&str; 6] = [
    "HTTPS_PROXY",
    "https_proxy",
    "HTTP_PROXY",
    "http_proxy",
    "ALL_PROXY",
    "all_proxy",
];
const NO_PROXY_ENV_KEYS: [&str; 2] = ["NO_PROXY", "no_proxy"];
/// yt-dlp picks the Windows code page for its own output even when stdio is a
/// pipe — `--verbose` reports `out gbk` — so a CJK title arrived as GBK bytes
/// and decayed into U+FFFD replacement characters once decoded. `--encoding` is
/// yt-dlp's own switch for this; the environment variable still covers anything
/// printed before the options are parsed, such as a bootloader failure.
const YTDLP_OUTPUT_ENCODING: &str = "utf-8";
const PYTHON_IO_ENCODING: (&str, &str) = ("PYTHONIOENCODING", "utf-8");

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadYoutubeTask {
    pub id: String,
    pub media_path: String,
    pub name: String,
    pub media_kind: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadYoutubeTaskResponse {
    pub task: DownloadYoutubeTask,
    pub output_dir: String,
    pub downloaded_path: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct YoutubeDownloadProgressResponse {
    pub task_id: String,
    pub phase: String,
    pub progress_percent: f64,
    pub title: String,
    pub speed: String,
    pub total_size: String,
    pub downloaded_size: String,
    pub eta: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateYtDlpResponse {
    pub from_version: String,
    pub to_version: String,
    pub updated: bool,
    pub output: String,
}

#[derive(Debug, Clone, Deserialize)]
struct YoutubeVideoMetadata {
    #[serde(default)]
    title: String,
    #[serde(default)]
    filesize: Option<u64>,
    #[serde(default)]
    filesize_approx: Option<u64>,
}

static YOUTUBE_PROGRESS: OnceLock<DashMap<String, YoutubeDownloadProgressResponse>> =
    OnceLock::new();
static YOUTUBE_CANCEL_FLAGS: OnceLock<DashMap<String, Arc<AtomicBool>>> = OnceLock::new();

fn progress_snapshots() -> &'static DashMap<String, YoutubeDownloadProgressResponse> {
    YOUTUBE_PROGRESS.get_or_init(DashMap::new)
}

fn cancel_flags() -> &'static DashMap<String, Arc<AtomicBool>> {
    YOUTUBE_CANCEL_FLAGS.get_or_init(DashMap::new)
}

pub fn download_youtube_to_task(
    app: &tauri::AppHandle,
    url: String,
    task_id: Option<String>,
) -> Result<DownloadYoutubeTaskResponse, String> {
    let url = url.trim().to_string();
    if url.is_empty() {
        return Err("YouTube link is empty".to_string());
    }
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err("Please enter a valid YouTube link".to_string());
    }

    let task_id = task_id
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(default_task_id);
    let safe_task_id = sanitize_filename_component(&task_id);
    if safe_task_id.is_empty() {
        return Err("taskId is invalid".to_string());
    }

    let cancel = Arc::new(AtomicBool::new(false));
    cancel_flags().insert(task_id.clone(), cancel.clone());

    let mut snapshot = new_progress(&task_id, "starting", "parsing_video_info");
    set_progress(app, snapshot.clone());

    // Resolved up front: yt-dlp is spawned with this directory as its working
    // directory, so the config file it looks for is always the same one.
    let output_root = resolve_output_dir();

    let metadata = match fetch_youtube_metadata(&output_root, &task_id, &url) {
        Ok(value) => value,
        Err(err) => {
            snapshot.phase = "failed".to_string();
            snapshot.message = err.clone();
            set_progress(app, snapshot);
            remove_cancel_flag(&task_id);
            return Err(err);
        }
    };
    if cancel.load(Ordering::SeqCst) {
        snapshot.phase = "cancelled".to_string();
        snapshot.message = "cancelled".to_string();
        set_progress(app, snapshot);
        remove_cancel_flag(&task_id);
        return Err("YouTube download cancelled".to_string());
    }
    let output_dir =
        match youtube_output_dir_for_title(&output_root, &metadata.title, &task_id) {
            Ok(value) => value,
            Err(err) => {
                snapshot.phase = "failed".to_string();
                snapshot.message = err.clone();
                set_progress(app, snapshot);
                remove_cancel_flag(&task_id);
                return Err(err);
            }
        };
    if let Err(err) = std::fs::create_dir_all(&output_dir) {
        let message = format!("Failed to create download directory: {err}");
        snapshot.phase = "failed".to_string();
        snapshot.message = message.clone();
        set_progress(app, snapshot);
        remove_cancel_flag(&task_id);
        return Err(message);
    }

    snapshot.title = metadata.title.clone();
    if let Some(size_bytes) = metadata_size_bytes(&metadata) {
        snapshot.total_size = format_size_bytes(size_bytes);
    }
    snapshot.message = "preparing_download".to_string();
    set_progress(app, snapshot.clone());

    let result = run_ytdlp_download(
        app,
        &task_id,
        &url,
        &output_root,
        &output_dir,
        &cancel,
        &mut snapshot,
    );
    remove_cancel_flag(&task_id);
    result
}

pub fn get_download_progress(task_id: &str) -> Result<YoutubeDownloadProgressResponse, String> {
    let task_id = task_id.trim();
    if task_id.is_empty() {
        return Err("taskId is required".to_string());
    }
    Ok(progress_snapshots()
        .get(task_id)
        .map(|r| r.clone())
        .unwrap_or_else(|| new_progress(task_id, "idle", "")))
}

pub fn list_download_progress() -> Result<Vec<YoutubeDownloadProgressResponse>, String> {
    Ok(progress_snapshots()
        .iter()
        .map(|r| r.clone())
        .collect())
}

pub fn request_cancel(task_id: &str) -> bool {
    cancel_flags()
        .get(task_id)
        .map(|flag| {
            flag.store(true, Ordering::SeqCst);
            true
        })
        .unwrap_or(false)
}

pub fn get_yt_dlp_version() -> Result<String, String> {
    let output = build_yt_dlp_command(&resolve_output_dir())
        .arg("--version")
        .output()
        .map_err(|err| format!("Failed to run yt-dlp: {err}"))?;
    if !output.status.success() {
        return Err(command_output_message("Failed to detect yt-dlp version", &output));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub fn update_yt_dlp() -> Result<UpdateYtDlpResponse, String> {
    let from_version = get_yt_dlp_version().unwrap_or_default();
    let output = build_yt_dlp_command(&resolve_output_dir())
        .arg("-U")
        .output()
        .map_err(|err| format!("Failed to run yt-dlp update: {err}"))?;
    let text = command_output_text(&output);
    if !output.status.success() {
        return Err(format!("yt-dlp update failed: {text}"));
    }
    let to_version = get_yt_dlp_version().unwrap_or_else(|_| from_version.clone());
    Ok(UpdateYtDlpResponse {
        updated: !from_version.is_empty() && from_version != to_version,
        from_version,
        to_version,
        output: text,
    })
}

fn run_ytdlp_download(
    app: &tauri::AppHandle,
    task_id: &str,
    url: &str,
    output_root: &Path,
    output_dir: &Path,
    cancel: &AtomicBool,
    snapshot: &mut YoutubeDownloadProgressResponse,
) -> Result<DownloadYoutubeTaskResponse, String> {
    let mut command = build_yt_dlp_command(output_dir);
    command
        .arg("--verbose")
        .arg("--newline")
        .arg("--progress")
        .arg("--no-playlist")
        .arg("--retries")
        .arg("15")
        .arg("--fragment-retries")
        .arg("15")
        .arg("-f")
        .arg(MP4_FORMAT_SELECTOR)
        .arg("--merge-output-format")
        .arg("mp4")
        .arg("--print")
        .arg("after_move:filepath")
        .arg("-P")
        .arg(output_dir)
        .arg("-o")
        .arg("%(title).200B.%(ext)s")
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(ffmpeg_dir) = resolve_ffmpeg_dir() {
        command.arg("--ffmpeg-location").arg(ffmpeg_dir);
    }

    log_ytdlp(
        output_root,
        task_id,
        &format!(
            "download start | dir={} | env_proxy_keys={} | url={url}",
            output_dir.display(),
            proxy_env_summary()
        ),
    );

    let mut child = command.spawn().map_err(|err| {
        let message = format!("Failed to start yt-dlp: {err}");
        log_ytdlp(output_root, task_id, &message);
        message
    })?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "Failed to read yt-dlp output".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "Failed to read yt-dlp error output".to_string())?;

    let (tx, rx) = mpsc::channel::<String>();
    let stdout_reader = spawn_line_reader(stdout, tx.clone());
    let stderr_reader = spawn_line_reader(stderr, tx);
    let mut line_state = YtdlpLineState {
        final_path: None,
        recent_lines: VecDeque::new(),
        // Start "due" so the first download-progress line emits immediately.
        last_download_emit: Instant::now()
            .checked_sub(Duration::from_millis(300))
            .unwrap_or_else(Instant::now),
    };
    let mut debug_lines: Vec<String> = Vec::new();
    let mut last_output_at = Instant::now();

    loop {
        while let Ok(line) = rx.try_recv() {
            last_output_at = Instant::now();
            collect_ytdlp_debug(&mut debug_lines, &line);
            handle_ytdlp_line(app, task_id, output_dir, &line, snapshot, &mut line_state);
        }

        if cancel.load(Ordering::SeqCst) {
            let _ = child.kill();
            let _ = child.wait();
            log_ytdlp(output_root, task_id, "download cancelled by user");
            snapshot.phase = "cancelled".to_string();
            snapshot.message = "cancelled".to_string();
            set_progress(app, snapshot.clone());
            return Err("YouTube download cancelled".to_string());
        }

        // yt-dlp emits something at least every second while it is working, so
        // silence this long means a wedged socket, not a slow transfer.
        if last_output_at.elapsed() >= DOWNLOAD_STALL_TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            let message = stalled_message(&line_state.recent_lines);
            log_ytdlp(output_root, task_id, &format!("download stalled | {message}"));
            log_ytdlp(output_root, task_id, &debug_blob(&debug_lines));
            snapshot.phase = "failed".to_string();
            snapshot.message = message.clone();
            set_progress(app, snapshot.clone());
            return Err(message);
        }

        if let Some(status) = child.try_wait().map_err(|err| err.to_string())? {
            let _ = stdout_reader.join();
            let _ = stderr_reader.join();
            while let Ok(line) = rx.try_recv() {
                collect_ytdlp_debug(&mut debug_lines, &line);
                handle_ytdlp_line(app, task_id, output_dir, &line, snapshot, &mut line_state);
            }

            log_ytdlp(output_root, task_id, &debug_blob(&debug_lines));
            if !status.success() {
                let message =
                    recent_output_message(&line_state.recent_lines, "YouTube download failed");
                log_ytdlp(output_root, task_id, &format!("download failed | {message}"));
                snapshot.phase = "failed".to_string();
                snapshot.message = message.clone();
                set_progress(app, snapshot.clone());
                return Err(message);
            }
            break;
        }

        thread::sleep(Duration::from_millis(100));
    }

    let downloaded_path = line_state
        .final_path
        .filter(|path| path.is_file())
        .or_else(|| find_downloaded_media(output_dir))
        .ok_or_else(|| "YouTube download finished but no media file was found".to_string())?;
    let metadata =
        std::fs::metadata(&downloaded_path).map_err(|err| format!("Failed to read downloaded file: {err}"))?;
    let name = media_name(&downloaded_path);
    snapshot.phase = "completed".to_string();
    snapshot.progress_percent = 100.0;
    snapshot.title = name.clone();
    snapshot.message = "done".to_string();
    set_progress(app, snapshot.clone());
    log_ytdlp(
        output_root,
        task_id,
        &format!("download done | path={}", downloaded_path.display()),
    );

    Ok(DownloadYoutubeTaskResponse {
        task: DownloadYoutubeTask {
            id: task_id.to_string(),
            media_path: downloaded_path.to_string_lossy().to_string(),
            name,
            media_kind: media_kind_for_path(&downloaded_path).to_string(),
            size_bytes: metadata.len(),
        },
        output_dir: output_dir.to_string_lossy().to_string(),
        downloaded_path: downloaded_path.to_string_lossy().to_string(),
    })
}

fn build_yt_dlp_command(working_dir: &Path) -> Command {
    let mut command = Command::new(resolve_tool("yt-dlp"));
    configure_background_command(&mut command);
    // Pin the working directory. yt-dlp also loads `yt-dlp.conf` from the
    // current directory, and such a file silently overrides the proxy it took
    // from the system (a single `--proxy` line there is enough to break every
    // download). A GUI process is handed whatever cwd the shell decided, so it
    // must not be left to chance.
    command.current_dir(working_dir);
    command.env(PYTHON_IO_ENCODING.0, PYTHON_IO_ENCODING.1);
    command.arg("--encoding").arg(YTDLP_OUTPUT_ENCODING);
    clear_stray_no_proxy(&mut command);
    command
}

fn env_is_set(key: &str) -> bool {
    std::env::var(key)
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
}

/// Drop a `no_proxy` that only serves to switch the system proxy off.
///
/// `urllib.request.getproxies()` is `getproxies_environment() or
/// getproxies_registry()`, and `no_proxy` also ends in `_proxy`, so a *single*
/// non-empty `no_proxy` makes the environment branch truthy —
/// `{'no': 'localhost,127.0.0.1,::1'}` — and the registry is never consulted.
/// yt-dlp then has no proxy at all, which on a filtered network is a hang
/// rather than an error, while a plain `yt-dlp.bat` that passes `--proxy`
/// keeps working. A bypass list only means something next to a proxy that
/// comes from the environment, so when the environment carries none, this
/// variable is removed and the system setting wins; the system's own bypass
/// list already covers localhost and 127.*.
fn clear_stray_no_proxy(command: &mut Command) {
    if PROXY_ENV_KEYS.iter().any(|key| env_is_set(key)) {
        return;
    }
    for key in NO_PROXY_ENV_KEYS {
        if env_is_set(key) {
            command.env(key, "");
        }
    }
}

/// Names only — never values, since a proxy URL may embed credentials.
fn proxy_env_summary() -> String {
    let keys: Vec<&str> = PROXY_ENV_KEYS
        .iter()
        .chain(NO_PROXY_ENV_KEYS.iter())
        .filter(|key| env_is_set(key))
        .copied()
        .collect();
    if keys.is_empty() {
        "none".to_string()
    } else {
        keys.join(",")
    }
}

/// Run a command with a hard deadline, capturing both streams.
///
/// `Command::output()` waits forever, so a wedged network leaves the invoke
/// pending and the UI silent. This deadline turns that into a reported error.
/// stdin is null rather than inherited: a GUI process has no usable console
/// handle to hand down, and `Command::output()` behaves the same way.
fn run_command_with_timeout(
    command: &mut Command,
    timeout: Duration,
) -> Result<std::process::Output, String> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|err| format!("Failed to start yt-dlp: {err}"))?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| "Failed to read yt-dlp output".to_string())?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| "Failed to read yt-dlp error output".to_string())?;

    let stdout_reader = thread::spawn(move || {
        let mut buffer = Vec::new();
        let _ = stdout.read_to_end(&mut buffer);
        buffer
    });
    let stderr_reader = thread::spawn(move || {
        let mut buffer = Vec::new();
        let _ = stderr.read_to_end(&mut buffer);
        buffer
    });

    let deadline = Instant::now() + timeout;
    let status: ExitStatus = loop {
        if let Some(status) = child.try_wait().map_err(|err| err.to_string())? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            let _ = stdout_reader.join();
            let _ = stderr_reader.join();
            return Err(format!(
                "yt-dlp gave no answer within {}s; check that the network or proxy the system reports is reachable",
                timeout.as_secs()
            ));
        }
        thread::sleep(Duration::from_millis(80));
    };

    let stdout = stdout_reader.join().unwrap_or_default();
    let stderr = stderr_reader.join().unwrap_or_default();
    Ok(std::process::Output {
        status,
        stdout,
        stderr,
    })
}

fn fetch_youtube_metadata(
    output_root: &Path,
    task_id: &str,
    url: &str,
) -> Result<YoutubeVideoMetadata, String> {
    let mut command = build_yt_dlp_command(output_root);
    command
        .arg("--verbose")
        .arg("--no-playlist")
        .arg("--skip-download")
        .arg("--dump-single-json")
        .arg(url);

    // Logged before the spawn, so a call that never returns still leaves a
    // record of what the process saw.
    log_ytdlp(
        output_root,
        task_id,
        &format!(
            "metadata start | cwd={} | env_proxy_keys={}",
            output_root.display(),
            proxy_env_summary()
        ),
    );

    let output = match run_command_with_timeout(&mut command, METADATA_TIMEOUT) {
        Ok(output) => output,
        Err(err) => {
            log_ytdlp(output_root, task_id, &format!("metadata error | {err}"));
            return Err(format!("Failed to parse YouTube info: {err}"));
        }
    };
    log_ytdlp(output_root, task_id, &debug_blob(&collect_debug_lines(&output.stderr)));

    if !output.status.success() {
        let detail = tail_lines(&String::from_utf8_lossy(&output.stderr), 4);
        let message = if detail.is_empty() {
            "Failed to parse YouTube info".to_string()
        } else {
            format!("Failed to parse YouTube info: {detail}")
        };
        log_ytdlp(output_root, task_id, &format!("metadata failed | {message}"));
        return Err(message);
    }
    let mut metadata = serde_json::from_slice::<YoutubeVideoMetadata>(&output.stdout)
        .map_err(|err| format!("Failed to parse YouTube info: {err}"))?;
    metadata.title = metadata.title.trim().to_string();
    if metadata.title.is_empty() {
        return Err("Failed to parse YouTube info: video title not found".to_string());
    }
    log_ytdlp(
        output_root,
        task_id,
        &format!("metadata ok | title={}", metadata.title),
    );
    Ok(metadata)
}

/// Append one line to the yt-dlp log in the output root.
///
/// Best effort on purpose: a read-only output directory must never turn a
/// download into a failure.
fn log_ytdlp(output_root: &Path, task_id: &str, message: &str) {
    let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(output_root.join(YTDLP_LOG_FILE))
    else {
        return;
    };
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
    let _ = writeln!(file, "[{now}] [{task_id}] {message}");
}

/// Keep the verbose lines that explain the network path: which proxy yt-dlp
/// resolved, and which config files fed into that decision.
fn collect_ytdlp_debug(lines: &mut Vec<String>, line: &str) {
    let trimmed = line.trim();
    if trimmed.is_empty() || !is_ytdlp_debug_line(trimmed) {
        return;
    }
    if lines.iter().any(|existing| existing == trimmed) {
        return;
    }
    lines.push(trimmed.to_string());
    while lines.len() > DEBUG_LINE_LIMIT {
        lines.remove(0);
    }
}

fn collect_debug_lines(stderr: &[u8]) -> Vec<String> {
    let mut lines = Vec::new();
    for line in String::from_utf8_lossy(stderr).lines() {
        collect_ytdlp_debug(&mut lines, line);
    }
    lines
}

fn is_ytdlp_debug_line(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    lower.contains("proxy map")
        || lower.contains("config:")
        || lower.contains("--proxy")
        || lower.contains("ignoring config")
}

fn debug_blob(lines: &[String]) -> String {
    if lines.is_empty() {
        "yt-dlp debug: none".to_string()
    } else {
        format!("yt-dlp debug: {}", lines.join(" | "))
    }
}

fn stalled_message(recent: &VecDeque<String>) -> String {
    let detail = recent
        .iter()
        .rev()
        .find(|line| !line.starts_with("[download]"))
        .or_else(|| recent.back())
        .cloned()
        .unwrap_or_default();
    let secs = DOWNLOAD_STALL_TIMEOUT.as_secs();
    if detail.is_empty() {
        format!("YouTube download stalled: no output for {secs}s")
    } else {
        clamp_message(&format!(
            "YouTube download stalled after {secs}s without output ({detail})"
        ))
    }
}

fn youtube_output_dir_for_title(
    output_root: &Path,
    title: &str,
    task_id: &str,
) -> Result<PathBuf, String> {
    let safe_title = sanitize_filename_component(title);
    if safe_title.is_empty() {
        return Err("Failed to parse YouTube info: video title not found".to_string());
    }
    let safe_task_id = sanitize_filename_component(task_id);
    if safe_task_id.is_empty() {
        return Err("taskId is invalid".to_string());
    }
    Ok(output_root.join(format!("{safe_title}_{safe_task_id}")))
}

fn metadata_size_bytes(metadata: &YoutubeVideoMetadata) -> Option<u64> {
    metadata
        .filesize
        .or(metadata.filesize_approx)
        .filter(|value| *value > 0)
}

fn format_size_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit_index = 0usize;
    while value >= 1024.0 && unit_index + 1 < UNITS.len() {
        value /= 1024.0;
        unit_index += 1;
    }
    if unit_index == 0 {
        format!("{bytes}B")
    } else {
        format!("{value:.2}{}", UNITS[unit_index])
    }
}

fn remove_cancel_flag(task_id: &str) {
    let _ = cancel_flags().remove(task_id);
}

fn resolve_tool(program: &str) -> PathBuf {
    let bundled = resolve_bundled_or_path(program);
    if bundled.is_file() {
        return bundled;
    }
    let exe_name = format!("{program}{}", std::env::consts::EXE_SUFFIX);
    let dev_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("bin")
        .join(exe_name);
    if dev_path.is_file() {
        return dev_path;
    }
    bundled
}

fn resolve_ffmpeg_dir() -> Option<PathBuf> {
    let ffmpeg = resolve_tool("ffmpeg");
    if ffmpeg.is_file() {
        return ffmpeg.parent().map(Path::to_path_buf);
    }
    None
}

/// Read a child stream line by line without ever stopping early.
///
/// `BufRead::lines()` yields an error for any byte sequence that is not valid
/// UTF-8, and `map_while(Result::ok)` ends the loop right there — which also
/// drops the read handle, so the child's next write hits a broken pipe and the
/// process dies without printing anything. yt-dlp writes the output path in
/// the Windows code page, so any non-ASCII task folder used to kill the
/// download exactly this way: exit 1 after ~1s, zero output, no explanation.
fn spawn_line_reader(
    stream: impl std::io::Read + Send + 'static,
    tx: mpsc::Sender<String>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut reader = BufReader::new(stream);
        let mut buffer = Vec::new();
        loop {
            buffer.clear();
            match reader.read_until(b'\n', &mut buffer) {
                Ok(0) => return,
                Ok(_) => {
                    let line = String::from_utf8_lossy(&buffer);
                    let line = line.trim_end_matches(['\r', '\n']);
                    if tx.send(line.to_string()).is_err() {
                        return;
                    }
                }
                Err(_) => return,
            }
        }
    })
}

/// Mutable state threaded through yt-dlp stdout/stderr line handling.
struct YtdlpLineState {
    final_path: Option<PathBuf>,
    recent_lines: VecDeque<String>,
    /// Last time a download-percent progress event was emitted (throttle).
    last_download_emit: Instant,
}

fn handle_ytdlp_line(
    app: &tauri::AppHandle,
    _task_id: &str,
    output_dir: &Path,
    line: &str,
    snapshot: &mut YoutubeDownloadProgressResponse,
    state: &mut YtdlpLineState,
) {
    let line = line.trim();
    if line.is_empty() {
        return;
    }
    remember_line(&mut state.recent_lines, line);

    if let Some(path) = extract_path_from_line(line, output_dir)
        && path.extension().is_some() {
            state.final_path = Some(path.clone());
            if let Some(title) = title_from_download_path(&path) {
                snapshot.title = title;
            }
        }

    if line.contains("[Merger]") {
        snapshot.phase = "merging".to_string();
        snapshot.message = "merging".to_string();
        set_progress(app, snapshot.clone());
        return;
    }

    if let Some(percent) = parse_download_percent(line) {
        snapshot.phase = "downloading".to_string();
        snapshot.progress_percent = percent;
        snapshot.total_size = parse_after_marker(line, " of ", &[" at ", " ETA "])
            .unwrap_or_else(|| snapshot.total_size.clone());
        snapshot.downloaded_size = format!("{percent:.1}%");
        snapshot.speed =
            parse_after_marker(line, " at ", &[" ETA "]).unwrap_or_else(|| snapshot.speed.clone());
        snapshot.eta =
            parse_after_marker(line, " ETA ", &[]).unwrap_or_else(|| snapshot.eta.clone());
        snapshot.message = "downloading".to_string();
        // Throttle the per-line progress events (yt-dlp prints many per
        // second); completion (100%) always goes through, and the snapshot
        // state above is updated regardless so nothing is lost.
        if percent >= 100.0 || state.last_download_emit.elapsed() >= Duration::from_millis(300) {
            state.last_download_emit = Instant::now();
            set_progress(app, snapshot.clone());
        }
        return;
    }

    if line.contains("[download] Destination:") {
        snapshot.phase = "downloading".to_string();
        snapshot.message = "downloading".to_string();
        set_progress(app, snapshot.clone());
    } else if line.contains("has already been downloaded") {
        snapshot.phase = "downloading".to_string();
        snapshot.progress_percent = 100.0;
        snapshot.message = "finalizing".to_string();
        set_progress(app, snapshot.clone());
    }
}

fn extract_path_from_line(line: &str, output_dir: &Path) -> Option<PathBuf> {
    // `--verbose` also prints URLs (`[debug] Invoking downloader on "https://…"`);
    // a quoted-tail read would happily adopt one as a file path. No real
    // destination path contains a scheme.
    if line.contains("://") {
        return None;
    }
    if let Some(value) = line.split("Destination:").nth(1) {
        return Some(PathBuf::from(value.trim().trim_matches('"')));
    }
    if let Some(value) = quoted_tail(line) {
        return Some(PathBuf::from(value));
    }
    let path = PathBuf::from(line.trim_matches('"'));
    if path.is_absolute() && path.extension().is_some() {
        return Some(path);
    }
    let joined = output_dir.join(&path);
    if joined.extension().is_some() {
        return Some(joined);
    }
    None
}

fn quoted_tail(line: &str) -> Option<String> {
    let end = line.rfind('"')?;
    let start = line[..end].rfind('"')?;
    if start + 1 >= end {
        return None;
    }
    Some(line[start + 1..end].to_string())
}

fn parse_download_percent(line: &str) -> Option<f64> {
    let percent_index = line.find('%')?;
    let before = &line[..percent_index];
    let token = before.split_whitespace().last()?.trim_start_matches('~');
    token
        .parse::<f64>()
        .ok()
        .map(|value| value.clamp(0.0, 100.0))
}

fn parse_after_marker(line: &str, marker: &str, terminators: &[&str]) -> Option<String> {
    let start = line.find(marker)? + marker.len();
    let tail = &line[start..];
    let end = terminators
        .iter()
        .filter_map(|term| tail.find(term))
        .min()
        .unwrap_or(tail.len());
    let value = tail[..end].trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn remember_line(lines: &mut VecDeque<String>, line: &str) {
    lines.push_back(line.to_string());
    while lines.len() > 20 {
        lines.pop_front();
    }
}

fn recent_output_message(lines: &VecDeque<String>, fallback: &str) -> String {
    let detail = lines
        .iter()
        .rev()
        .find(|line| !line.starts_with("[download]"))
        .or_else(|| lines.back())
        .cloned()
        .unwrap_or_default();
    if detail.is_empty() {
        fallback.to_string()
    } else {
        clamp_message(&format!("{fallback}: {detail}"))
    }
}

/// Last few meaningful lines of a stream, joined for display.
///
/// `--verbose` makes raw stderr far too large to surface in a toast or in the
/// status message, so failures report a tail instead of everything.
fn tail_lines(text: &str, limit: usize) -> String {
    let mut lines: VecDeque<&str> = VecDeque::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        lines.push_back(trimmed);
        while lines.len() > limit {
            lines.pop_front();
        }
    }
    clamp_message(&lines.into_iter().collect::<Vec<_>>().join(" | "))
}

fn clamp_message(text: &str) -> String {
    const MESSAGE_LIMIT: usize = 400;
    let trimmed = text.trim();
    if trimmed.chars().count() <= MESSAGE_LIMIT {
        return trimmed.to_string();
    }
    let head: String = trimmed.chars().take(MESSAGE_LIMIT).collect();
    format!("{head}…")
}

fn find_downloaded_media(output_dir: &Path) -> Option<PathBuf> {
    let mut candidates = std::fs::read_dir(output_dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && is_supported_media_path(path))
        .filter_map(|path| {
            let modified = std::fs::metadata(&path)
                .and_then(|meta| meta.modified())
                .ok()?;
            Some((modified, path))
        })
        .collect::<Vec<_>>();
    candidates.sort_by_key(|(modified, _)| *modified);
    candidates.pop().map(|(_, path)| path)
}

fn is_supported_media_path(path: &Path) -> bool {
    let Some(ext) = path.extension().and_then(|value| value.to_str()) else {
        return false;
    };
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "mp4" | "webm" | "mkv" | "mov" | "mp3" | "m4a" | "wav" | "flac" | "aac" | "opus"
    )
}

fn media_kind_for_path(path: &Path) -> &'static str {
    let ext = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if matches!(
        ext.as_str(),
        "mp3" | "m4a" | "wav" | "flac" | "aac" | "opus"
    ) {
        "audio"
    } else {
        "video"
    }
}

fn media_name(path: &Path) -> String {
    path.file_stem()
        .and_then(|value| value.to_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("YouTube download")
        .to_string()
}

/// Title taken from a `Destination:` line, ignored when it cannot be trusted.
///
/// A path that was decoded from a code page we do not read contains U+FFFD, and
/// such a title must never overwrite the real one the metadata call already
/// delivered — the row would flip from proper Japanese to replacement
/// characters the moment the download starts.
fn title_from_download_path(path: &Path) -> Option<String> {
    let title = media_name(path);
    if title.is_empty() || title.contains('\u{FFFD}') {
        None
    } else {
        Some(title)
    }
}

fn new_progress(task_id: &str, phase: &str, message: &str) -> YoutubeDownloadProgressResponse {
    YoutubeDownloadProgressResponse {
        task_id: task_id.to_string(),
        phase: phase.to_string(),
        progress_percent: 0.0,
        title: String::new(),
        speed: String::new(),
        total_size: String::new(),
        downloaded_size: String::new(),
        eta: String::new(),
        message: message.to_string(),
    }
}

fn set_progress(app: &tauri::AppHandle, snapshot: YoutubeDownloadProgressResponse) {
    progress_snapshots().insert(snapshot.task_id.clone(), snapshot.clone());
    let _ = app.emit("youtube-download-progress", snapshot);
}

fn default_task_id() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    format!("yt-{millis}")
}

fn command_output_message(label: &str, output: &std::process::Output) -> String {
    let text = command_output_text(output);
    if text.is_empty() {
        label.to_string()
    } else {
        format!("{label}: {text}")
    }
}

fn command_output_text(output: &std::process::Output) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    match (stdout.is_empty(), stderr.is_empty()) {
        (false, false) => format!("{stdout}\n{stderr}"),
        (false, true) => stdout,
        (true, false) => stderr,
        (true, true) => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::path::Path;
    use std::sync::mpsc;

    use super::{
        DEBUG_LINE_LIMIT, clamp_message, collect_ytdlp_debug, extract_path_from_line,
        parse_after_marker, parse_download_percent, spawn_line_reader, stalled_message, tail_lines,
        title_from_download_path,
    };

    const YOUTUBE_RS: &str = include_str!("youtube.rs");

    #[test]
    fn parses_ytdlp_progress_line() {
        let line = "[download]  42.5% of 12.34MiB at 1.23MiB/s ETA 00:05";
        assert_eq!(parse_download_percent(line), Some(42.5));
        assert_eq!(
            parse_after_marker(line, " of ", &[" at ", " ETA "]).as_deref(),
            Some("12.34MiB")
        );
        assert_eq!(
            parse_after_marker(line, " at ", &[" ETA "]).as_deref(),
            Some("1.23MiB/s")
        );
        assert_eq!(
            parse_after_marker(line, " ETA ", &[]).as_deref(),
            Some("00:05")
        );
    }

    #[test]
    fn download_template_uses_title_without_video_id() {
        let old_id_suffix = ["[", "%(id)s", "]"].join("");
        assert!(YOUTUBE_RS.contains(".arg(\"%(title).200B.%(ext)s\")"));
        assert!(!YOUTUBE_RS.contains(&old_id_suffix));
    }

    #[test]
    fn download_command_forces_progress_when_printing_final_path() {
        assert!(YOUTUBE_RS.contains(".arg(\"--print\")"));
        assert!(YOUTUBE_RS.contains(".arg(\"--progress\")"));
    }

    #[test]
    fn progress_line_is_not_treated_as_downloaded_path() {
        let line = "[download]  42.5% of 12.34MiB at 1.23MiB/s ETA 00:05";
        assert!(extract_path_from_line(line, Path::new("C:\\output")).is_none());
    }

    #[test]
    fn verbose_url_lines_are_not_mistaken_for_paths() {
        let line = "[debug] Invoking downloader on \"https://rr3---sn-x.googlevideo.com/videoplayback?mime=video%2Fmp4&clen=123\"";

        assert!(extract_path_from_line(line, Path::new("C:\\output")).is_none());
        assert!(extract_path_from_line(
            "[download] Destination: C:\\output\\clip.f137.mp4",
            Path::new("C:\\output")
        )
        .is_some());
    }

    #[test]
    fn tail_lines_keeps_the_last_meaningful_lines() {
        let text = "[debug] noise\n\nERROR: boom\nERROR: last\n";

        assert_eq!(tail_lines(text, 2), "ERROR: boom | ERROR: last");
    }

    #[test]
    fn line_reader_survives_bytes_that_are_not_utf8() {
        // yt-dlp writes the Windows code page, so a task folder holding a CJK
        // title produces GBK bytes on the first stderr line. The reader used to
        // stop there, which closed the pipe and killed the child mid-download
        // with no output at all.
        let payload = b"[debug] Destination: \x81\x40\x82\xa0\r\n[download]  50.0%\n".to_vec();
        let (tx, rx) = mpsc::channel();
        let handle = spawn_line_reader(std::io::Cursor::new(payload), tx);
        let _ = handle.join();
        let lines: Vec<String> = rx.try_iter().collect();

        assert_eq!(lines.len(), 2, "both lines must survive: {lines:?}");
        assert!(lines[0].starts_with("[debug] Destination:"));
        assert!(!lines[0].ends_with('\r'));
        assert!(lines[1].contains("50.0%"));
    }

    #[test]
    fn yt_dlp_gets_utf8_output_and_no_stray_no_proxy() {
        assert!(YOUTUBE_RS.contains("PYTHONIOENCODING"));
        assert!(YOUTUBE_RS.contains("clear_stray_no_proxy(&mut command)"));
        assert!(YOUTUBE_RS.contains("env_proxy_keys="));
    }

    #[test]
    fn yt_dlp_output_encoding_is_pinned_to_utf8() {
        // Without this, a CJK title comes back as GBK and the row shows
        // replacement characters once the download progress takes over.
        assert!(YOUTUBE_RS.contains(".arg(\"--encoding\")"));
        assert!(YOUTUBE_RS.contains("YTDLP_OUTPUT_ENCODING"));
    }

    #[test]
    fn a_mojibake_title_never_replaces_a_good_one() {
        let good = Path::new("C:\\out\\【HOSTCALL #012】.f137.mp4");
        let broken = Path::new("C:\\out\\\u{FFFD}\u{FFFD}d3\u{FFFD}.f137.mp4");

        // The `.f137` suffix stays: the frontend's normalizeTitle strips it.
        assert_eq!(
            title_from_download_path(good).as_deref(),
            Some("【HOSTCALL #012】.f137")
        );
        assert_eq!(title_from_download_path(broken), None);
    }

    #[test]
    fn clamp_message_caps_verbose_output() {
        let long = "x".repeat(1000);

        assert_eq!(clamp_message(&long).chars().count(), 401);
        assert_eq!(clamp_message("  short  "), "short");
    }

    #[test]
    fn youtube_output_dir_uses_parsed_title_and_task_id() {
        let dir =
            super::youtube_output_dir_for_title(Path::new("C:\\output"), "Video Title", "yt-123")
                .expect("output dir");

        assert_eq!(dir, Path::new("C:\\output").join("Video Title_yt-123"));
    }

    #[test]
    fn youtube_output_dir_rejects_empty_parsed_title() {
        let result = super::youtube_output_dir_for_title(Path::new("C:\\output"), "...", "yt-123");

        assert!(result.is_err());
    }

    #[test]
    fn yt_dlp_is_spawned_with_a_pinned_working_directory() {
        assert!(
            YOUTUBE_RS.contains("command.current_dir(working_dir)"),
            "yt-dlp must not inherit the process cwd: a yt-dlp.conf there can override the proxy"
        );
    }

    #[test]
    fn yt_dlp_is_never_given_an_invented_proxy() {
        // Proxy handling belongs to yt-dlp / the system. Passing our own guess
        // would break clients that expose no local port at all. The needle is
        // assembled at runtime, otherwise this assertion would match its own
        // source text inside YOUTUBE_RS.
        let proxy_flag = [".arg(\"", "--proxy", "\")"].concat();
        assert!(!YOUTUBE_RS.contains(&proxy_flag));
        assert!(YOUTUBE_RS.contains("--verbose"));
    }

    #[test]
    fn download_prefers_h264_and_retries_more() {
        assert!(YOUTUBE_RS.contains("\"bestvideo[vcodec*=avc1]"));
        assert!(YOUTUBE_RS.contains(".arg(\"-f\")"));
        assert!(YOUTUBE_RS.contains(".arg(\"--fragment-retries\")"));
    }

    #[test]
    fn metadata_call_and_download_loop_are_bounded() {
        assert!(YOUTUBE_RS.contains("run_command_with_timeout(&mut command, METADATA_TIMEOUT)"));
        assert!(YOUTUBE_RS.contains("last_output_at.elapsed() >= DOWNLOAD_STALL_TIMEOUT"));
    }

    #[test]
    fn debug_capture_keeps_proxy_and_config_lines_only() {
        let mut lines = Vec::new();
        collect_ytdlp_debug(
            &mut lines,
            "[debug] Proxy map: {'http': 'http://127.0.0.1:7897'}",
        );
        collect_ytdlp_debug(
            &mut lines,
            "[debug] User config \"C:\\Users\\x\\yt-dlp.conf\": ['--proxy', 'http://127.0.0.1:1']",
        );
        collect_ytdlp_debug(&mut lines, "[download]  12.3% of 1.12GiB at 800KiB/s ETA 19:00");
        collect_ytdlp_debug(&mut lines, "[debug] Request Handlers: urllib, requests");

        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("Proxy map"));
        assert!(lines[1].contains("User config"));
    }

    #[test]
    fn debug_capture_dedupes_and_caps_its_buffer() {
        let mut lines = Vec::new();
        for index in 0..(DEBUG_LINE_LIMIT + 5) {
            collect_ytdlp_debug(&mut lines, &format!("[debug] config: entry {index}"));
        }
        collect_ytdlp_debug(&mut lines, "[debug] config: entry 0");

        assert_eq!(lines.len(), DEBUG_LINE_LIMIT);
        assert!(lines.last().is_some_and(|line| line.contains("entry 0")));
    }

    #[test]
    fn stall_message_prefers_the_last_non_progress_line() {
        let mut recent = VecDeque::new();
        recent.push_back("[youtube] Extracting URL: https://youtu.be/x".to_string());
        recent.push_back("[download]  12.3% of 1.12GiB at 800KiB/s ETA 19:00".to_string());

        let message = stalled_message(&recent);

        assert!(message.contains("stalled"));
        assert!(message.contains("Extracting URL"));
    }

    #[test]
    fn stall_message_still_reads_when_only_progress_was_seen() {
        let mut recent = VecDeque::new();
        recent.push_back("[download]  12.3% of 1.12GiB at 800KiB/s ETA 19:00".to_string());

        let message = stalled_message(&recent);

        assert!(message.contains("stalled"));
        assert!(message.contains("12.3%"));
    }
}
