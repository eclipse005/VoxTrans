//! Backend service modules.
//!
//! Boundary:
//! - `transcribe`: ASR runtime calls and token-level output.
//! - `transcription`: step2 sentence grouping pipeline.

/// Which engine build this binary is. The CPU and CUDA installers are separate
/// downloads with separate failure modes (the CUDA one fails on drivers, VRAM
/// and PTX mismatch), so every support artifact records which produced it: the
/// updater picks the matching installer asset with it, and the app log stamps
/// it on every entry.
pub(crate) const BUILD_VARIANT: &str = if cfg!(feature = "cuda") { "cuda" } else { "cpu" };

pub mod app_log;
pub mod binary;
pub mod demucs;
pub mod file;
pub mod file_download;
pub mod llm;
pub mod logs;
pub mod model;
pub mod output;
pub mod preferences;
pub mod prompts;
pub mod subtitle_beautify;
pub mod subtitle_import;
pub mod subtitle_length;
pub mod subtitle_render;
pub mod subtitle_srt;
pub mod system;
pub mod task_log;
pub mod task_path;
pub mod terminology_agent;
pub mod task_usage;
pub mod time_utils;
pub mod transcribe;
pub mod transcription;
pub mod translation;
pub mod updater;
pub mod youtube;
