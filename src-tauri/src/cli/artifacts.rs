//! Stage artifacts: the interface between verbs.
//!
//! Each verb reads the previous stage's artifact from disk and writes its own,
//! so a stage can be re-run in isolation:
//!
//! ```text
//! media ──transcribe──▶ words.json ──segment──▶ source.json ──translate──▶ trans.json
//!                                                      │
//!                                                      └──terminology──▶ briefing.json
//! ```
//!
//! All JSON is camelCase (the codebase's DTO convention) and carries the task
//! identity plus the settings that produced it, so a report can always say
//! which model/preset a result came from.
//!
//! The conversions here are deliberately the *same* functions the desktop app
//! uses to shape a task's source of truth
//! (`commands::workspace::adapters::workspace_subtitle_segments_from_step2_segments`
//! and `services::subtitle_import::workspace_segments_to_step2`), because the
//! whole point of the CLI is to reproduce what the GUI would have done.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::commands::transcription::GroupedSentenceSegmentCommandDto;
use crate::commands::transcribe_types::WordTokenCommandDto;
use crate::commands::workspace::adapters::{
    step2_segments_to_srt, workspace_subtitle_segments_from_step2_segments,
};
use crate::domain::workspace_subtitle::WorkspaceSubtitleSegment;
use crate::services::subtitle_beautify::{beautify_workspace_segments, beautify_workspace_text};
use crate::services::subtitle_import::workspace_segments_to_step2;
use crate::services::subtitle_srt::SubtitleSrtSegment;
use crate::services::terminology_agent::TranscriptCue;

use super::CliResult;
use super::context::CliContext;

pub const WORDS_JSON: &str = "words.json";
pub const SOURCE_JSON: &str = "source.json";
pub const BRIEFING_JSON: &str = "briefing.json";
pub const TRANSLATED_JSON: &str = "trans.json";
pub const RUN_JSON: &str = "run.json";
pub const SRC_SRT: &str = "src.srt";

// ── transcribe ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct WordsStats {
    #[serde(default)]
    pub audio_duration_sec: f64,
    #[serde(default)]
    pub vad_elapsed_sec: f64,
    #[serde(default)]
    pub transcribe_elapsed_sec: f64,
    /// Real-time factor: transcribe seconds per audio second.
    #[serde(default)]
    pub rtf_x: f64,
    #[serde(default)]
    pub execution_provider: String,
    #[serde(default)]
    pub segment_total: usize,
    /// ASR / alignment units loaded from the resume tables instead of being
    /// recomputed. A run where these equal `segment_total` did no inference at
    /// all, which is the difference between "your change was exercised" and
    /// "the cache answered" — the report says `cache` for the stage in that
    /// case.
    #[serde(default)]
    pub cached_asr_segments: usize,
    #[serde(default)]
    pub cached_align_segments: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WordsArtifact {
    pub task_id: String,
    pub media_path: String,
    pub source_lang: String,
    pub asr_model: String,
    pub align_model: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub aligned_text: String,
    pub words: Vec<WordTokenCommandDto>,
    #[serde(default)]
    pub vad_speech_segments: Vec<(f64, f64)>,
    #[serde(default)]
    pub stats: WordsStats,
}

// ── segment ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SourceStats {
    #[serde(default)]
    pub micro_chunk_total: usize,
    #[serde(default)]
    pub boundary_total: usize,
    #[serde(default)]
    pub sentence_total: usize,
    #[serde(default)]
    pub cue_total: usize,
    #[serde(default)]
    pub beautified: bool,
    #[serde(default)]
    pub subtitle_length_preset: String,
}

/// Sentence-segmented, optionally beautified source cues — the CLI's analogue
/// of the app's source "source of truth" (SoT) rows.
///
/// `segments` is the SoT as translation-step input (post-beautify), so the
/// `translate` verb translates exactly what the GUI would have translated.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceArtifact {
    pub task_id: String,
    pub media_path: String,
    pub source_lang: String,
    pub target_lang: String,
    #[serde(default)]
    pub source_text: String,
    pub segments: Vec<GroupedSentenceSegmentCommandDto>,
    #[serde(default)]
    pub stats: SourceStats,
}

impl SourceArtifact {
    /// Build the source SoT from step-2 segments, applying beautify the same way
    /// `review_flow::materialize_source_sot` does for the app.
    pub fn build(
        ctx: &CliContext,
        segments: &[GroupedSentenceSegmentCommandDto],
        stats: SourceStats,
    ) -> Self {
        let mut workspace = workspace_subtitle_segments_from_step2_segments(segments);
        if ctx.frozen.enable_subtitle_beautify {
            beautify_workspace_segments(
                &mut workspace,
                ctx.frozen.subtitle_length_preset.as_str(),
                &ctx.target_lang,
            );
        }
        let source_text = workspace
            .iter()
            .map(|segment| segment.source_text.trim())
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        let cues = workspace_segments_to_step2(&workspace);
        Self {
            task_id: ctx.task_id.clone(),
            media_path: ctx.input_path_str.clone(),
            source_lang: ctx.source_lang.clone(),
            target_lang: ctx.target_lang.clone(),
            source_text,
            segments: cues,
            stats: SourceStats {
                cue_total: workspace.len(),
                beautified: ctx.frozen.enable_subtitle_beautify,
                subtitle_length_preset: ctx.frozen.subtitle_length_preset.as_str().to_string(),
                ..stats
            },
        }
    }

    /// Step-2 DTOs as the translation layer consumes them.
    pub fn step2(&self) -> Vec<GroupedSentenceSegmentCommandDto> {
        self.segments.clone()
    }

    pub fn srt_segments(&self) -> Vec<SubtitleSrtSegment> {
        self.segments
            .iter()
            .map(|segment| SubtitleSrtSegment {
                start_ms: crate::services::time_utils::seconds_to_millis(segment.start),
                end_ms: crate::services::time_utils::seconds_to_millis(segment.end.max(segment.start)),
                source_text: segment.segment.clone(),
                translated_text: String::new(),
            })
            .collect()
    }

    /// Cues for the terminology agent (1-based index + start time, like the
    /// app's `translation_flow::resolve_terminology_for_translation`).
    pub fn cues(&self) -> Vec<TranscriptCue> {
        self.segments
            .iter()
            .enumerate()
            .map(|(index, segment)| TranscriptCue {
                index: index + 1,
                start_ms: crate::services::time_utils::seconds_to_millis(segment.start),
                text: segment.segment.clone(),
            })
            .collect()
    }

    /// Source text used for glossary grounding checks (one cue per line).
    pub fn haystack(&self) -> String {
        self.segments
            .iter()
            .map(|segment| segment.segment.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

// ── translate ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslatedSegment {
    pub segment_id: usize,
    pub start: f64,
    pub end: f64,
    pub source: String,
    pub translation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TranslateStats {
    #[serde(default)]
    pub batch_total: usize,
    #[serde(default)]
    pub batch_size: usize,
    #[serde(default)]
    pub elapsed_sec: f64,
    #[serde(default)]
    pub glossary_terms: usize,
    #[serde(default)]
    pub user_terms: usize,
    #[serde(default)]
    pub agent_glossary_terms: usize,
    #[serde(default)]
    pub agent_end_reason: String,
    #[serde(default)]
    pub style_guide_chars: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslatedArtifact {
    pub task_id: String,
    pub media_path: String,
    pub source_lang: String,
    pub target_lang: String,
    #[serde(default)]
    pub segments: Vec<TranslatedSegment>,
    #[serde(default)]
    pub stats: TranslateStats,
}

impl TranslatedArtifact {
    /// Join translations back onto the source cues (1-based segment ids) to
    /// produce the deliverable SRT rows. Fails when a cue has no translation,
    /// rather than silently emitting an empty line.
    pub fn srt_segments(&self, source: &SourceArtifact) -> CliResult<Vec<SubtitleSrtSegment>> {
        let by_id: std::collections::HashMap<usize, &str> = self
            .segments
            .iter()
            .map(|segment| (segment.segment_id, segment.translation.as_str()))
            .collect();

        source
            .segments
            .iter()
            .enumerate()
            .map(|(index, cue)| {
                let id = index + 1;
                let translation = by_id.get(&id).copied().ok_or_else(|| {
                    format!("translation is missing for segment #{id} ({:?})", cue.segment)
                })?;
                Ok(SubtitleSrtSegment {
                    start_ms: crate::services::time_utils::seconds_to_millis(cue.start),
                    end_ms: crate::services::time_utils::seconds_to_millis(cue.end.max(cue.start)),
                    source_text: cue.segment.clone(),
                    translated_text: translation.to_string(),
                })
            })
            .collect()
    }
}

// ── loading ─────────────────────────────────────────────────────────────────

pub fn load_words(path: &Path) -> CliResult<WordsArtifact> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| format!("failed to read {}: {e}", path.display()))?;
    serde_json::from_str(&raw).map_err(|e| {
        format!(
            "failed to parse {} as a words.json artifact: {e}",
            path.display()
        )
    })
}

pub fn load_source(path: &Path) -> CliResult<SourceArtifact> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| format!("failed to read {}: {e}", path.display()))?;
    serde_json::from_str(&raw).map_err(|e| {
        format!(
            "failed to parse {} as a source.json artifact: {e}",
            path.display()
        )
    })
}

/// Resolve any accepted input form into a `SourceArtifact`:
///
/// - `*.srt` — imported cues. Each cue is its own translation unit
///   (`semanticSentenceId = 0`), exactly like the app's SRT-import task; a
///   beautified pass is *not* applied because imported boundaries are the
///   user's.
/// - `source.json` — the `segment` verb's output, used as-is.
/// - `words.json` — the `segment` verb is run in memory first (with the
///   context's preset/beautify settings).
pub async fn source_from_input(ctx: &CliContext, path: &Path) -> CliResult<SourceArtifact> {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default();

    if !path.is_file() {
        return Err(format!("input file not found: {}", path.display()));
    }

    if name.eq_ignore_ascii_case(SRC_SRT) || crate::services::subtitle_import::is_srt_path(&name) {
        return source_from_srt(ctx, path);
    }
    if name.eq_ignore_ascii_case(SOURCE_JSON) {
        return load_source(path);
    }
    if name.eq_ignore_ascii_case(WORDS_JSON) {
        let words = load_words(path)?;
        return super::asr_stage::segment_stage(ctx, &words).await;
    }

    // Unknown name: sniff the shape rather than guessing from the extension.
    let raw = std::fs::read_to_string(path)
        .map_err(|e| format!("failed to read {}: {e}", path.display()))?;
    if let Ok(source) = serde_json::from_str::<SourceArtifact>(&raw) {
        return Ok(source);
    }
    if let Ok(words) = serde_json::from_str::<WordsArtifact>(&raw) {
        return super::asr_stage::segment_stage(ctx, &words).await;
    }
    Err(format!(
        "{} is neither an SRT nor a source.json/words.json artifact",
        path.display()
    ))
}

fn source_from_srt(ctx: &CliContext, path: &Path) -> CliResult<SourceArtifact> {
    let segments = crate::services::subtitle_import::load_srt_segments_for_run("", &path.to_string_lossy())?;
    if segments.is_empty() {
        return Err(format!("{} contains no subtitle cues", path.display()));
    }
    let source_text = crate::services::subtitle_import::source_text_from_workspace_segments(&segments);
    let cues = workspace_segments_to_step2(&segments);
    Ok(SourceArtifact {
        task_id: ctx.task_id.clone(),
        media_path: path.to_string_lossy().to_string(),
        source_lang: ctx.source_lang.clone(),
        target_lang: ctx.target_lang.clone(),
        source_text,
        stats: SourceStats {
            cue_total: cues.len(),
            subtitle_length_preset: ctx.frozen.subtitle_length_preset.as_str().to_string(),
            ..SourceStats::default()
        },
        segments: cues,
    })
}

/// Write the deliverable source SRT (`src.srt`) and return its path.
pub fn write_source_srt(ctx: &CliContext, source: &SourceArtifact) -> CliResult<String> {
    let path = ctx.out_path(SRC_SRT);
    crate::services::subtitle_srt::write_variants_to_directory(
        &ctx.out_dir,
        &source.srt_segments(),
        &[crate::services::subtitle_srt::ExportSrtItem::Source],
    )?;
    Ok(path.display().to_string())
}

/// Step-2 SRT (pre-beautify boundaries) — what the app keeps as `resultSrt`
/// before the beautify pass rewrites cue text.
pub fn step2_srt(segments: &[GroupedSentenceSegmentCommandDto]) -> String {
    step2_segments_to_srt(segments)
}

/// Apply the target-language text polish that the app applies to translated
/// cues (`review_flow::materialize_target_sot`).
pub fn beautify_translated(segments: &mut [WorkspaceSubtitleSegment], target_lang: &str) {
    beautify_workspace_text(segments, target_lang);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::transcription::GroupedSentenceTokenCommandDto;

    fn cue(text: &str, start: f64, end: f64, sid: usize) -> GroupedSentenceSegmentCommandDto {
        GroupedSentenceSegmentCommandDto {
            segment: text.to_string(),
            semantic_sentence_id: sid,
            start,
            end,
            tokens: vec![GroupedSentenceTokenCommandDto {
                text: text.to_string(),
                start,
                end,
            }],
        }
    }

    /// Cue identity and 1-based ids are what `srt_segments` pairs on: a missing
    /// translation must be an error, not an empty subtitle line.
    #[test]
    fn missing_translation_is_an_error() {
        let source = SourceArtifact {
            task_id: "t".into(),
            media_path: "m".into(),
            source_lang: "en".into(),
            target_lang: "zh-CN".into(),
            source_text: "a\nb".into(),
            segments: vec![cue("a", 0.0, 1.0, 1), cue("b", 1.0, 2.0, 1)],
            stats: SourceStats::default(),
        };
        let translated = TranslatedArtifact {
            task_id: "t".into(),
            media_path: "m".into(),
            source_lang: "en".into(),
            target_lang: "zh-CN".into(),
            segments: vec![TranslatedSegment {
                segment_id: 1,
                start: 0.0,
                end: 1.0,
                source: "a".into(),
                translation: "甲".into(),
            }],
            stats: TranslateStats::default(),
        };

        let err = translated
            .srt_segments(&source)
            .expect_err("segment #2 has no translation");
        assert!(err.contains("segment #2"), "unexpected error: {err}");
    }

    #[test]
    fn srt_segments_pair_by_segment_id_and_preserve_timing() {
        let source = SourceArtifact {
            task_id: "t".into(),
            media_path: "m".into(),
            source_lang: "en".into(),
            target_lang: "zh-CN".into(),
            source_text: "a".into(),
            segments: vec![cue("a", 1.25, 2.5, 0)],
            stats: SourceStats::default(),
        };
        let translated = TranslatedArtifact {
            task_id: "t".into(),
            media_path: "m".into(),
            source_lang: "en".into(),
            target_lang: "zh-CN".into(),
            segments: vec![TranslatedSegment {
                segment_id: 1,
                start: 1.25,
                end: 2.5,
                source: "a".into(),
                translation: "甲".into(),
            }],
            stats: TranslateStats::default(),
        };

        let rows = translated.srt_segments(&source).expect("rows");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].start_ms, 1250);
        assert_eq!(rows[0].end_ms, 2500);
        assert_eq!(rows[0].source_text, "a");
        assert_eq!(rows[0].translated_text, "甲");
    }
}
