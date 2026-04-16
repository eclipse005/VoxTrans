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
pub mod system;
pub mod task_log;
pub mod task_path;
pub mod task_usage;
pub mod terminology;
pub mod transcribe;
pub mod transcription;
pub mod translation;
pub mod updater;
