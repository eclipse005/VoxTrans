use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::Emitter;

use super::binary::{configure_background_command, resolve_bundled_or_path};
use super::output::resolve_output_dir;
use super::task_path::sanitize_filename_component;

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

static YOUTUBE_PROGRESS: OnceLock<Mutex<HashMap<String, YoutubeDownloadProgressResponse>>> =
    OnceLock::new();
static YOUTUBE_CANCEL_FLAGS: OnceLock<Mutex<HashMap<String, Arc<AtomicBool>>>> = OnceLock::new();

fn progress_snapshots() -> &'static Mutex<HashMap<String, YoutubeDownloadProgressResponse>> {
    YOUTUBE_PROGRESS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn cancel_flags() -> &'static Mutex<HashMap<String, Arc<AtomicBool>>> {
    YOUTUBE_CANCEL_FLAGS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn download_youtube_to_task(
    app: &tauri::AppHandle,
    url: String,
    task_id: Option<String>,
) -> Result<DownloadYoutubeTaskResponse, String> {
    let url = url.trim().to_string();
    if url.is_empty() {
        return Err("YouTube 链接不能为空".to_string());
    }
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err("请输入有效的 YouTube 链接".to_string());
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
    cancel_flags()
        .lock()
        .map_err(|_| "youtube cancel state lock poisoned".to_string())?
        .insert(task_id.clone(), cancel.clone());

    let mut snapshot = new_progress(&task_id, "starting", "解析视频信息");
    set_progress(app, snapshot.clone());

    let metadata = match fetch_youtube_metadata(&url) {
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
        snapshot.message = "YouTube 下载已取消".to_string();
        set_progress(app, snapshot);
        remove_cancel_flag(&task_id);
        return Err("YouTube 下载已取消".to_string());
    }
    let output_dir =
        match youtube_output_dir_for_title(&resolve_output_dir(), &metadata.title, &task_id) {
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
        let message = format!("创建下载目录失败: {err}");
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
    snapshot.message = "准备下载".to_string();
    set_progress(app, snapshot.clone());

    let result = run_ytdlp_download(app, &task_id, &url, &output_dir, &cancel, &mut snapshot);
    remove_cancel_flag(&task_id);
    result
}

pub fn get_download_progress(task_id: &str) -> Result<YoutubeDownloadProgressResponse, String> {
    let task_id = task_id.trim();
    if task_id.is_empty() {
        return Err("taskId is required".to_string());
    }
    Ok(progress_snapshots()
        .lock()
        .map_err(|_| "youtube progress state lock poisoned".to_string())?
        .get(task_id)
        .cloned()
        .unwrap_or_else(|| new_progress(task_id, "idle", "")))
}

pub fn list_download_progress() -> Result<Vec<YoutubeDownloadProgressResponse>, String> {
    Ok(progress_snapshots()
        .lock()
        .map_err(|_| "youtube progress state lock poisoned".to_string())?
        .values()
        .cloned()
        .collect())
}

pub fn request_cancel(task_id: &str) -> bool {
    cancel_flags()
        .lock()
        .ok()
        .and_then(|flags| flags.get(task_id).cloned())
        .map(|flag| {
            flag.store(true, Ordering::SeqCst);
            true
        })
        .unwrap_or(false)
}

pub fn get_yt_dlp_version() -> Result<String, String> {
    let output = build_yt_dlp_command()
        .arg("--version")
        .output()
        .map_err(|err| format!("运行 yt-dlp 失败: {err}"))?;
    if !output.status.success() {
        return Err(command_output_message("yt-dlp 版本检测失败", &output));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub fn update_yt_dlp() -> Result<UpdateYtDlpResponse, String> {
    let from_version = get_yt_dlp_version().unwrap_or_default();
    let output = build_yt_dlp_command()
        .arg("-U")
        .output()
        .map_err(|err| format!("运行 yt-dlp 更新失败: {err}"))?;
    let text = command_output_text(&output);
    if !output.status.success() {
        return Err(format!("yt-dlp 更新失败: {text}"));
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
    output_dir: &Path,
    cancel: &AtomicBool,
    snapshot: &mut YoutubeDownloadProgressResponse,
) -> Result<DownloadYoutubeTaskResponse, String> {
    let mut command = build_yt_dlp_command();
    command
        .arg("--newline")
        .arg("--progress")
        .arg("--no-playlist")
        .arg("--merge-output-format")
        .arg("mp4")
        .arg("--print")
        .arg("after_move:filepath")
        .arg("-P")
        .arg(output_dir)
        .arg("-o")
        .arg("%(title).200B.%(ext)s")
        .arg(url)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(ffmpeg_dir) = resolve_ffmpeg_dir() {
        command.arg("--ffmpeg-location").arg(ffmpeg_dir);
    }

    let mut child = command
        .spawn()
        .map_err(|err| format!("启动 yt-dlp 失败: {err}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "无法读取 yt-dlp 输出".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "无法读取 yt-dlp 错误输出".to_string())?;

    let (tx, rx) = mpsc::channel::<String>();
    let stdout_reader = spawn_line_reader(stdout, tx.clone());
    let stderr_reader = spawn_line_reader(stderr, tx);
    let mut recent_lines = VecDeque::<String>::new();
    let mut final_path: Option<PathBuf> = None;

    loop {
        while let Ok(line) = rx.try_recv() {
            handle_ytdlp_line(
                app,
                task_id,
                output_dir,
                &line,
                snapshot,
                &mut final_path,
                &mut recent_lines,
            );
        }

        if cancel.load(Ordering::SeqCst) {
            let _ = child.kill();
            let _ = child.wait();
            snapshot.phase = "cancelled".to_string();
            snapshot.message = "YouTube 下载已取消".to_string();
            set_progress(app, snapshot.clone());
            return Err("YouTube 下载已取消".to_string());
        }

        if let Some(status) = child.try_wait().map_err(|err| err.to_string())? {
            let _ = stdout_reader.join();
            let _ = stderr_reader.join();
            while let Ok(line) = rx.try_recv() {
                handle_ytdlp_line(
                    app,
                    task_id,
                    output_dir,
                    &line,
                    snapshot,
                    &mut final_path,
                    &mut recent_lines,
                );
            }

            if !status.success() {
                let message = recent_output_message(&recent_lines, "YouTube 下载失败");
                snapshot.phase = "failed".to_string();
                snapshot.message = message.clone();
                set_progress(app, snapshot.clone());
                return Err(message);
            }
            break;
        }

        thread::sleep(Duration::from_millis(100));
    }

    let downloaded_path = final_path
        .filter(|path| path.is_file())
        .or_else(|| find_downloaded_media(output_dir))
        .ok_or_else(|| "YouTube 下载完成但未找到媒体文件".to_string())?;
    let metadata =
        std::fs::metadata(&downloaded_path).map_err(|err| format!("读取下载文件失败: {err}"))?;
    let name = media_name(&downloaded_path);
    snapshot.phase = "completed".to_string();
    snapshot.progress_percent = 100.0;
    snapshot.title = name.clone();
    snapshot.message = "YouTube 下载完成".to_string();
    set_progress(app, snapshot.clone());

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

fn build_yt_dlp_command() -> Command {
    let mut command = Command::new(resolve_tool("yt-dlp"));
    configure_background_command(&mut command);
    command
}

fn fetch_youtube_metadata(url: &str) -> Result<YoutubeVideoMetadata, String> {
    let output = build_yt_dlp_command()
        .arg("--no-playlist")
        .arg("--skip-download")
        .arg("--dump-single-json")
        .arg(url)
        .output()
        .map_err(|err| format!("解析 YouTube 信息失败: {err}"))?;
    if !output.status.success() {
        return Err(command_output_message("解析 YouTube 信息失败", &output));
    }
    let mut metadata = serde_json::from_slice::<YoutubeVideoMetadata>(&output.stdout)
        .map_err(|err| format!("解析 YouTube 信息失败: {err}"))?;
    metadata.title = metadata.title.trim().to_string();
    if metadata.title.is_empty() {
        return Err("解析 YouTube 信息失败: 未获取到视频标题".to_string());
    }
    Ok(metadata)
}

fn youtube_output_dir_for_title(
    output_root: &Path,
    title: &str,
    task_id: &str,
) -> Result<PathBuf, String> {
    let safe_title = sanitize_filename_component(title);
    if safe_title.is_empty() {
        return Err("解析 YouTube 信息失败: 未获取到视频标题".to_string());
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
    let _ = cancel_flags().lock().map(|mut flags| flags.remove(task_id));
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

fn spawn_line_reader(
    stream: impl std::io::Read + Send + 'static,
    tx: mpsc::Sender<String>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        for line in BufReader::new(stream).lines().map_while(Result::ok) {
            let _ = tx.send(line);
        }
    })
}

fn handle_ytdlp_line(
    app: &tauri::AppHandle,
    task_id: &str,
    output_dir: &Path,
    line: &str,
    snapshot: &mut YoutubeDownloadProgressResponse,
    final_path: &mut Option<PathBuf>,
    recent_lines: &mut VecDeque<String>,
) {
    let line = line.trim();
    if line.is_empty() {
        return;
    }
    remember_line(recent_lines, line);

    if let Some(path) = extract_path_from_line(line, output_dir) {
        if path.extension().is_some() {
            *final_path = Some(path.clone());
            let title = media_name(&path);
            if !title.is_empty() {
                snapshot.title = title;
            }
        }
    }

    if line.contains("[Merger]") {
        snapshot.phase = "merging".to_string();
        snapshot.message = "正在合并音视频".to_string();
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
        snapshot.message = "YouTube 下载中".to_string();
        set_progress(app, snapshot.clone());
        return;
    }

    if line.contains("[download] Destination:") {
        snapshot.phase = "downloading".to_string();
        snapshot.message = "YouTube 下载中".to_string();
        set_progress(app, snapshot.clone());
    } else if line.contains("has already been downloaded") {
        snapshot.phase = "downloading".to_string();
        snapshot.progress_percent = 100.0;
        snapshot.message = "文件已下载，正在整理".to_string();
        set_progress(app, snapshot.clone());
    }

    let _ = task_id;
}

fn extract_path_from_line(line: &str, output_dir: &Path) -> Option<PathBuf> {
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
        format!("{fallback}: {detail}")
    }
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
        .unwrap_or("YouTube 下载")
        .to_string()
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
    if let Ok(mut progress) = progress_snapshots().lock() {
        progress.insert(snapshot.task_id.clone(), snapshot.clone());
    }
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
    use std::path::Path;

    use super::{extract_path_from_line, parse_after_marker, parse_download_percent};

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
}
