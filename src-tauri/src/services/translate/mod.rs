//! Translation pipeline modules (skeleton).

pub mod pipeline;
pub mod prompt;
pub mod segment_optimize;
pub mod rules;
pub mod service;
pub mod types;
pub mod validation;

pub use service::{
    run_translate_pipeline, run_translate_summarize, run_translate_with_theme,
};
