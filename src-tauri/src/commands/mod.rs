//! Tauri command entrypoints grouped by domain.
//!
//! Boundary:
//! - `transcribe`: ASR execution and low-level segmentation command.
//! - `transcription`: post-ASR pipeline orchestration and phase events.
pub mod file;
pub mod logs;
pub mod model;
pub mod preferences;
pub mod subtitle;
pub mod system;
pub mod task_engine;
pub mod translate;
pub mod transcribe;
pub mod transcription;
pub mod workspace;
pub mod youtube;
pub mod updater;
pub(crate) mod dto;
