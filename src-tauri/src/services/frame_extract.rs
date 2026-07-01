//! Video frame extraction for vision-assisted translation.
//!
//! Samples JPEG frames at fixed intervals within each translation batch's
//! time range and returns base64 data URLs suitable for OpenAI-compatible
//! `image_url` content parts.
//!
//! Design notes (informed by the owarai-grillmaster reference project):
//! - Frame sampling is by **fixed time interval**, not per subtitle segment.
//!   A 30s interval yields ~2 frames per typical batch; per-segment sampling
//!   would produce thousands of frames for a 1h video and blow the token
//!   budget.
//! - Frame files are cached on disk, keyed by `frame_{timestamp:010.3}_orig.jpg`.
//!   Resuming a partially-translated task reuses cached frames for free.
//! - Skip the first `INTRO_SKIP_SECONDS` (TV station intro/logo) and leave
//!   `LAST_FRAME_OFFSET` seconds at the end to avoid fast-seek landing on the
//!   final GOP keyframe with no decodable frame after it.
//! - Compression uses mjpeg + qscale=2 (visually lossless). Frames are kept
//!   at the source resolution — text-heavy content (screen recordings,
//!   charts, slides) needs full resolution to remain legible to the vision
//!   model. Token cost is higher than a downscaled variant, but accuracy on
//!   text-heavy media is the priority.

use std::collections::HashSet;
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use base64::Engine;
use serde_json::json;

use crate::services::binary::{configure_background_command, resolve_bundled_or_path};
use crate::services::task_log::{TaskLogger, event};
use crate::services::task_path::task_artifacts_dir;

/// Monotonic counter used to give each frame-extraction temp file a unique
/// name, so two workers sampling the same global-grid timestamp never share a
/// `.part` path. Combined with the PID it is unique across the process.
static TEMP_FILE_SEQ: AtomicU64 = AtomicU64::new(0);

/// One sampled video frame: its cache filename (for logging/diagnostics) plus
/// the base64 data URL sent to the LLM.
#[derive(Debug, Clone)]
pub struct Frame {
    /// Cache filename, e.g. `frame_0030.000_orig.jpg`. Stable across runs so
    /// it can be used to identify which frames a batch used in the logs.
    pub filename: String,
    /// `data:image/jpeg;base64,...` sent as an `image_url` content part.
    pub data_url: String,
}

/// Sampling interval. A 30s interval gives ~2 frames per typical batch.
/// Informed by owarai-grillmaster's `chunk_frame_interval_seconds`.
const INTERVAL_SECONDS: f64 = 30.0;

/// Hard cap on frames per batch. Bounds token cost and request payload size
/// regardless of batch length or interval changes. A pathological long batch
/// can otherwise produce dozens of full-resolution frames that blow the
/// context window — especially on non-OpenAI providers that ignore any
/// resolution hint and charge full detail tokens.
const MAX_FRAMES_PER_BATCH: usize = 4;

/// Skip the first few seconds (TV station intro / logo). Only applied to the
/// first batch's first frame.
const INTRO_SKIP_SECONDS: f64 = 3.0;

/// Avoid fast-seek landing on the final GOP keyframe with no decodable frame
/// after it. Keep 1.5s of margin at the end.
const LAST_FRAME_OFFSET: f64 = 1.5;

/// Maximum number of ffmpeg processes running concurrently for frame
/// extraction. Each process decodes + encodes a full-resolution frame, so an
/// unbounded fan-out (one per batch) can exhaust process/thread limits or
/// cause CPU thrash on long videos. Batches are processed in chunks of this
/// size: parallel within a chunk, sequential across chunks.
const FRAME_EXTRACTION_CONCURRENCY: usize = 4;

/// Check whether a path looks like a video file. This is a lightweight
/// extension-based guard to avoid running ffmpeg on audio files or virtual
/// paths such as `youtube://...`.
fn is_likely_video_path(path: &Path) -> bool {
    const VIDEO_EXTENSIONS: &[&str] = &[
        "mp4", "mkv", "mov", "avi", "webm", "flv", "wmv", "m4v", "mpeg", "mpg", "ts",
    ];
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    VIDEO_EXTENSIONS.contains(&ext.as_str())
}

/// Compute a media-identity tag from the file's size + last-modified time.
///
/// Frame cache directories are keyed on this tag so that replacing the source
/// media *in place* (same path, e.g. a re-rendered download) invalidates the
/// cache structurally: a changed file gets a different tag → a different
/// `frames/media_<tag>/` subdir → old frames are never reused. This avoids the
/// stale-frame risk that the timestamp-only cache key created, without needing
/// a content hash (size+mtime is O(1) and sufficient to detect replacement).
///
/// Returns `None` if metadata can't be read (e.g. the file vanished between
/// the existence check and here). The caller decides how to handle that —
/// falling back to text-only is safer than reusing an `"unknown"` tag shared
/// across unrelated media.
fn media_identity_tag(video: &Path) -> Option<String> {
    let meta = std::fs::metadata(video).ok()?;
    let size = meta.len();
    // mtime in seconds since UNIX_EPOCH; fall back to 0 if unavailable.
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    Some(format!("{size}_{mtime}"))
}

/// Probe the video's duration in seconds by parsing ffmpeg's stderr
/// `Duration: HH:MM:SS.xx` line. Returns `None` if it can't be determined
/// (callers then skip end-clamping and rely on per-frame best-effort skip).
///
/// We don't ship ffprobe, so this reuses the bundled `ffmpeg -i` (which exits
/// non-zero but still prints the stream info to stderr before bailing).
fn probe_video_duration(ffmpeg: &Path, video: &Path) -> Option<f64> {
    let mut command = Command::new(ffmpeg);
    configure_background_command(&mut command);
    // `-i` with no output makes ffmpeg print metadata and exit; we only care
    // about stderr's "Duration:" line.
    let output = command.arg("-i").arg(video).output().ok()?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    let line = stderr.lines().find(|l| l.contains("Duration:"))?;
    let raw = line.split_once("Duration:").and_then(|(_, rest)| {
        rest.trim_start().split(|c: char| c == ',' || c.is_whitespace()).next()
    })?;
    parse_duration(raw)
}

/// Parse `HH:MM:SS.xx` into seconds.
fn parse_duration(s: &str) -> Option<f64> {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() != 3 {
        return None;
    }
    let h: f64 = parts[0].parse().ok()?;
    let m: f64 = parts[1].parse().ok()?;
    let sec: f64 = parts[2].parse().ok()?;
    Some(h * 3600.0 + m * 60.0 + sec)
}

/// Extract frames for each batch's time range, returning `Frame` pairs
/// (filename + base64 data URL).
///
/// `batch_time_ranges` is one `(start_seconds, end_seconds)` per batch,
/// aligned with the order batches are constructed in `build_batch_windows`.
/// The returned `Vec` has one entry per batch; each entry is the list of
/// frames for that batch's time range (may be empty if extraction failed
/// for all timestamps in the range, or if the batch was skipped).
///
/// `skip_batch_indices` holds batch indices whose translations are already
/// persisted (resume case). Those batches get empty frame lists and no ffmpeg
/// work, avoiding redundant extraction on resume.
///
/// Batches are extracted with bounded concurrency (`FRAME_EXTRACTION_CONCURRENCY`
/// ffmpeg processes at a time) to avoid process/thread exhaustion on long videos.
pub fn extract_frames_for_batches(
    video_path: &str,
    task_id: &str,
    batch_time_ranges: &[(f64, f64)],
    skip_batch_indices: &HashSet<usize>,
) -> Result<Vec<Vec<Frame>>, String> {
    let logger = TaskLogger::llm_with_media(task_id.to_string(), video_path.to_string());

    if batch_time_ranges.is_empty() {
        return Ok(Vec::new());
    }
    if video_path.trim().is_empty() {
        return Err("media path is empty".to_string());
    }
    let video = Path::new(video_path);
    if !video.exists() {
        return Err(format!("media file not found: {video_path}"));
    }
    if !is_likely_video_path(video) {
        logger.event(
            event::VISION_BATCH_SKIPPED,
            Some(&json!({ "reason": "non_video_media", "mediaPath": video_path })),
        );
        return Ok(batch_time_ranges.iter().map(|_| Vec::new()).collect());
    }

    let ffmpeg = resolve_bundled_or_path("ffmpeg");
    // Cache subdir includes a media-identity tag (size+mtime) so a replaced
    // source file can never reuse stale frames from a prior version. If the
    // metadata read fails right after the `exists()` check (file swapped /
    // AV scan lock / race), bail out of vision entirely rather than risk
    // serving frames from an `"unknown"` cache shared across unrelated media.
    let media_tag = media_identity_tag(video).ok_or_else(|| {
        format!("failed to read media metadata for cache keying: {video_path}")
    })?;
    let cache_dir = task_artifacts_dir(task_id, video)
        .join("frames")
        .join(format!("media_{media_tag}"));
    std::fs::create_dir_all(&cache_dir)
        .map_err(|e| format!("failed to create frames cache dir {:?}: {e}", cache_dir))?;

    // Probe the real video duration once and clamp every batch's end to it.
    // ASR/alignment timestamps can slightly overshoot the actual media length
    // (e.g. a 3527.69s video gets a batch ending at 3540s), and seeking past
    // the end always fails — silently dropping that batch's frames. Clamping
    // to `duration - LAST_FRAME_OFFSET` keeps the last frame inside the media.
    //
    // A reported duration of 0 (live streams, truncated downloads, or ffmpeg
    // emitting `Duration: 00:00:00.00`) is treated as "unknown" — clamping to
    // 0 would force every batch's `frame_end` to 0 and silently disable
    // vision for the whole task with no log. Falling back to `f64::MAX` keeps
    // per-frame best-effort seeking behavior instead.
    let max_end = match probe_video_duration(&ffmpeg, video) {
        Some(d) if d > LAST_FRAME_OFFSET => d - LAST_FRAME_OFFSET,
        // Zero / negative / unprobeable: don't clamp.
        _ => f64::MAX,
    };

    // Build the per-batch jobs, honoring skip_batch_indices (resume: already
    // translated → no frames needed) and clamping each end to the media length.
    let jobs: Vec<(usize, f64, f64)> = batch_time_ranges
        .iter()
        .enumerate()
        .filter_map(|(batch_index, &(start, end))| {
            if skip_batch_indices.contains(&batch_index) {
                logger.event(
                    event::VISION_BATCH_SKIPPED,
                    Some(&json!({ "batchId": batch_index + 1, "reason": "already_translated" })),
                );
                return None;
            }
            // Push the first batch's first frame past the TV station intro.
            let frame_start = if batch_index == 0 {
                start.max(INTRO_SKIP_SECONDS)
            } else {
                start
            };
            // Clamp end to the real media duration so we never seek past it.
            let frame_end = end.min(max_end);
            Some((batch_index, frame_start, frame_end))
        })
        .collect();

    // Run jobs with bounded concurrency: chunk into groups of
    // FRAME_EXTRACTION_CONCURRENCY, spawn one thread per job within a chunk
    // (parallel), and wait for the chunk before starting the next (serial).
    // This caps concurrent ffmpeg processes without a semaphore dependency.
    let mut results: Vec<(usize, Result<Vec<Frame>, String>)> = Vec::with_capacity(jobs.len());
    for chunk in jobs.chunks(FRAME_EXTRACTION_CONCURRENCY) {
        let chunk_results: Vec<(usize, Result<Vec<Frame>, String>)> = std::thread::scope(|scope| {
            // Spawn one worker per job in the chunk. Each worker wraps its body
            // in `catch_unwind` so a panic is converted into an `Err` that still
            // carries the worker's own batch index — a panic never gets
            // misattributed to batch 0 (which would clobber a real batch's slot).
            let handles: Vec<_> = chunk
                .iter()
                .map(|(batch_index, frame_start, frame_end)| {
                    let ffmpeg = ffmpeg.clone();
                    let video = video.to_path_buf();
                    let cache_dir = cache_dir.clone();
                    // Borrow the logger by reference; std::thread::scope
                    // guarantees it outlives the spawned threads.
                    let logger = &logger;
                    scope.spawn(move || {
                        // catch_unwind so the index survives a panic inside
                        // extract_frames_in_range (or any library it calls).
                        let inner = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            extract_frames_in_range(
                                &ffmpeg,
                                &video,
                                *frame_start,
                                *frame_end,
                                &cache_dir,
                                logger,
                            )
                        }));
                        let res = match inner {
                            Ok(Ok(frames)) => Ok(frames),
                            Ok(Err(err)) => Err(format!("batch {batch_index}: {err}")),
                            Err(_) => Err(format!("batch {batch_index}: worker thread panicked")),
                        };
                        (*batch_index, res)
                    })
                })
                .collect();
            // join() can only fail if the thread panicked *despite* the
            // catch_unwind above (e.g. panic-in-drop). That path can't recover
            // the index, so attribute it conservatively; the slot stays empty
            // and the batch falls back to text-only — the safe degradation.
            handles
                .into_iter()
                .map(|h| {
                    h.join()
                        .unwrap_or((0, Err("worker thread panicked".to_string())))
                })
                .collect()
        });
        results.extend(chunk_results);
    }

    // Preserve batch order in the output regardless of thread completion order.
    let mut out = vec![Vec::<Frame>::new(); batch_time_ranges.len()];
    for (batch_index, result) in results {
        match result {
            Ok(frames) => out[batch_index] = frames,
            // Per-batch failure is best-effort: an empty frame list makes the
            // batch fall back to text-only translation rather than failing.
            Err(err) => logger.event(
                event::VISION_BATCH_FAILED,
                Some(&json!({ "batchId": batch_index + 1, "error": err })),
            ),
        }
    }
    Ok(out)
}

/// Extract frames at fixed interval within [start, end] (clamped to avoid
/// the trailing GOP), returning `Frame` pairs.
fn extract_frames_in_range(
    ffmpeg: &Path,
    video: &Path,
    start_seconds: f64,
    end_seconds: f64,
    cache_dir: &Path,
    logger: &TaskLogger,
) -> Result<Vec<Frame>, String> {
    if end_seconds <= start_seconds {
        return Ok(Vec::new());
    }
    // Clamp the end to avoid the trailing GOP fast-seek issue.
    let end_clamped = (end_seconds - LAST_FRAME_OFFSET).max(start_seconds);

    let timestamps = absolute_interval_timestamps(start_seconds, end_clamped);
    let mut frames = Vec::new();
    for ts in timestamps {
        // Bound the number of frames sent per batch so a pathological long
        // batch can't blow the token/request budget. Earlier timestamps win
        // (they cover the start of the batch's subtitle range).
        if frames.len() >= MAX_FRAMES_PER_BATCH {
            break;
        }
        let filename = format!("frame_{:010.3}_orig.jpg", ts);
        let frame_path = cache_dir.join(&filename);
        if !frame_path.exists()
            && let Err(err) = extract_single_frame(ffmpeg, video, &frame_path, ts)
        {
            // Best-effort: skip frames that fail to extract (e.g. corrupt
            // video segments) rather than failing the whole batch.
            logger.event(
                event::VISION_FRAME_SKIPPED,
                Some(&json!({ "timestamp": format!("{ts:.3}"), "error": err })),
            );
            continue;
        }
        match std::fs::read(&frame_path) {
            Ok(bytes) => {
                let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
                frames.push(Frame {
                    filename,
                    data_url: format!("data:image/jpeg;base64,{b64}"),
                });
            }
            Err(err) => {
                logger.event(
                    event::VISION_FRAME_READ_FAILED,
                    Some(&json!({
                        "frame": filename,
                        "error": err.to_string(),
                    })),
                );
            }
        }
    }
    Ok(frames)
}

/// Sample timestamps at `INTERVAL_SECONDS` intervals within [start, end],
/// always including `start`. Mirrors owarai-grillmaster's
/// `absolute_interval_timestamps` logic.
fn absolute_interval_timestamps(start: f64, end: f64) -> Vec<f64> {
    if INTERVAL_SECONDS <= 0.0 {
        return Vec::new();
    }
    let mut out: Vec<f64> = vec![start];

    // First slot on the global interval grid >= start.
    let mut current = (start / INTERVAL_SECONDS).ceil() * INTERVAL_SECONDS;
    if (current - start).abs() < 1e-3 {
        // start itself was on the grid; advance to next slot.
        current += INTERVAL_SECONDS;
    }
    while current < end {
        out.push((current * 1000.0).round() / 1000.0);
        current += INTERVAL_SECONDS;
    }
    out
}

/// Extract one frame at `timestamp` at the source resolution. Output is JPEG
/// (mjpeg + q:v=2, visually lossless).
///
/// Writes to a *unique* `<output>.<pid>.<thread>.part` temp file first, then
/// atomically renames to the final name. Uniqueness matters because two batches
/// can sample the same global-grid timestamp: if they shared one `.part` path,
/// the two ffmpeg processes would overwrite each other's partial output and
/// the final `rename` would race (on Windows `std::fs::rename` fails when the
/// destination already exists, so the second writer would error out). With a
/// unique temp file each writer is independent; the second `rename` overwrites
/// the first frame with byte-identical content — no corruption either way.
/// `rename` is atomic on both Windows and POSIX when source and destination
/// share a filesystem/directory.
fn extract_single_frame(
    ffmpeg: &Path,
    video: &Path,
    output: &Path,
    timestamp: f64,
) -> Result<(), String> {
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("create frame dir: {e}"))?;
    }
    // Unique temp file per writer: two workers extracting the same timestamp
    // get distinct temp paths, so neither clobbers the other. The PID scopes
    // uniqueness across processes; the counter across threads/calls within one.
    let temp_path = {
        let stem = output
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("frame");
        let pid = std::process::id();
        let seq = TEMP_FILE_SEQ.fetch_add(1, Ordering::Relaxed);
        let temp_name = format!("{stem}.{pid}.{seq}.part");
        output.with_file_name(temp_name)
    };
    // `-ss` before `-i` = fast seek (keyframe-based). Good enough for frame
    // sampling; precise seek would require `-ss` after `-i` which is much
    // slower. No scale filter: frames are kept at source resolution so
    // text-heavy content stays legible to the vision model.
    //
    // `-q:v` (not the deprecated `-qscale`, which ffmpeg 7.1 rejects as
    // ambiguous) controls JPEG quality. `-pix_fmt yuvj420p` is mandatory:
    // source video is usually tv-range `yuv420p`, but the mjpeg encoder
    // needs full-range `yuvj420p`. Without it mjpeg's threaded encoder fails
    // to init (`ff_frame_thread_encoder_init failed` → every frame skipped →
    // vision assist silently degrades to text-only).
    let mut command = Command::new(ffmpeg);
    configure_background_command(&mut command);
    let output_result = command
        .arg("-y")
        .arg("-ss")
        .arg(format!("{:.3}", timestamp))
        .arg("-i")
        .arg(video)
        .arg("-frames:v")
        .arg("1")
        .arg("-f")
        .arg("image2")
        .arg("-vcodec")
        .arg("mjpeg")
        .arg("-q:v")
        .arg("2")
        .arg("-pix_fmt")
        .arg("yuvj420p")
        .arg(&temp_path)
        .output()
        .map_err(|e| format!("ffmpeg invocation failed: {e}"))?;

    if !output_result.status.success() || !temp_path.exists() {
        // Clean up a possible partial temp file on failure.
        let _ = std::fs::remove_file(&temp_path);
        let stderr = String::from_utf8_lossy(&output_result.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            "ffmpeg exited without success".to_string()
        } else {
            stderr
        });
    }
    // Atomically publish the complete frame. If two threads raced to extract
    // the same timestamp, the second rename simply overwrites the first with
    // identical content — no corruption either way.
    std::fs::rename(&temp_path, output)
        .map_err(|e| format!("rename frame temp file failed: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absolute_interval_timestamps_includes_start_and_slots() {
        let ts = absolute_interval_timestamps(0.0, 90.0);
        // start=0, then 30, 60 (90 is excluded by `< end`).
        assert_eq!(ts, vec![0.0, 30.0, 60.0]);
    }

    #[test]
    fn absolute_interval_timestamps_clamps_short_range() {
        // Range shorter than interval: only start.
        let ts = absolute_interval_timestamps(5.0, 10.0);
        assert_eq!(ts, vec![5.0]);
    }

    #[test]
    fn absolute_interval_timestamps_handles_start_on_grid() {
        // start=30 is on the grid; next slot is 60.
        let ts = absolute_interval_timestamps(30.0, 90.0);
        assert_eq!(ts, vec![30.0, 60.0]);
    }

    #[test]
    fn absolute_interval_timestamps_handles_non_grid_start() {
        // start=10, next grid slot is 30.
        let ts = absolute_interval_timestamps(10.0, 90.0);
        assert_eq!(ts, vec![10.0, 30.0, 60.0]);
    }

    #[test]
    fn parse_duration_handles_hhmmss() {
        // 58:47.69 = 3527.69s — the HOSTCALL video that exposed the overshoot.
        assert_eq!(parse_duration("00:58:47.69"), Some(3527.69));
        assert_eq!(parse_duration("01:02:03.00"), Some(3723.0));
    }

    #[test]
    fn parse_duration_rejects_garbage() {
        assert_eq!(parse_duration("N/A"), None);
        assert_eq!(parse_duration("12:34"), None);
    }

    #[test]
    fn media_identity_tag_changes_with_metadata() {
        // The tag encodes size + mtime so a replaced source file is detected.
        // Verify the format is deterministic for a known (size, mtime) pair by
        // checking it contains both components. (We can't fix mtime portably,
        // so just assert the shape: "size_" prefix + mtime segment.)
        let dir = std::env::temp_dir();
        let a = dir.join("voxtrans_mit_a.bin");
        let b = dir.join("voxtrans_mit_b.bin");
        std::fs::write(&a, b"hello").unwrap();
        std::fs::write(&b, b"hello world").unwrap();
        let tag_a = media_identity_tag(&a).expect("readable file yields a tag");
        let tag_b = media_identity_tag(&b).expect("readable file yields a tag");
        // Different sizes → different tags.
        assert_ne!(tag_a, tag_b, "different-size files must differ in tag");
        assert!(tag_a.starts_with("5_"), "tag should start with size: {tag_a}");
        assert!(tag_b.starts_with("11_"), "tag should start with size: {tag_b}");
        let _ = std::fs::remove_file(&a);
        let _ = std::fs::remove_file(&b);
    }

    #[test]
    fn media_identity_tag_unknown_for_missing_file() {
        let missing = std::path::Path::new("/nonexistent/voxtrans_missing_xyz.bin");
        // A missing file yields no tag (caller decides how to degrade) rather
        // than a fixed `"unknown"` string that would be shared across media.
        assert_eq!(media_identity_tag(missing), None);
    }
}
