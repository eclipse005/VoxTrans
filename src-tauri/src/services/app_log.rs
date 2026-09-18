//! Application-wide error log.
//!
//! Task logs (`services::logs`) are scoped to one subtitle task and live inside
//! that task's output dir. This is the other axis: failures that belong to the
//! *app* rather than to a task — startup, panics, yt-dlp, model downloads, LLM
//! endpoints, persistence — recorded once per process and meant to be sent to
//! the developer as-is.
//!
//! Design notes:
//!
//! - **Error level only.** The file is a support artifact, not a trace: every
//!   line should be something a user would plausibly report.
//! - **One JSON object per line.** Append-only and crash-safe (a torn write
//!   ruins only its own line), groupable by `code`, and still greppable. `code`
//!   is what makes "is this bug fixed?" an answerable question.
//! - **Never in the way.** Every write is best-effort: a failure disables the
//!   log instead of propagating, so a full disk or a locked file can never break
//!   a pipeline. The lock tolerates poisoning for the same reason — the panic
//!   hook must still be able to record the panic that poisoned it.
//! - **No Tauri handle.** `output::resolve_output_dir()` needs only the exe path
//!   and the environment, so the log exists *before* `tauri::Builder` runs. That
//!   is deliberate: bootstrap failures happen before an `AppHandle` exists and
//!   are exactly the ones that used to vanish into a console-less release build.
//!
//! Task correlation is by convention, not by a dedicated field: pass
//! `{"taskId": ...}` in `context`.
//!
//! [`init`] and [`install_panic_hook`] are called as the binary's first
//! statements; nothing else needs wiring.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};

use chrono::Local;
use serde::Serialize;

/// Logs land beside the managed output root, not inside a task dir: the app log
/// must be writable when no task has ever run.
const LOG_DIR_NAME: &str = "logs";
const LOG_FILE_NAME: &str = "app.log";
/// Rotate once the active file reaches this size. Small on purpose — the file is
/// meant to be emailed.
const MAX_BYTES: u64 = 2 * 1024 * 1024;
/// `app.log` plus `app.1.log` .. `app.3.log`.
const MAX_FILES: usize = 4;
/// RFC 3339 with a local offset — the timestamp a user can compare against their
/// own clock when describing "it happened around 21:50".
const TS_FORMAT: &str = "%Y-%m-%dT%H:%M:%S%.3f%:z";

static LOG: OnceLock<Mutex<State>> = OnceLock::new();

/// The directory holding the app log (`<output root>/logs`).
pub fn log_dir() -> PathBuf {
    crate::services::output::resolve_output_dir().join(LOG_DIR_NAME)
}

/// The active log file (`<output root>/logs/app.log`).
pub fn log_path() -> PathBuf {
    log_dir().join(LOG_FILE_NAME)
}

/// Resolve the path and open the file eagerly, so the very first failure has a
/// destination. Idempotent and best-effort; never returns an error.
pub fn init() {
    let mut state = lock();
    state.ensure_open();
}

/// Record an error. `scope` names the subsystem (`"youtube.download"`), `code` is
/// a stable identifier for this failure mode (`"YTDLP_STALL"`), and `context`
/// carries whatever makes the report actionable — conventionally including
/// `"taskId"` when the failure belongs to a task.
pub fn error(
    scope: &str,
    code: &str,
    message: impl AsRef<str>,
    context: Option<serde_json::Value>,
) {
    let entry = Entry {
        ts: Local::now().format(TS_FORMAT).to_string(),
        level: "error",
        pid: std::process::id(),
        app: env!("CARGO_PKG_VERSION"),
        os: os_tag(),
        scope,
        code,
        message: message.as_ref(),
        context: context.as_ref(),
    };

    let mut state = lock();
    let _ = write_entry(&mut state, &entry);
}

/// Wrap a command result so a failure is recorded once, at the IPC boundary.
///
/// A command is where a user-visible failure materializes — "it showed an error"
/// or "nothing happened" — and most of them delegate straight into a service, so
/// this is the cheapest seam that reaches every operation the app exposes. The
/// result passes through untouched: recording never changes control flow.
pub fn report<T, E: std::fmt::Display>(
    scope: &str,
    code: &str,
    result: Result<T, E>,
) -> Result<T, E> {
    if let Err(err) = &result {
        error(scope, code, err.to_string(), None);
    }
    result
}

/// Route panics into the log, keeping the default hook so debug builds still
/// print. Release builds set `windows_subsystem = "windows"`, so stderr goes
/// nowhere — without this, a crash leaves no trace at all.
pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "unknown".to_string());
        error(
            "app.panic",
            "PANIC",
            panic_payload(info),
            Some(serde_json::json!({ "location": location })),
        );
        previous(info);
    }));
}

fn panic_payload(info: &std::panic::PanicHookInfo<'_>) -> String {
    if let Some(text) = info.payload().downcast_ref::<&str>() {
        return (*text).to_string();
    }
    if let Some(text) = info.payload().downcast_ref::<String>() {
        return text.clone();
    }
    "panic with a non-string payload".to_string()
}

fn os_tag() -> &'static str {
    static TAG: OnceLock<String> = OnceLock::new();
    TAG.get_or_init(|| {
        format!(
            "{}-{}",
            std::env::consts::OS,
            std::env::consts::ARCH
        )
    })
}

fn state() -> &'static Mutex<State> {
    LOG.get_or_init(|| Mutex::new(State::default()))
}

/// Poisoning is tolerated: the panic hook needs the lock *after* an unwinding
/// thread may have dropped it mid-write.
fn lock() -> MutexGuard<'static, State> {
    match state().lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Entry<'a> {
    ts: String,
    level: &'static str,
    pid: u32,
    app: &'static str,
    os: &'static str,
    scope: &'a str,
    code: &'a str,
    message: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    context: Option<&'a serde_json::Value>,
}

#[derive(Default)]
struct State {
    dir: Option<PathBuf>,
    file: Option<File>,
    /// Set after a directory/file failure so we stop retrying on every error.
    disabled: bool,
}

impl State {
    fn ensure_open(&mut self) -> bool {
        self.ensure_open_in(&log_dir())
    }

    fn ensure_open_in(&mut self, dir: &Path) -> bool {
        if self.file.is_some() {
            return true;
        }
        if self.disabled {
            return false;
        }
        if fs::create_dir_all(dir).is_err() {
            self.disabled = true;
            return false;
        }
        match open_append(&active_path(dir)) {
            Ok(file) => {
                self.dir = Some(dir.to_path_buf());
                self.file = Some(file);
                true
            }
            Err(_) => {
                self.disabled = true;
                false
            }
        }
    }

    /// Roll the file over when it has reached the cap, so a long-lived install
    /// (or an error storm) can never fill the disk.
    fn rotate_if_needed(&mut self) -> bool {
        let Some(dir) = self.dir.clone() else {
            return false;
        };
        let at_cap = self
            .file
            .as_ref()
            .and_then(|file| file.metadata().ok())
            .is_some_and(|meta| meta.len() >= MAX_BYTES);
        if !at_cap {
            return true;
        }

        // The handle must be closed before the rename — Windows refuses to
        // rename a file that is still open.
        drop(self.file.take());
        if rotate(&dir).is_err() {
            // Rotation failed; keep appending to whatever path is left rather
            // than losing the line we were about to write.
            return self.reopen(&dir);
        }
        self.reopen(&dir)
    }

    fn reopen(&mut self, dir: &Path) -> bool {
        match open_append(&active_path(dir)) {
            Ok(file) => {
                self.file = Some(file);
                true
            }
            Err(_) => {
                self.disabled = true;
                false
            }
        }
    }
}

fn write_entry(state: &mut State, entry: &Entry<'_>) -> io::Result<()> {
    if !state.ensure_open() {
        return Ok(());
    }
    if !state.rotate_if_needed() {
        return Ok(());
    }

    let mut line = serde_json::to_string(entry).map_err(io::Error::other)?;
    line.push('\n');

    let Some(file) = state.file.as_mut() else {
        return Ok(());
    };
    // One `write_all` plus a flush per line: errors are rare, and a support log
    // is worthless if it is still sitting in a buffer when the process dies.
    if let Err(err) = file.write_all(line.as_bytes()).and_then(|()| file.flush()) {
        state.disabled = true;
        state.file = None;
        return Err(err);
    }
    Ok(())
}

fn open_append(path: &Path) -> io::Result<File> {
    OpenOptions::new().create(true).append(true).open(path)
}

fn active_path(dir: &Path) -> PathBuf {
    dir.join(LOG_FILE_NAME)
}

/// `app.log` for `0`, `app.1.log` for `1`, ...
fn rotated_path(dir: &Path, generation: usize) -> PathBuf {
    if generation == 0 {
        active_path(dir)
    } else {
        dir.join(format!("app.{generation}.log"))
    }
}

/// Shift every generation up by one and drop the oldest, freeing `app.log`.
fn rotate(dir: &Path) -> io::Result<()> {
    let _ = fs::remove_file(rotated_path(dir, MAX_FILES - 1));
    for generation in (1..MAX_FILES - 1).rev() {
        let from = rotated_path(dir, generation);
        if from.exists() {
            fs::rename(&from, rotated_path(dir, generation + 1))?;
        }
    }
    fs::rename(active_path(dir), rotated_path(dir, 1))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    /// Unique per test so parallel cases cannot share a directory.
    fn scratch_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "voxtrans_app_log_{label}_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    fn sample<'a>(message: &'a str) -> Entry<'a> {
        Entry {
            ts: "2026-09-18T22:00:00.000+08:00".to_string(),
            level: "error",
            pid: 4242,
            app: "1.5.1",
            os: "windows-x86_64",
            scope: "youtube.download",
            code: "YTDLP_STALL",
            message,
            context: None,
        }
    }

    fn read_lines(path: &Path) -> Vec<String> {
        let mut text = String::new();
        File::open(path)
            .expect("open log")
            .read_to_string(&mut text)
            .expect("read log");
        text.lines().map(|line| line.to_string()).collect()
    }

    #[test]
    fn report_passes_the_result_through_untouched() {
        // Recording must never change control flow. The failure arm writes one
        // line into the build dir's output tree — that is where `log_dir()`
        // resolves during tests, not the user's install.
        assert_eq!(report("test.boundary", "OK_CODE", Ok::<u32, String>(7)), Ok(7));
        assert_eq!(
            report("test.boundary", "ERR_CODE", Err::<u32, String>("boom".into())),
            Err("boom".to_string())
        );
    }

    #[test]
    fn the_log_sits_under_the_managed_output_root() {
        // The support workflow ("open the folder and send me that file") depends
        // on this being a predictable place, so pin the shape of it.
        let dir = log_dir();
        assert_eq!(dir.file_name().and_then(|name| name.to_str()), Some(LOG_DIR_NAME));
        assert_eq!(dir, crate::services::output::resolve_output_dir().join(LOG_DIR_NAME));
        assert_eq!(log_path(), dir.join(LOG_FILE_NAME));
    }

    #[test]
    fn one_line_per_entry_is_a_self_contained_json_object() {
        let dir = scratch_dir("envelope");
        let mut state = State::default();
        assert!(state.ensure_open_in(&dir));

        let context = serde_json::json!({ "taskId": "task-1" });
        let mut entry = sample("yt-dlp stalled after 300s with no output");
        entry.context = Some(&context);
        write_entry(&mut state, &entry).expect("write");

        let lines = read_lines(&active_path(&dir));
        assert_eq!(lines.len(), 1, "one entry must be exactly one line");

        let value: serde_json::Value = serde_json::from_str(&lines[0]).expect("valid json line");
        assert_eq!(value["level"], "error");
        assert_eq!(value["code"], "YTDLP_STALL");
        assert_eq!(value["scope"], "youtube.download");
        assert_eq!(value["app"], "1.5.1");
        assert_eq!(value["context"]["taskId"], "task-1");
    }

    #[test]
    fn context_is_omitted_rather_than_null_when_absent() {
        let dir = scratch_dir("no_context");
        let mut state = State::default();
        assert!(state.ensure_open_in(&dir));
        write_entry(&mut state, &sample("startup failure")).expect("write");

        let line = read_lines(&active_path(&dir)).remove(0);
        assert!(!line.contains("context"), "absent context must not serialize: {line}");
    }

    #[test]
    fn entries_accumulate_in_append_order() {
        let dir = scratch_dir("append");
        let mut state = State::default();
        assert!(state.ensure_open_in(&dir));

        write_entry(&mut state, &sample("first")).expect("write");
        write_entry(&mut state, &sample("second")).expect("write");

        let lines = read_lines(&active_path(&dir));
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("first"));
        assert!(lines[1].contains("second"));
    }

    #[test]
    fn rotation_keeps_the_newest_generations_and_drops_the_oldest() {
        let dir = scratch_dir("rotation");

        // Pre-fill every generation so the drop is observable.
        for generation in 0..MAX_FILES {
            fs::write(rotated_path(&dir, generation), format!("gen{generation}")).expect("seed");
        }

        rotate(&dir).expect("rotate");

        assert_eq!(
            fs::read_to_string(rotated_path(&dir, 1)).expect("gen1"),
            "gen0",
            "the active file becomes the first generation"
        );
        assert_eq!(
            fs::read_to_string(rotated_path(&dir, MAX_FILES - 1)).expect("oldest kept"),
            format!("gen{}", MAX_FILES - 2)
        );
        assert!(
            !rotated_path(&dir, MAX_FILES).exists(),
            "nothing beyond the retention window is kept"
        );
    }

    #[test]
    fn the_active_file_rolls_over_once_it_reaches_the_cap() {
        let dir = scratch_dir("cap");
        let mut state = State::default();
        assert!(state.ensure_open_in(&dir));

        // Overshoot the cap without writing 2 MB of real entries.
        fs::write(active_path(&dir), vec![b'x'; MAX_BYTES as usize]).expect("fill");
        state.file = None;
        assert!(state.ensure_open_in(&dir));

        write_entry(&mut state, &sample("after rotation")).expect("write");

        let rolled = read_lines(&rotated_path(&dir, 1));
        assert_eq!(rolled.len(), 1);
        assert!(rolled[0].starts_with("xxxx"), "the oversized file was rolled aside");

        let active = read_lines(&active_path(&dir));
        assert_eq!(active.len(), 1);
        assert!(active[0].contains("after rotation"));
    }

    #[test]
    fn an_unusable_directory_disables_the_log_without_panicking() {
        // A file where a directory is expected: create_dir_all fails fatally.
        let blocker = std::env::temp_dir().join(format!(
            "voxtrans_app_log_blocker_{}",
            std::process::id()
        ));
        fs::write(&blocker, b"not a directory").expect("blocker file");

        let mut state = State::default();
        assert!(!state.ensure_open_in(&blocker.join("logs")));
        assert!(state.disabled, "a failed open must not be retried forever");
        // Still a no-op rather than a panic.
        write_entry(&mut state, &sample("unwritable")).expect("best effort");

        let _ = fs::remove_file(&blocker);
    }
}
