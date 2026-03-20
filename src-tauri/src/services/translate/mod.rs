//! Translation pipeline modules (skeleton).

pub mod adapters;
pub mod pipeline;
pub mod prompt;
pub mod qa_agent;
pub mod rules;
pub mod service;
pub mod types;
pub mod validation;

pub use service::{run_translate_pipeline, run_translate_pipeline_with_phase};
