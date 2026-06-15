//! Watchability merge utilities for subtitle beautify.
//!
//! These are internal helper functions used by watchability_merge. Some are
//! not currently called but retained as part of the toolkit.
#![allow(dead_code)]

mod constants;
mod language_units;
mod numbers;
mod quality;
mod text_utils;
mod time_utils;
mod translation_candidate;
mod types;
mod watchability;
mod watchability_merge;

pub use types::*;
pub use watchability_merge::merge_watchability_subtitle_srt_segments;
