//! Post-ASR processing pipeline.
//!
//! This module orchestrates phases after ASR (currently segmentation), while
//! ASR inference itself stays in `services::transcribe`.
mod pipeline;
mod punctuation;
mod correction;

pub use pipeline::{
    RunPostAsrPipelineRequest, RunPostAsrPipelineResponse, run_post_asr_pipeline,
};
pub use punctuation::{PunctuationConfig, optimize_words_with_rig_node};
pub use correction::{CorrectionConfig, CorrectionTerminologyEntry, correct_words_with_rig_node};
