//! Post-ASR processing pipeline.
//!
//! This module orchestrates phases after ASR (currently segmentation), while
//! ASR inference itself stays in `services::transcribe`.
mod pipeline;
mod punctuation;

pub use pipeline::{
    RunPostAsrPipelineRequest, run_post_asr_pipeline,
};
pub use punctuation::{PunctuationConfig, optimize_words_with_llm};
