//! Tauri command entrypoints grouped by domain.
//!
//! Boundary:
//! - `transcribe`: ASR execution and low-level segmentation command.
//! - `transcription`: post-ASR pipeline orchestration and phase events.
pub mod file;
pub mod history;
pub mod logs;
pub mod model;
pub mod preferences;
pub mod subtitle;
pub mod system;
pub mod transcribe;
pub mod transcription;
pub mod workspace;
