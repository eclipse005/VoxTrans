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
//!   line should be something a user would plausibly report. The one exception is
//!   the session markers (`level: "session"`, two lines per run): they exist so
//!   that an abnormal exit is detectable at all. Filter on `level` to read the
//!   file as a pure error list.
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
//! # Deaths that leave a trace
//!
//! "It just disappeared" used to be undiagnosable, because nothing in the log
//! said whether the app had been closed or had died. Three mechanisms cover the
//! space between a clean shutdown and a silent kill, in decreasing order of how
//! much they can tell you:
//!
//! 1. [`session_start`] / [`session_stop`] write one line each. A start with no
//!    matching stop means the process never reached its shutdown path: killed,
//!    out of memory, power loss, or a hard crash. Nothing else can distinguish
//!    "the user closed it" from "it vanished".
//! 2. [`set_phase`] keeps the running pipeline stage in `logs/last-phase.txt` —
//!    a sidecar rather than a log line, because stage transitions are normal
//!    operation. It is read exactly once, when (1) says the previous session
//!    died, turning "it crashed" into "it crashed while translating task ep12".
//! 3. [`install_crash_handler`] (Windows) writes `logs/crash-<pid>.txt` when the
//!    process dies from a native fault — a CUDA driver error, an illegal access,
//!    a stack overflow — that reaches no Rust `Result` and no panic hook. The
//!    next start promotes it into a real entry.
//!
//! [`init`], [`install_panic_hook`], [`install_crash_handler`] and
//! [`session_start`] are called as the binary's first statements;
//! [`session_stop`] is called on a deliberate shutdown.

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
/// Level of the two session lifecycle lines. Not `error`: they exist to make an
/// abnormal exit detectable, and a reader who wants errors only can filter them
/// out by this field.
const SESSION_LEVEL: &str = "session";
const SESSION_SCOPE: &str = "app.session";
const SESSION_START_CODE: &str = "SESSION_START";
const SESSION_STOP_CODE: &str = "SESSION_STOP";
/// Sidecar holding the current pipeline stage (see the module doc, layer 2).
const PHASE_FILE_NAME: &str = "last-phase.txt";
/// Native-crash reports (module doc, layer 3).
const CRASH_FILE_PREFIX: &str = "crash-";
/// The phase may embed a task id; keep the sidecar short enough to read at a
/// glance inside an error message.
const MAX_PHASE_CHARS: usize = 160;

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
        build: crate::services::BUILD_VARIANT,
        os: os_tag(),
        scope,
        code,
        message: message.as_ref(),
        context: context.as_ref(),
    };

    let mut state = lock();
    let _ = write_entry(&mut state, &entry);
}

/// Run `body` and record a failure once, at the IPC boundary.
///
/// The body is a **closure on purpose**. `?` inside it returns from the closure,
/// so the `Err` always reaches this function; an API that takes an
/// already-computed `Result` cannot promise that — `report(scope, code,
/// Ok(x.await?))` returns from the *command* before the call even happens, and
/// records nothing. Taking the closure makes that mistake unrepresentable.
pub fn guard<T, E: std::fmt::Display>(
    scope: &str,
    code: &str,
    body: impl FnOnce() -> Result<T, E>,
) -> Result<T, E> {
    let result = body();
    if let Err(err) = &result {
        error(scope, code, err.to_string(), None);
    }
    result
}

/// [`guard`] for async command bodies:
/// `guard_async(scope, code, async { .. }).await`.
pub async fn guard_async<T, E: std::fmt::Display>(
    scope: &str,
    code: &str,
    body: impl std::future::Future<Output = Result<T, E>>,
) -> Result<T, E> {
    let result = body.await;
    if let Err(err) = &result {
        error(scope, code, err.to_string(), None);
    }
    result
}

// ── Session lifecycle, phase trail, native crashes ──────────────────────────
//
// The three layers described in the module doc. They share one property: none of
// them can fail the caller. Recording that the app died must never be a reason
// for the app to die.

/// The pipeline stage last reported by [`set_phase`], cached so repeated
/// progress ticks with an unchanged stage touch no file.
static LAST_PHASE: Mutex<Option<String>> = Mutex::new(None);

/// Record the start of this session, and report what the previous one left
/// behind: a native-crash report if there is one, otherwise an unfinished
/// session marker.
///
/// Called once, right after [`init`]. Both reports are best-effort; a missing or
/// unreadable log directory simply means nothing to say.
pub fn session_start() {
    // Layer 3 before layer 1: a crash report is the most specific evidence, and
    // "unfinished session" is the fallback when there is none.
    for report in take_crash_reports() {
        let phase = report.phase.clone();
        error(
            "app.crash",
            "NATIVE_EXCEPTION",
            report.summary(),
            Some(serde_json::json!({
                "report": report.raw,
                "phase": phase,
            })),
        );
    }

    if let Some(previous) = previous_session_left_unfinished() {
        error(
            "app.session",
            "UNEXPECTED_EXIT",
            format!(
                "previous session (pid {}) started at {} and never recorded a clean shutdown",
                previous.pid, previous.ts
            ),
            Some(serde_json::json!({
                "pid": previous.pid,
                "startedAt": previous.ts,
                "phase": read_phase(),
            })),
        );
    }

    write_session_line(SESSION_START_CODE, "session started");
}

/// Record a deliberate shutdown. This is the marker that makes the next start's
/// abnormal-exit check mean something, so it is written on every exit path that
/// the process actually reaches.
pub fn session_stop() {
    write_session_line(SESSION_STOP_CODE, "clean shutdown");
    clear_phase();
}

fn write_session_line(code: &str, message: &str) {
    let entry = session_marker_entry(code, message);
    let mut state = lock();
    let _ = write_entry(&mut state, &entry);
}

/// The session marker as an entry. Split out from the writer so a test can pin
/// its shape (level, scope, code) without touching the global log state.
fn session_marker_entry<'a>(code: &'a str, message: &'a str) -> Entry<'a> {
    Entry {
        ts: Local::now().format(TS_FORMAT).to_string(),
        level: SESSION_LEVEL,
        pid: std::process::id(),
        app: env!("CARGO_PKG_VERSION"),
        build: crate::services::BUILD_VARIANT,
        os: os_tag(),
        scope: SESSION_SCOPE,
        code,
        message,
        context: None,
    }
}

/// Remember which pipeline stage is running, for the next start's report.
///
/// Deliberately a sidecar file and not a log line: stage transitions are normal
/// operation, and the log is error-only. Cheap when nothing changed — the same
/// stage reported twice (progress ticks) writes nothing, which is what keeps
/// this safe to call from a per-tick path.
pub fn set_phase(phase: &str) {
    let phase: String = phase.trim().chars().take(MAX_PHASE_CHARS).collect();
    if phase.is_empty() {
        return;
    }
    {
        let Ok(mut last) = LAST_PHASE.lock() else { return };
        if last.as_deref() == Some(phase.as_str()) {
            return;
        }
        *last = Some(phase.clone());
    }
    let dir = log_dir();
    if fs::create_dir_all(&dir).is_err() {
        return;
    }
    let _ = fs::write(dir.join(PHASE_FILE_NAME), phase.as_bytes());
}

/// Drop the phase trail on a clean shutdown: a leftover file must mean "died
/// mid-stage", never "closed the window".
pub fn clear_phase() {
    if let Ok(mut last) = LAST_PHASE.lock() {
        *last = None;
    }
    let _ = fs::remove_file(log_dir().join(PHASE_FILE_NAME));
}

fn read_phase() -> Option<String> {
    read_phase_in(&log_dir())
}

fn read_phase_in(dir: &Path) -> Option<String> {
    let text = fs::read_to_string(dir.join(PHASE_FILE_NAME)).ok()?;
    let text = text.trim();
    if text.is_empty() {
        None
    } else {
        Some(text.to_string())
    }
}

struct PreviousSession {
    pid: u32,
    ts: String,
}

enum SessionMarker {
    Start { pid: u32, ts: String },
    Stop,
    Unknown,
}

/// Whether the newest session marker in the log is an unfinished start from a
/// different process.
///
/// Reads the active file first and only walks older generations when it holds no
/// session line at all (an error storm can rotate the start line away), stopping
/// at the first marker found from the end.
fn previous_session_left_unfinished() -> Option<PreviousSession> {
    previous_session_left_unfinished_in(&log_dir())
}

fn previous_session_left_unfinished_in(dir: &Path) -> Option<PreviousSession> {
    for generation in 0..MAX_FILES {
        let Ok(text) = fs::read_to_string(rotated_path(dir, generation)) else {
            continue;
        };
        let Some(marker) = last_session_marker(&text) else {
            continue;
        };
        return match marker {
            SessionMarker::Start { pid, ts } if pid != std::process::id() => {
                Some(PreviousSession { pid, ts })
            }
            _ => None,
        };
    }
    None
}

/// The newest session marker in one log's text.
///
/// Pure on purpose: "was the previous session unfinished?" is a property of the
/// log contents, so it is testable without a dying process. Lines are filtered
/// by a substring before parsing, so scanning 8 MB costs almost nothing.
fn last_session_marker(text: &str) -> Option<SessionMarker> {
    for line in text.lines().rev() {
        if !line.contains("\"level\":\"session\"") {
            continue;
        }
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let code = value.get("code").and_then(|c| c.as_str()).unwrap_or_default();
        let pid = value.get("pid").and_then(|p| p.as_u64()).unwrap_or(0) as u32;
        let ts = value
            .get("ts")
            .and_then(|t| t.as_str())
            .unwrap_or_default()
            .to_string();
        return Some(match code {
            SESSION_START_CODE => SessionMarker::Start { pid, ts },
            SESSION_STOP_CODE => SessionMarker::Stop,
            _ => SessionMarker::Unknown,
        });
    }
    None
}

struct CrashReport {
    raw: String,
    phase: Option<String>,
}

impl CrashReport {
    fn summary(&self) -> String {
        let code = self
            .raw
            .lines()
            .next()
            .unwrap_or("native exception")
            .trim()
            .to_string();
        match &self.phase {
            Some(phase) if !phase.is_empty() => format!("native exception while in {phase} ({code})"),
            _ => format!("native exception ({code})"),
        }
    }
}

/// Promote the reports a previous process left when it died of a native fault,
/// deleting them so the next start does not repeat the same news.
fn take_crash_reports() -> Vec<CrashReport> {
    take_crash_reports_in(&log_dir())
}

fn take_crash_reports_in(dir: &Path) -> Vec<CrashReport> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut reports = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if !name.starts_with(CRASH_FILE_PREFIX) || !name.ends_with(".txt") {
            continue;
        }
        let Ok(bytes) = fs::read(entry.path()) else { continue };
        let _ = fs::remove_file(entry.path());
        // Lossy: the handler truncates into a fixed buffer and may cut a
        // multi-byte character. A slightly mangled line beats no line.
        let raw = String::from_utf8_lossy(&bytes).trim().to_string();
        // The handler writes one line (`code=… address=… pid=… phase=…`), so the
        // phase is a token in the line, not a line of its own.
        let phase = raw
            .split_whitespace()
            .find_map(|token| token.strip_prefix("phase=").map(str::to_string));
        reports.push(CrashReport { raw, phase });
    }
    reports
}

/// Route a Windows unhandled exception (SEH) into a crash report.
///
/// This is the only hook that sees a native fault: a CUDA driver error, an
/// illegal access inside a C++ engine, a stack overflow. Rust's panic hook never
/// runs for those, and the process gets no chance to write a log line — so the
/// report is a sidecar the next start promotes (see the module doc).
///
/// No-op on other platforms.
#[cfg(windows)]
pub fn install_crash_handler() {
    native_crash::install();
}

#[cfg(not(windows))]
pub fn install_crash_handler() {}

#[cfg(windows)]
mod native_crash {
    use super::{CRASH_FILE_PREFIX, LAST_PHASE, log_dir};
    use std::fmt::{self, Write as _};
    use std::fs;
    use std::path::PathBuf;
    use std::sync::OnceLock;
    use std::sync::atomic::{AtomicBool, Ordering};
    use windows_sys::Win32::System::Diagnostics::Debug::{
        EXCEPTION_POINTERS, SetUnhandledExceptionFilter,
    };

    /// Conventional "handle it and terminate" answer for an unhandled filter.
    const EXCEPTION_EXECUTE_HANDLER: i32 = 1;

    /// Resolved once at install time: the handler must not build a path (or do
    /// anything else that allocates) while the process is dying.
    static REPORT_PATH: OnceLock<PathBuf> = OnceLock::new();
    /// A fault inside the handler would otherwise re-enter it forever.
    static IN_HANDLER: AtomicBool = AtomicBool::new(false);

    pub(super) fn install() {
        let dir = log_dir();
        if fs::create_dir_all(&dir).is_ok() {
            let _ = REPORT_PATH
                .set(dir.join(format!("{CRASH_FILE_PREFIX}{}.txt", std::process::id())));
        }
        // SAFETY: a plain function pointer with the documented signature; the
        // filter only writes a file and returns.
        unsafe { SetUnhandledExceptionFilter(Some(filter)) };
    }

    unsafe extern "system" fn filter(info: *const EXCEPTION_POINTERS) -> i32 {
        if IN_HANDLER.swap(true, Ordering::SeqCst) {
            return EXCEPTION_EXECUTE_HANDLER;
        }
        if !info.is_null() && unsafe { !(*info).ExceptionRecord.is_null() } {
            let record = unsafe { *(*info).ExceptionRecord };
            write_report(record.ExceptionCode as u32, record.ExceptionAddress as usize);
        }
        EXCEPTION_EXECUTE_HANDLER
    }

    /// Best-effort, allocation-light, and never allowed to unwind: the stack may
    /// be exhausted and the heap may be suspect by the time we get here.
    fn write_report(code: u32, address: usize) {
        let Some(path) = REPORT_PATH.get() else { return };
        let mut buffer = [0u8; 192];
        // `try_lock`: the crash may have happened while the log lock (or this
        // one) was held, and blocking here would hang the dying process.
        let phase = LAST_PHASE
            .try_lock()
            .ok()
            .and_then(|guard| guard.clone());
        let len = format_report(code, address, phase.as_deref(), &mut buffer);
        let _ = fs::write(path, &buffer[..len]);
    }

    /// The report body, built without allocating (only the caller's stack buffer
    /// is used). Pure so the format stays testable without crashing a process.
    pub(super) fn format_report(
        code: u32,
        address: usize,
        phase: Option<&str>,
        buffer: &mut [u8],
    ) -> usize {
        let mut sink = FixedBuffer::new(buffer);
        let _ = write!(
            sink,
            "code=0x{code:08X} address=0x{address:016X} pid={}",
            std::process::id()
        );
        if let Some(phase) = phase.filter(|phase| !phase.is_empty()) {
            let _ = write!(sink, " phase={phase}");
        }
        sink.len
    }

    /// A `fmt::Write` sink over a fixed stack buffer: truncates instead of
    /// allocating. The handler has no heap budget to trust. `pub(super)` so the
    /// truncation contract stays covered by a test.
    pub(super) struct FixedBuffer<'a> {
        pub(super) buffer: &'a mut [u8],
        pub(super) len: usize,
    }

    impl<'a> FixedBuffer<'a> {
        pub(super) fn new(buffer: &'a mut [u8]) -> Self {
            Self { buffer, len: 0 }
        }
    }

    impl fmt::Write for FixedBuffer<'_> {
        fn write_str(&mut self, text: &str) -> fmt::Result {
            let bytes = text.as_bytes();
            let room = self.buffer.len() - self.len;
            let take = bytes.len().min(room);
            self.buffer[self.len..self.len + take].copy_from_slice(&bytes[..take]);
            self.len += take;
            Ok(())
        }
    }
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
    /// `cuda` or `cpu`: the two installers fail differently, and a GPU bug report
    /// is unusable without knowing which one produced it.
    build: &'static str,
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
    write_entry_in(&log_dir(), state, entry)
}

/// [`write_entry`] against an explicit directory. Split out so tests can write
/// into a scratch dir instead of the real log (which, during a test run, is the
/// build output tree — while another cargo process may be pruning it).
fn write_entry_in(dir: &Path, state: &mut State, entry: &Entry<'_>) -> io::Result<()> {
    if !state.ensure_open_in(dir) {
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
            build: "cpu",
            os: "windows-x86_64",
            scope: "youtube.download",
            code: "YTDLP_STALL",
            message,
            context: None,
        }
    }

    /// Serialize a session line exactly as `write_session_line` does, so the
    /// detector below is tested against the real on-disk shape (level, code,
    /// pid) rather than a hand-written approximation.
    fn session_line(code: &str, pid: u32, ts: &str) -> String {
        let entry = Entry {
            ts: ts.to_string(),
            level: SESSION_LEVEL,
            pid,
            app: "1.5.1",
            build: "cuda",
            os: "windows-x86_64",
            scope: SESSION_SCOPE,
            code,
            message: "",
            context: None,
        };
        serde_json::to_string(&entry).expect("session line")
    }

    /// A serialized error line, the shape the session-marker parser must
    /// ignore.
    fn sample_line(message: &str) -> String {
        serde_json::to_string(&sample(message)).expect("error line")
    }

    fn read_lines(path: &Path) -> Vec<String> {
        let mut text = String::new();
        File::open(path)
            .expect("open log")
            .read_to_string(&mut text)
            .expect("read log");
        text.lines().map(|line| line.to_string()).collect()
    }

    /// Recording must never change control flow. The failure arm writes one line
    /// into the build dir's output tree — that is where `log_dir()` resolves
    /// during tests, not the user's install. That recording *happens* is covered
    /// by the file tests above; these pin the pass-through contract.
    #[test]
    fn guard_passes_the_result_through_untouched() {
        assert_eq!(guard("test.boundary", "OK_CODE", || Ok::<u32, String>(7)), Ok(7));
        assert_eq!(
            guard("test.boundary", "ERR_CODE", || Err::<u32, String>("boom".into())),
            Err("boom".to_string())
        );
    }

    #[tokio::test]
    async fn guard_async_passes_the_result_through_untouched() {
        assert_eq!(
            guard_async("test.boundary", "OK_CODE", async { Ok::<u32, String>(7) }).await,
            Ok(7)
        );
        assert_eq!(
            guard_async("test.boundary", "ERR_CODE", async { Err::<u32, String>("boom".into()) }).await,
            Err("boom".to_string())
        );
    }

    /// This is why the API takes a closure rather than a computed `Result`: a `?`
    /// in the body must not be able to skip the recording call.
    #[test]
    fn guard_still_records_when_the_body_uses_the_question_mark_operator() {
        fn fallible() -> Result<u32, String> {
            Err("raised with ?".to_string())
        }

        // Shaped like a real command body: do fallible work, then produce a value.
        let result = guard("test.boundary", "QUESTION_MARK_FAILED", || {
            let value = fallible()?;
            Ok(value + 1)
        });

        assert_eq!(result, Err("raised with ?".to_string()));
    }

    #[test]
    fn the_log_sits_under_the_managed_output_root() {
        // The support workflow ("open the folder and send me that file") depends
        // on this being a predictable place, so pin the shape of it. Deliberately
        // not `assert_eq!(dir, resolve_output_dir().join(LOG_DIR_NAME))`: that
        // compares two independent writability probes, and a concurrent cargo run
        // pruning `target/` can make them disagree (exe-relative root vs the
        // platform fallback) for no real reason.
        let dir = log_dir();
        assert_eq!(dir.file_name().and_then(|name| name.to_str()), Some(LOG_DIR_NAME));
        assert_eq!(log_path(), dir.join(LOG_FILE_NAME));
        let root = dir.parent().expect("log dir has a parent");
        assert_eq!(root.file_name().and_then(|name| name.to_str()), Some("output"));
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
    fn an_unusable_directory_disables_the_log_without_panicking() {        // A file where a directory is expected: create_dir_all fails fatally.
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

    // ── the three death-visibility layers ───────────────────────────────

    #[test]
    fn the_newest_session_marker_decides_whether_a_run_ended_cleanly() {
        let start = session_line(SESSION_START_CODE, 111, "2026-10-10T21:00:00.000+08:00");
        let stop = session_line(SESSION_STOP_CODE, 111, "2026-10-10T21:05:00.000+08:00");

        // A start with nothing after it: the process never reached shutdown.
        match last_session_marker(&start).expect("marker") {
            SessionMarker::Start { pid, .. } => assert_eq!(pid, 111),
            _ => panic!("a lone start must read as a start"),
        }

        // Errors after the start do not confuse it; the older start still owns
        // the "unfinished" verdict.
        let text = format!("{start}\n{}\n{}", sample_line("later error"), sample_line("another"));
        assert!(matches!(last_session_marker(&text), Some(SessionMarker::Start { .. })));

        // A stop after a start: clean, regardless of anything older.
        let text = format!("{start}\n{stop}");
        assert!(matches!(last_session_marker(&text), Some(SessionMarker::Stop)));

        // Nothing but errors.
        assert!(last_session_marker(&sample_line("only an error")).is_none());
        // Malformed session lines are skipped, not fatal.
        let text = format!("{start}\n{}bad json", "{\"level\":\"session\"");
        assert!(matches!(last_session_marker(&text), Some(SessionMarker::Start { .. })));
    }

    /// The detector is only useful if it agrees with the writer about the
    /// on-disk format; `session_line` serializes the real `Entry`.
    #[test]
    fn session_lines_are_filterable_from_errors_by_level() {
        let line = session_line(SESSION_START_CODE, 7, "2026-10-10T21:00:00.000+08:00");
        let value: serde_json::Value = serde_json::from_str(&line).expect("json");
        assert_eq!(value["level"], "session");
        assert_eq!(value["scope"], "app.session");
        assert_eq!(value["build"], "cuda");
        assert_eq!(value["pid"], 7);
    }

    /// A session that died is reported from the log contents; one that stopped
    /// cleanly is not; and our own marker (pid == current) never reports itself.
    #[test]
    fn an_unfinished_session_from_another_process_is_detected() {
        let dir = scratch_dir("unfinished");
        let start = session_line(SESSION_START_CODE, 999_999, "2026-10-10T21:00:00.000+08:00");
        fs::write(active_path(&dir), format!("{start}\n")).expect("write log");

        let previous = previous_session_left_unfinished_in(&dir).expect("unfinished session");
        assert_eq!(previous.pid, 999_999);
        assert_eq!(previous.ts, "2026-10-10T21:00:00.000+08:00");

        let stop = session_line(SESSION_STOP_CODE, 999_999, "2026-10-10T21:05:00.000+08:00");
        fs::write(active_path(&dir), format!("{start}\n{stop}\n")).expect("write log");
        assert!(
            previous_session_left_unfinished_in(&dir).is_none(),
            "a clean shutdown must not be reported as an unexpected exit"
        );
    }

    /// The phase trail: a plain string beside the log, deleted on a clean
    /// shutdown so a leftover file can only mean "died mid-stage".
    #[test]
    fn the_phase_trail_reads_back_and_clears() {
        let dir = scratch_dir("phase");
        assert!(read_phase_in(&dir).is_none(), "nothing written yet");

        fs::write(dir.join(PHASE_FILE_NAME), "ep12:translating").expect("write phase");
        assert_eq!(read_phase_in(&dir).as_deref(), Some("ep12:translating"));

        fs::write(dir.join(PHASE_FILE_NAME), "   ").expect("blank phase");
        assert!(read_phase_in(&dir).is_none(), "blank is not a phase");
    }

    /// A crash report is promoted once and then deleted, and it carries the
    /// phase the dying process recorded.
    #[test]
    fn crash_reports_are_promoted_once_and_then_removed() {
        let dir = scratch_dir("crash");
        let report = dir.join(format!("{CRASH_FILE_PREFIX}{}.txt", 1234));
        fs::write(&report, "code=0xC0000005 address=0x00007FF612345678 pid=1234 phase=ep12:recognizing")
            .expect("write report");
        // An unrelated file must not be swept up.
        fs::write(dir.join("app.log"), "{}").expect("decoy");

        let reports = take_crash_reports_in(&dir);
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].phase.as_deref(), Some("ep12:recognizing"));
        assert!(reports[0].summary().contains("ep12:recognizing"));
        assert!(reports[0].summary().contains("0xC0000005"));
        assert!(!report.exists(), "a promoted report must not repeat next start");
        assert!(dir.join("app.log").exists());
        assert!(take_crash_reports_in(&dir).is_empty(), "nothing left to promote");
    }

    /// The crash handler must not allocate; its sink truncates instead.
    #[cfg(windows)]
    #[test]
    fn the_crash_handler_buffer_truncates_instead_of_growing() {
        use std::fmt::Write as _;
        let mut buffer = [0u8; 16];
        let mut sink = native_crash::FixedBuffer::new(&mut buffer);
        write!(sink, "0123456789abcdefghij").expect("write");
        assert_eq!(sink.len, 16, "writes past the end are dropped, not grown");
        assert_eq!(&sink.buffer[..sink.len], b"0123456789abcdef");
    }

    /// What the dying process leaves on disk: the exception code, the faulting
    /// address and the phase, on one line, with no heap involved. The next start
    /// parses exactly this shape (see the promotion test above).
    #[cfg(windows)]
    #[test]
    fn a_crash_report_carries_code_address_and_phase() {
        let mut buffer = [0u8; 192];
        let len = native_crash::format_report(
            0xC000_0005,
            0x0000_7FF6_1234_5678,
            Some("ep12:recognizing"),
            &mut buffer,
        );
        let line = std::str::from_utf8(&buffer[..len]).expect("ascii");
        assert!(line.starts_with("code=0xC0000005 address=0x00007FF612345678"), "{line}");
        assert!(line.contains("phase=ep12:recognizing"), "{line}");

        // Without a phase (the crash happened before any task stage) the report
        // is still written, just shorter.
        let len = native_crash::format_report(0xC000_0005, 0x10, None, &mut buffer);
        let line = std::str::from_utf8(&buffer[..len]).expect("ascii");
        assert!(!line.contains("phase="), "{line}");
    }

    /// Installing the handler must be safe to call on any thread and must not
    /// disturb a healthy process (the filter only runs on a fatal fault).
    #[cfg(windows)]
    #[test]
    fn installing_the_crash_handler_is_harmless() {
        install_crash_handler();
    }

    /// The glue `main.rs` calls, against a scratch directory: a start marker is
    /// written with the session level, and the marker a previous session left can
    /// then be read back as "unfinished". No global log state, and nothing under
    /// the real log dir, which a concurrent cargo run may be pruning.
    #[test]
    fn the_session_glue_writes_a_filterable_stop_marker() {
        let dir = scratch_dir("session_glue");
        let mut state = State::default();
        assert!(state.ensure_open_in(&dir));

        let start = session_marker_entry(SESSION_START_CODE, "session started");
        write_entry_in(&dir, &mut state, &start).expect("write start");
        // A start alone reads as an unfinished session.
        assert!(matches!(
            last_session_marker(&fs::read_to_string(active_path(&dir)).expect("log")),
            Some(SessionMarker::Start { .. })
        ));

        let stop = session_marker_entry(SESSION_STOP_CODE, "clean shutdown");
        write_entry_in(&dir, &mut state, &stop).expect("write stop");
        let text = fs::read_to_string(active_path(&dir)).expect("log");
        assert!(matches!(last_session_marker(&text), Some(SessionMarker::Stop)));
        // The stop must be filterable the same way the start is.
        let last_line = text.lines().last().expect("line");
        let value: serde_json::Value = serde_json::from_str(last_line).expect("json");
        assert_eq!(value["level"], SESSION_LEVEL);
        assert_eq!(value["code"], SESSION_STOP_CODE);
        assert_eq!(value["scope"], SESSION_SCOPE);
    }
}
