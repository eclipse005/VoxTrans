//! One configuration resolution for every verb: the app's saved settings, then
//! flag overrides.
//!
//! The desktop app and the CLI read the *same* SQLite settings row
//! (`settings` table in `<app data>/com.voxtrans.desktop/voxtrans.db`), resolved
//! through the *same* function
//! ([`crate::commands::workspace::runtime_settings::runtime_from_saved`]) so a
//! model swap, a new API key or a terminology group edited in the GUI is picked
//! up by the CLI with no second configuration file to keep in sync. Flags
//! override individual fields for a single run and are never persisted.

use clap::Args;
use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::commands::translate_types::TranslateTerminologyEntryCommand;
use crate::commands::workspace::runtime_settings::{
    FrozenSettings, PipelineRuntimeSettings, runtime_from_saved,
};
use crate::db::conversion::TaskMetaExtras;
use crate::db::store::TaskStore;
use crate::domain::pipeline::UnitStore;
use crate::domain::preferences_normalize::{default_settings, normalize_saved_settings};
use crate::domain::preferences_types::{Provider, SavedSettings, SubtitleLengthPreset};
use crate::domain::workspace::{WorkspaceQueueItem, WorkspaceTaskProgressState};
use crate::services::model::set_models_root_override;
use crate::services::output::set_output_root_override;
use crate::services::task_log::TaskLogger;

use super::CliResult;
use super::artifacts::RUN_JSON;

/// `--db auto`: the desktop app's database.
pub const DB_AUTO: &str = "auto";
/// `--db none`: run with built-in defaults, touch no database.
pub const DB_NONE: &str = "none";

/// Bundle identifier from `tauri.conf.json` — the leaf directory Tauri's
/// `app_data_dir()` appends to the platform data directory.
const APP_IDENTIFIER: &str = "com.voxtrans.desktop";

#[derive(Debug, Clone, Args)]
pub struct CommonArgs {
    /// Settings database. `auto` = the desktop app's database, `none` = built-in
    /// defaults. Any other value is a path to a `voxtrans.db`.
    #[arg(long, value_name = "PATH|auto|none", default_value = DB_AUTO)]
    pub db: String,

    /// Register this run under an explicit task id. Enables GUI-compatible side
    /// effects: the task row is created (visible in the app's task list),
    /// per-unit resume state is read and written, token usage is recorded.
    /// Without it the run is isolated: no database writes at all.
    #[arg(long, value_name = "ID")]
    pub task: Option<String>,

    /// Output directory for artifacts, logs and SRTs.
    /// Default: `./voxtrans-out/<task id>`.
    #[arg(long, value_name = "DIR")]
    pub out: Option<PathBuf>,

    /// Ignore and do not write per-unit resume state (`--task` runs only).
    /// Use this for A/B runs, where a cached batch would silently answer for
    /// the model.
    #[arg(long)]
    pub no_cache: bool,

    /// Source language code (e.g. `en`, `zh`). Default `en` — the same seed the
    /// app gives a new task. Unlike models/endpoint, source and target are
    /// per-task fields, not saved settings, so there is nothing to read back.
    #[arg(long, value_name = "LANG")]
    pub source: Option<String>,

    /// Target language code (e.g. `zh-CN`, `en`). Default `zh-CN` (the app's
    /// new-task seed).
    #[arg(long, value_name = "LANG")]
    pub target: Option<String>,

    // ── ASR / alignment ────────────────────────────────────────────────
    /// ASR model name (e.g. `Qwen3-ASR-0.6B`).
    #[arg(long, value_name = "MODEL")]
    pub asr_model: Option<String>,

    /// Forced-alignment model name.
    #[arg(long, value_name = "MODEL")]
    pub align_model: Option<String>,

    /// Execution provider: `cpu` or `cuda`.
    #[arg(long, value_name = "PROVIDER")]
    pub provider: Option<String>,

    /// ASR chunk target length in seconds (clamped to 30..180).
    #[arg(long, value_name = "SECONDS")]
    pub chunk_seconds: Option<u32>,

    /// Model storage directory (overrides the saved setting).
    #[arg(long, value_name = "DIR")]
    pub models_dir: Option<PathBuf>,

    /// Force vocal separation on (`--vocal-separation`) or off before ASR.
    #[arg(long)]
    pub vocal_separation: bool,

    #[arg(long, conflicts_with = "vocal_separation")]
    pub no_vocal_separation: bool,

    // ── LLM (terminology + translation) ─────────────────────────────────
    /// LLM API key. Overrides the saved setting; never written to disk.
    #[arg(long, value_name = "KEY")]
    pub api_key: Option<String>,

    /// LLM base URL (OpenAI- or Anthropic-compatible endpoint).
    #[arg(long, value_name = "URL")]
    pub base_url: Option<String>,

    /// LLM model name.
    #[arg(long, value_name = "MODEL")]
    pub model: Option<String>,

    /// Parallel LLM requests (clamped to 1..16).
    #[arg(long, value_name = "N")]
    pub concurrency: Option<u32>,

    /// Translation units per LLM call (default: the source-of-truth constant
    /// `services::translation::DEFAULT_BATCH_SIZE`).
    #[arg(long, value_name = "N")]
    pub batch_size: Option<usize>,

    /// Extra terminology as a JSON file: `[{"source":"…","target":"…","note":"…"}]`
    /// (`origin`/`target` keys are accepted too). Entries here win over the
    /// saved group's entries with the same source.
    #[arg(long, value_name = "FILE")]
    pub terms: Option<PathBuf>,

    /// Saved terminology group id to use. Default: the active group.
    #[arg(long, value_name = "ID")]
    pub terms_group: Option<String>,

    /// Style guide text passed to the translator (file contents).
    #[arg(long, value_name = "FILE")]
    pub style_guide: Option<PathBuf>,

    // ── Subtitle shaping ────────────────────────────────────────────────
    /// Subtitle length preset: `short`, `standard`, `loose`.
    #[arg(long, value_name = "PRESET")]
    pub preset: Option<String>,

    /// Force subtitle beautify on (`--beautify`) or off (`--no-beautify`).
    #[arg(long)]
    pub beautify: bool,

    #[arg(long, conflicts_with = "beautify")]
    pub no_beautify: bool,

    // ── Output ──────────────────────────────────────────────────────────
    /// Print the run report as JSON on stdout (progress still goes to stderr).
    #[arg(long)]
    pub json: bool,

    /// Silence progress lines on stderr.
    #[arg(long)]
    pub quiet: bool,
}

/// Resolved configuration for one run.
///
/// Everything a verb needs, resolved once, in one place. Fields are plain
/// values (not `Option`) because a verb may not open-code its own fallbacks:
/// the same precedence has to apply to every stage of the pipeline, and every
/// run has to be reproducible from `run.json`.
pub struct CliContext {
    pub verb: &'static str,
    pub task_id: String,
    /// Input file this run reads (media, SRT or stage artifact). May be empty
    /// for runs that only take flags.
    pub input_path: PathBuf,
    /// `input_path` as a string — the form the services take.
    pub input_path_str: String,
    pub out_dir: PathBuf,
    pub db_path: Option<PathBuf>,
    /// Where the settings came from, for the report (`db:<path>` / `defaults`).
    pub settings_source: String,
    /// `Some` once a database is open. Writes (task row, unit resume state,
    /// token usage) only happen when a task id was supplied.
    pub store: Option<TaskStore>,
    /// True when this run may write to the database (explicit `--task`).
    pub task_registered: bool,
    pub settings: SavedSettings,
    pub frozen: FrozenSettings,
    pub runtime: PipelineRuntimeSettings,
    pub source_lang: String,
    pub target_lang: String,
    pub batch_size: usize,
    pub user_terms: Vec<TranslateTerminologyEntryCommand>,
    pub style_guide: String,
    pub json: bool,
    pub quiet: bool,
    pub no_cache: bool,
    pub started: Instant,
    /// Progress lines already printed, keyed by `phase\u{1}done/total\u{1}detail`.
    /// The translate pipeline emits an update per token stream frame and the
    /// ASR stage emits one at both segment start and completion, so without
    /// this every batch prints the same `0/2` line dozens of times (a
    /// 49-minute video then buries the real progress in thousands of lines).
    printed_progress: std::sync::Mutex<std::collections::HashSet<String>>,
}

impl std::fmt::Debug for CliContext {
    /// Hand-written rather than derived: `settings` and `runtime` carry the LLM
    /// API key, and a derived `Debug` would print it the first time anyone logs
    /// a context (the app masks the key for the same reason, see
    /// `execution_flow::execute_single_task_inner`).
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CliContext")
            .field("verb", &self.verb)
            .field("task_id", &self.task_id)
            .field("input_path", &self.input_path)
            .field("out_dir", &self.out_dir)
            .field("db_path", &self.db_path)
            .field("settings_source", &self.settings_source)
            .field("task_registered", &self.task_registered)
            .field("source_lang", &self.source_lang)
            .field("target_lang", &self.target_lang)
            .field("batch_size", &self.batch_size)
            .field("model", &self.runtime.translate_model)
            .field("concurrency", &self.runtime.llm_concurrency)
            .finish()
    }
}

/// What a verb asks of the shared context.
pub struct ContextRequest<'a> {
    pub verb: &'static str,
    pub common: &'a CommonArgs,
    /// Input path, used for the task folder/log placement and defaults.
    pub input_path: Option<&'a Path>,
    /// Fail now (not three stages later) when the LLM connection settings are
    /// missing — the terminology/translation verbs need them.
    pub require_llm: bool,
}

impl CliContext {
    pub async fn new(req: ContextRequest<'_>) -> CliResult<Self> {
        let common = req.common;
        let started = Instant::now();
        let task_id = common
            .task
            .clone()
            .unwrap_or_else(|| format!("cli-{}", chrono::Local::now().format("%Y%m%d-%H%M%S")));
        let input_path = req
            .input_path
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from(""));
        let input_path_str = input_path.to_string_lossy().to_string();

        // Output root first: every path helper below (task folder, task logs,
        // flat SRT exports) resolves through it.
        let out_dir = resolve_out_dir(common, &task_id)?;
        std::fs::create_dir_all(&out_dir)
            .map_err(|e| format!("failed to create output dir {}: {e}", out_dir.display()))?;
        set_output_root_override(Some(out_dir.clone()));

        // ── Settings: the app's database, then flags ────────────────────
        let (store, db_path) = open_store(&common.db).await?;
        let (mut settings, settings_source) = match &store {
            Some(store) => {
                let settings = crate::services::preferences::load_saved_settings_from_default_path(
                    store,
                )?;
                (
                    settings,
                    format!("db:{}", db_path.as_ref().map(|p| p.display().to_string()).unwrap_or_default()),
                )
            }
            None => (normalize_saved_settings(default_settings()), "defaults".to_string()),
        };
        apply_overrides(&mut settings, common);

        if let Some(models_dir) = settings
            .models_dir
            .as_deref()
            .map(str::trim)
            .filter(|dir| !dir.is_empty())
        {
            set_models_root_override(Some(PathBuf::from(models_dir)));
        }

        // ── Runtime settings (frozen vs live, requirement checks) ───────
        let terms_group_id = common
            .terms_group
            .clone()
            .unwrap_or_else(|| settings.active_terminology_group_id.clone());
        let frozen = FrozenSettings::from_saved(&settings, &terms_group_id);
        let runtime = runtime_from_saved(&settings, &frozen, req.require_llm)?;
        let user_terms = merge_user_terms(&runtime.terminology_entries, common.terms.as_deref())?;
        let style_guide = read_optional_text(common.style_guide.as_deref())?;

        let source_lang = common
            .source
            .clone()
            .unwrap_or_else(|| DEFAULT_SOURCE_LANG.to_string());
        let target_lang = common
            .target
            .clone()
            .unwrap_or_else(|| DEFAULT_TARGET_LANG.to_string());

        let mut ctx = Self {
            verb: req.verb,
            task_id,
            input_path,
            input_path_str,
            out_dir,
            db_path,
            settings_source,
            store,
            task_registered: false,
            settings,
            frozen,
            runtime,
            source_lang,
            target_lang,
            batch_size: common
                .batch_size
                .unwrap_or(crate::services::translation::DEFAULT_BATCH_SIZE),
            user_terms,
            style_guide,
            json: common.json,
            quiet: common.quiet,
            no_cache: common.no_cache,
            started,
            printed_progress: std::sync::Mutex::new(std::collections::HashSet::new()),
        };

        // A `--task` run owns a row in `tasks`: the per-unit resume tables and
        // `task_usage` rows are foreign-keyed to it, and the app must be able to
        // show (and later reopen) what the CLI produced.
        if common.task.is_some() {
            if let Some(store) = ctx.store.clone() {
                register_task(&store, &ctx).await?;
                ctx.task_registered = true;
            } else {
                return Err(
                    "--task needs a settings database; pass --db <path> (or drop --task)".to_string(),
                );
            }
        }

        Ok(ctx)
    }

    /// Per-unit resume store — `None` when this run must stay isolated
    /// (no database, no `--task`) or was asked to ignore caches (`--no-cache`).
    pub fn unit_store(&self) -> Option<UnitStore> {
        if self.no_cache || !self.task_registered {
            return None;
        }
        self.store
            .as_ref()
            .map(|store| UnitStore::new(store, &self.task_id))
    }

    /// Task folder, matching the desktop app's layout:
    /// `<out>/<input stem>_<task id>/`.
    pub fn task_dir(&self) -> PathBuf {
        let media = PathBuf::from(&self.input_path_str);
        crate::services::task_path::task_output_dir(&self.task_id, &media)
    }

    /// Directory holding `main.log` / `llm.log` for this run.
    pub fn log_dir(&self) -> PathBuf {
        let media = PathBuf::from(&self.input_path_str);
        crate::services::task_path::task_log_dir(&self.task_id, Some(&media))
    }

    pub fn logger_main(&self) -> TaskLogger {
        TaskLogger::main_with_media(self.task_id.clone(), self.input_path_str.clone())
    }

    pub fn logger_llm(&self) -> TaskLogger {
        TaskLogger::llm_with_media(self.task_id.clone(), self.input_path_str.clone())
    }

    /// Path of an artifact inside the output directory.
    pub fn out_path(&self, name: &str) -> PathBuf {
        self.out_dir.join(name)
    }

    /// Write an artifact as pretty JSON, reporting the path.
    pub fn write_json<T: serde::Serialize>(&self, name: &str, value: &T) -> CliResult<PathBuf> {
        let path = self.out_path(name);
        let json = serde_json::to_string_pretty(value)
            .map_err(|e| format!("failed to serialize {name}: {e}"))?;
        std::fs::write(&path, json)
            .map_err(|e| format!("failed to write {}: {e}", path.display()))?;
        Ok(path)
    }

    pub fn read_json<T: serde::de::DeserializeOwned>(&self, path: &Path) -> CliResult<T> {
        let raw = std::fs::read_to_string(path)
            .map_err(|e| format!("failed to read {}: {e}", path.display()))?;
        serde_json::from_str(&raw).map_err(|e| format!("failed to parse {}: {e}", path.display()))
    }

    /// Progress line on stderr — never stdout, so `--json` stays parseable.
    /// Fixed shape (`phase done/total detail`) so logs can be grepped, and
    /// **printed once per distinct line**: the streaming callbacks fire many
    /// times with an unchanged counter, and only the counter's movement is
    /// progress. A poisoned lock prints rather than panics — this is a progress
    /// meter, losing deduplication must never take down a run.
    pub fn progress(&self, phase: &str, done: usize, total: usize, detail: &str) {
        if self.quiet {
            return;
        }
        let detail = detail.trim();
        let key = format!("{phase}\u{1}{done}/{total}\u{1}{detail}");
        if let Ok(mut printed) = self.printed_progress.lock()
            && !printed.insert(key)
        {
            return;
        }
        if total > 0 {
            if detail.is_empty() {
                eprintln!("[{phase}] {done}/{total}");
            } else {
                eprintln!("[{phase}] {done}/{total} {detail}");
            }
        } else if detail.is_empty() {
            eprintln!("[{phase}]");
        } else {
            eprintln!("[{phase}] {detail}");
        }
    }

    pub fn note(&self, message: &str) {
        if !self.quiet {
            eprintln!("{message}");
        }
    }

    pub fn warn(&self, message: &str) {
        // Warnings survive `--quiet`: they are part of the result, not progress.
        eprintln!("warning: {message}");
    }

    pub fn report_path(&self) -> PathBuf {
        self.out_path(RUN_JSON)
    }
}

/// `--out`, defaulting to `./voxtrans-out/<task id>` in the current directory.
/// Relative paths resolve against the CWD (not the executable) because a CLI is
/// expected to write where it was invoked.
fn resolve_out_dir(common: &CommonArgs, task_id: &str) -> CliResult<PathBuf> {
    let raw = match &common.out {
        Some(dir) => dir.clone(),
        None => PathBuf::from("voxtrans-out").join(task_id),
    };
    if raw.is_absolute() {
        return Ok(raw);
    }
    let cwd = std::env::current_dir().map_err(|e| format!("failed to read cwd: {e}"))?;
    Ok(cwd.join(raw))
}

/// Open the settings database. `auto` resolves the app's database and is not an
/// error when it is missing (a fresh machine has no saved settings yet).
async fn open_store(db: &str) -> CliResult<(Option<TaskStore>, Option<PathBuf>)> {
    let trimmed = db.trim();
    if trimmed.eq_ignore_ascii_case(DB_NONE) {
        return Ok((None, None));
    }
    let path = if trimmed.eq_ignore_ascii_case(DB_AUTO) {
        match default_db_path() {
            Some(path) if path.is_file() => path,
            _ => return Ok((None, None)),
        }
    } else {
        let path = PathBuf::from(trimmed);
        if !path.is_file() {
            return Err(format!("settings database not found: {}", path.display()));
        }
        path
    };
    let pool = crate::db::open_pool_at(&path).await?;
    Ok((Some(TaskStore::new(pool)), Some(path)))
}

/// The desktop app's database path for this bundle identifier — the same leaf
/// `tauri::Manager::path().app_data_dir()` produces, resolved without a Tauri
/// handle (there is no app instance in a CLI process).
pub fn default_db_path() -> Option<PathBuf> {
    let base = platform_data_dir()?;
    Some(base.join(APP_IDENTIFIER).join("voxtrans.db"))
}

fn platform_data_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("APPDATA")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    }

    #[cfg(target_os = "macos")]
    {
        std::env::var_os("HOME")
            .filter(|value| !value.is_empty())
            .map(|home| PathBuf::from(home).join("Library").join("Application Support"))
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::env::var_os("XDG_DATA_HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME")
                    .filter(|value| !value.is_empty())
                    .map(|home| PathBuf::from(home).join(".local").join("share"))
            })
    }
}

/// Flag overrides, applied in flag order: explicit wins, nothing is persisted.
fn apply_overrides(settings: &mut SavedSettings, common: &CommonArgs) {
    if let Some(asr) = &common.asr_model {
        settings.asr_model = crate::domain::preferences_types::AsrModel::parse(asr);
    }
    if let Some(align) = &common.align_model {
        settings.align_model = crate::domain::preferences_types::AlignModel::parse(align);
    }
    if let Some(provider) = &common.provider {
        settings.provider = Provider::parse(provider);
    }
    if let Some(seconds) = common.chunk_seconds {
        settings.chunk_target_seconds = seconds;
    }
    if let Some(dir) = &common.models_dir {
        settings.models_dir = Some(dir.to_string_lossy().to_string());
    }
    if common.vocal_separation {
        settings.enable_vocal_separation = true;
    }
    if common.no_vocal_separation {
        settings.enable_vocal_separation = false;
    }
    if let Some(key) = &common.api_key {
        settings.translate_api_key = key.clone();
    }
    if let Some(url) = &common.base_url {
        settings.translate_base_url = url.clone();
    }
    if let Some(model) = &common.model {
        settings.translate_model = model.clone();
    }
    if let Some(concurrency) = common.concurrency {
        settings.llm_concurrency = concurrency;
    }
    if let Some(preset) = &common.preset {
        settings.subtitle_length_preset = SubtitleLengthPreset::parse(preset);
    }
    if common.beautify {
        settings.enable_subtitle_beautify = true;
    }
    if common.no_beautify {
        settings.enable_subtitle_beautify = false;
    }
}

/// Saved-group entries plus `--terms` file entries (file wins on the same
/// normalized source), mirroring how the app flattens a frozen group.
fn merge_user_terms(
    group_entries: &[TranslateTerminologyEntryCommand],
    terms_file: Option<&Path>,
) -> CliResult<Vec<TranslateTerminologyEntryCommand>> {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct TermsFileEntry {
        #[serde(alias = "origin", alias = "sourceText")]
        source: String,
        #[serde(alias = "targetText")]
        target: String,
        #[serde(default)]
        note: String,
    }

    let mut merged: Vec<TranslateTerminologyEntryCommand> = Vec::new();
    for entry in group_entries {
        merged.push(entry.clone());
    }

    if let Some(path) = terms_file {
        let raw = std::fs::read_to_string(path)
            .map_err(|e| format!("failed to read terms file {}: {e}", path.display()))?;
        let entries: Vec<TermsFileEntry> = serde_json::from_str(&raw)
            .map_err(|e| format!("failed to parse terms file {}: {e}", path.display()))?;
        for entry in entries {
            let source = entry.source.trim();
            let target = entry.target.trim();
            if source.is_empty() || target.is_empty() {
                continue;
            }
            let key = source.to_lowercase();
            merged.retain(|existing| existing.source.trim().to_lowercase() != key);
            merged.push(TranslateTerminologyEntryCommand {
                source: source.to_string(),
                target: target.to_string(),
                note: entry.note.trim().to_string(),
            });
        }
    }

    Ok(merged)
}

fn read_optional_text(path: Option<&Path>) -> CliResult<String> {
    match path {
        Some(path) => std::fs::read_to_string(path)
            .map(|text| text.trim().to_string())
            .map_err(|e| format!("failed to read {}: {e}", path.display())),
        None => Ok(String::new()),
    }
}

/// Insert the task row a `--task` run needs, unless the id already exists (an
/// existing row — most likely a GUI task — is reused as-is and never
/// overwritten with CLI bookkeeping).
async fn register_task(store: &TaskStore, ctx: &CliContext) -> CliResult<()> {
    if store.task_exists(&ctx.task_id).await? {
        return Ok(());
    }

    let name = Path::new(&ctx.input_path_str)
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .filter(|stem| !stem.is_empty())
        .unwrap_or_else(|| ctx.task_id.clone());
    let size_bytes = std::fs::metadata(&ctx.input_path_str).map(|m| m.len()).unwrap_or(0);

    let item = WorkspaceQueueItem {
        id: ctx.task_id.clone(),
        path: ctx.input_path_str.clone(),
        name,
        // `cli` (not video/audio/subtitle): the app's queue never claims it, and
        // it stays distinguishable from GUI-created work in the task list.
        media_kind: "cli".to_string(),
        size_bytes,
        source_lang: ctx.source_lang.clone(),
        target_lang: ctx.target_lang.clone(),
        transcribe_status: "done".to_string(),
        task_progress: WorkspaceTaskProgressState::default(),
        transcribe_error: String::new(),
        result_text: String::new(),
        result_srt: String::new(),
        subtitle_segments_json: String::new(),
        llm_total_tokens: 0,
        terminology_group_id: ctx
            .frozen
            .terminology_groups
            .first()
            .map(|group| group.id.clone())
            .unwrap_or_default(),
        review_source: false,
        review_target: false,
        resume_from: String::new(),
    };
    let extras = TaskMetaExtras {
        intent: format!("CLI_{}", ctx.verb.to_uppercase()),
        max_retries: 0,
        subtitle_length_preset: ctx.settings.subtitle_length_preset.as_str().to_string(),
        enable_subtitle_beautify: ctx.settings.enable_subtitle_beautify,
        terminology_groups_json: serde_json::to_string(&ctx.frozen.terminology_groups)
            .unwrap_or_else(|_| "[]".to_string()),
        enqueue_seq: store.next_enqueue_seq().await?,
    };
    store.upsert_task(&item, &extras).await
}

/// Default task languages, matching what the app seeds a new task with
/// (`WorkspaceQueueItem`'s serde defaults: `en` → `zh-CN`).
const DEFAULT_SOURCE_LANG: &str = "en";
const DEFAULT_TARGET_LANG: &str = "zh-CN";

#[cfg(test)]
mod tests {
    use super::*;

    /// Run `body` and reset the process-wide output root afterwards, even when
    /// the body fails the way the negative tests expect. `CliContext::new` pins
    /// that root (`--out`) — correct for the CLI process, but global state: a
    /// test that resolves the output dir afterwards (app log paths, task
    /// folders) would otherwise see this run's directory. The first test that
    /// forgot this reset broke an unrelated app-log test, so the reset lives in
    /// one helper next to the tests that create a context.
    async fn without_output_root<F, Fut, T>(body: F) -> T
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = T>,
    {
        let result = body().await;
        crate::services::output::set_output_root_override(None);
        result
    }

    fn common() -> CommonArgs {
        CommonArgs {
            db: DB_NONE.to_string(),
            task: None,
            out: None,
            no_cache: false,
            source: None,
            target: None,
            asr_model: None,
            align_model: None,
            provider: None,
            chunk_seconds: None,
            models_dir: None,
            vocal_separation: false,
            no_vocal_separation: false,
            api_key: None,
            base_url: None,
            model: None,
            concurrency: None,
            batch_size: None,
            terms: None,
            terms_group: None,
            style_guide: None,
            preset: None,
            beautify: false,
            no_beautify: false,
            json: false,
            quiet: false,
        }
    }

    /// Flags must reach the pipeline runtime, and a `--db none` context must
    /// carry no store (no writes) while still resolving every field.
    #[tokio::test]
    async fn flags_override_settings_and_no_db_means_no_store() {
        let mut args = common();
        args.api_key = Some("test-key".to_string());
        args.base_url = Some("https://example.invalid/v1".to_string());
        args.model = Some("test-model".to_string());
        args.source = Some("ja".to_string());
        args.target = Some("en".to_string());
        args.out = Some(PathBuf::from("target/cli-test-out"));

        let ctx = without_output_root(|| {
            CliContext::new(ContextRequest {
                verb: "test",
                common: &args,
                input_path: Some(Path::new("sample.srt")),
                require_llm: true,
            })
        })
        .await
        .expect("context resolves from defaults + flags");

        assert!(ctx.store.is_none());
        assert!(!ctx.task_registered);
        assert!(ctx.unit_store().is_none());
        assert_eq!(ctx.runtime.translate_model, "test-model");
        assert_eq!(ctx.runtime.translate_api_key, "test-key");
        assert_eq!(ctx.source_lang, "ja");
        assert_eq!(ctx.target_lang, "en");
        assert_eq!(ctx.settings_source, "defaults");
        assert!(ctx.out_dir.ends_with("cli-test-out"));
    }

    /// `require_llm` must fail before any stage runs when the endpoint is
    /// unconfigured — the defaults carry no key.
    #[tokio::test]
    async fn require_llm_fails_without_credentials() {
        let args = common();
        let err = without_output_root(|| {
            CliContext::new(ContextRequest {
                verb: "test",
                common: &args,
                input_path: None,
                require_llm: true,
            })
        })
        .await
        .expect_err("defaults have no api key");
        assert!(err.contains("translateApiKey"), "unexpected error: {err}");
    }

    /// A `--task` id with no database is a usage error, not a silent no-op:
    /// the user asked for resume state that cannot exist.
    #[tokio::test]
    async fn task_id_requires_a_database() {
        let mut args = common();
        args.task = Some("cli-test".to_string());
        args.out = Some(PathBuf::from("target/cli-test-out"));

        let err = without_output_root(|| {
            CliContext::new(ContextRequest {
                verb: "test",
                common: &args,
                input_path: None,
                require_llm: false,
            })
        })
        .await
        .expect_err("--task without a database must fail");
        assert!(err.contains("--task needs a settings database"), "unexpected error: {err}");
    }
}
