//! Backend service modules.
//!
//! Boundary:
//! - `transcribe`: ASR runtime calls and token-level output.
//! - `transcription`: step2 sentence grouping pipeline.
pub mod binary;
pub mod demucs;
pub mod file;
pub mod file_download;
pub mod llm;
pub mod logs;
pub mod model;
pub mod output;
pub mod pipeline;
pub mod preferences;
mod preferences_normalize;
mod preferences_types;
pub mod prompts;
pub mod subtitle_beautify;
pub mod subtitle_srt;
pub mod subtitle_step5;
pub mod system;
pub mod task_log;
pub mod task_path;
pub mod task_usage;
pub mod terminology;
mod terminology_responses;
mod terminology_terms;
mod terminology_text;
pub mod transcribe;
pub mod transcription;
pub mod translation;
pub mod updater;
pub mod workspace_subtitle;
pub mod youtube;
