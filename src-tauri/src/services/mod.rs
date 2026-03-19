//! Backend service modules.
//!
//! Boundary:
//! - `transcribe`: ASR runtime calls and token-level output.
//! - `transcription`: post-ASR processing pipeline (segment/srt generation phases).
pub mod file;
pub mod demucs;
pub mod history;
pub mod logs;
pub mod model;
pub mod output;
pub mod preferences;
pub mod subtitle;
pub mod system;
pub mod task_log;
pub mod task_engine;
pub mod task_executor;
pub mod task_path;
pub mod translate;
pub mod transcribe;
pub mod transcription;
pub mod workspace;
