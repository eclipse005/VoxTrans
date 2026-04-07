//! Backend service modules.
//!
//! Boundary:
//! - `transcribe`: ASR runtime calls and token-level output.
//! - `transcription`: post-ASR processing pipeline (segment/srt generation phases).
pub mod file;
pub mod binary;
pub mod demucs;
pub mod final_subtitle;
pub mod llm;
pub mod logs;
pub mod model;
pub mod output;
pub mod preferences;
pub mod subtitle;
pub mod subtitle_render;
pub mod system;
pub mod task_log;
pub mod task_usage;
pub mod task_context;
pub mod task_projection;
pub mod task_projection_store;
pub mod task_status;
pub mod task_stage_handlers;
pub mod task_subtitle_composer;
pub mod task_stage_runner;
pub mod task_stage_store;
pub mod task_worker;
pub mod task_engine;
pub mod task_executor;
pub mod task_path;
pub mod translate;
pub mod transcribe;
pub mod transcription;
pub mod workspace;
pub mod youtube;
pub mod updater;
pub mod file_download;
