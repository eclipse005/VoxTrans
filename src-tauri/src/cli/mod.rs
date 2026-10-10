//! Headless command-line interface over the same pipeline the desktop app runs.
//!
//! # Why this exists
//!
//! The pipeline has four independently tweakable stages — ASR/alignment,
//! sentence segmentation + beautify, terminology extraction, translation — and
//! the GUI only exposes them as one all-or-nothing task. Iterating on any of
//! them meant re-running the expensive stages (or hand-rolling a throwaway
//! `examples/` binary with hard-coded paths and a temp settings JSON). This
//! module gives each stage its own verb, one shared configuration source (the
//! app's SQLite settings row, overridable by flags) and one machine-readable
//! report per run.
//!
//! # Rules for code in here
//!
//! - **No `AppHandle`.** The CLI exists to run without the GUI process; it
//!   talks to `services::*` (and to `commands::transcription` for the step-2
//!   DTOs) but never to `commands::workspace`, which is AppHandle-shaped.
//! - **Same settings, same rules as the GUI.** Configuration is resolved
//!   through `commands::workspace::runtime_settings::{FrozenSettings,
//!   runtime_from_saved}`, so "what is frozen", "what is required" and how
//!   terminology groups flatten cannot drift between the two front ends.
//! - **Artifacts are the interface.** Each verb writes its stage artifact as
//!   JSON into `--out` and prints a report; the next verb reads that artifact.
//!   That is what makes stage-by-stage iteration possible.
//!
//! # Verbs
//!
//! ```text
//! voxtrans-cli transcribe  <media>       ASR + forced alignment  -> words.json
//! voxtrans-cli segment     <words.json>  step2 + beautify       -> source.json + src.srt
//! voxtrans-cli terminology <input>       terminology agent      -> briefing.json
//! voxtrans-cli translate   <input>       translation layer      -> trans.json + *srt
//! voxtrans-cli pipeline    <media>       all of the above in order
//! ```
//!
//! # Output contract (what makes it scriptable)
//!
//! - **stdout** carries the run result: a one-screen human summary, or the
//!   `run.json` document itself with `--json`.
//! - **stderr** carries progress and diagnostics (`--quiet` silences
//!   progress), so `... --json | jq` never sees a progress line.
//! - **exit code** is `0` on success, `1` on failure, `2` on a usage error
//!   (clap's own exit code).
//! - every run writes `<out>/run.json` — the same document `--json` prints —
//!   with per-stage timings, artifact paths, token usage and warnings.

pub mod artifacts;
pub mod asr_stage;
pub mod context;
pub mod llm_stage;
pub mod pipeline;
pub mod report;

use clap::{Parser, Subcommand};
use std::process::ExitCode;

/// Failures are plain strings, like the rest of the service layer: a CLI error
/// is a message for a human or a model to read, not something to match on.
pub type CliResult<T> = Result<T, String>;

#[derive(Debug, Parser)]
#[command(
    name = "voxtrans-cli",
    version,
    about = "Headless pipeline driver for VoxTrans (ASR, segmentation, terminology, translation)",
    long_about = None,
    // Keep `--help` tidy: subcommand help is where the flags are documented.
    arg_required_else_help = true
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Speech recognition + forced alignment on a media file → words.json
    Transcribe(asr_stage::TranscribeArgs),
    /// Sentence segmentation + subtitle beautify from words.json → source.json
    Segment(asr_stage::SegmentArgs),
    /// Terminology briefing (windowed LLM extraction) → briefing.json
    Terminology(llm_stage::TerminologyArgs),
    /// Translate source cues (SRT / source.json / words.json) → trans.json + SRTs
    Translate(llm_stage::TranslateArgs),
    /// Run the whole pipeline (transcribe → segment → terminology → translate)
    Pipeline(pipeline::PipelineArgs),
}

pub fn main() -> ExitCode {
    let cli = Cli::parse();

    let runtime = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(runtime) => runtime,
        Err(err) => {
            eprintln!("error: failed to start async runtime: {err}");
            return ExitCode::FAILURE;
        }
    };

    match runtime.block_on(dispatch(cli.command)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            // The failing verb already wrote its error into run.json (when it
            // got far enough to have an output directory); this is the
            // human-facing copy.
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}

async fn dispatch(command: Command) -> CliResult<()> {
    match command {
        Command::Transcribe(args) => asr_stage::run_transcribe(args).await,
        Command::Segment(args) => asr_stage::run_segment(args).await,
        Command::Terminology(args) => llm_stage::run_terminology(args).await,
        Command::Translate(args) => llm_stage::run_translate(args).await,
        Command::Pipeline(args) => pipeline::run_pipeline(args).await,
    }
}
