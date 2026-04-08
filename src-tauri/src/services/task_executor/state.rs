use serde_json::Value;

use crate::services::final_subtitle::parse_final_subtitle_segments;
use crate::services::task_context::{
    STAGE_ASR, STAGE_PUNCTUATE, STAGE_SEGMENT, STAGE_SEGMENT_OPTIMIZE, STAGE_SUMMARIZE,
    STAGE_TRANSLATE, TaskContext,
};
use crate::services::transcribe::{SegmentWithWordsDto, WordTokenDto};
use crate::services::translate::segment_optimize::SEGMENT_OPTIMIZE_LAYOUT_VERSION;
use crate::services::translate::types::{TranslateTerminologyEntry, TranslateToken};
use voxtrans_core::subtitle::segmenter::WordToken;

#[derive(Debug, Clone)]
pub(super) struct AsrResumeSnapshot {
    pub words: Vec<WordTokenDto>,
    pub segment_total: usize,
    pub audio_duration_sec: f64,
    pub vad_elapsed_sec: f64,
    pub transcribe_elapsed_sec: f64,
    pub execution_provider: String,
}

#[derive(Debug, Clone)]
pub(super) struct SegmentResumeSnapshot {
    pub text: String,
    pub srt: String,
    pub srt_output_path: String,
    pub segments: Vec<SegmentWithWordsDto>,
}

#[derive(Debug, Clone)]
pub(super) struct SummarizeSnapshot {
    pub theme: String,
    pub terminology_entries: Vec<TranslateTerminologyEntry>,
    pub terminology_primary_total: usize,
    pub terminology_supporting_total: usize,
}

#[derive(Debug, Clone)]
pub(super) struct TranslateSnapshot {
    pub source_srt: String,
    pub target_srt: String,
    pub bilingual_srt_source_first: String,
    pub bilingual_srt_target_first: String,
    pub segments: Vec<crate::services::translate::types::TranslateSegment>,
}

#[derive(Debug, Clone)]
pub(super) struct SegmentOptimizeSnapshot {
    pub segments: Vec<crate::services::translate::types::TranslateSegment>,
    pub report: Value,
    pub applied_change_total: usize,
    pub source_srt: String,
    pub target_srt: String,
    pub src_trans_srt: String,
    pub trans_src_srt: String,
}

pub(super) fn has_source_segments_available(raw: &str) -> bool {
    parse_tokens_from_segments(raw)
        .iter()
        .any(|token| !token.word.trim().is_empty())
}

pub(super) fn load_asr_resume_snapshot(context: &TaskContext) -> Option<AsrResumeSnapshot> {
    if !stage_is_done(context.stage_status(STAGE_ASR)) {
        return None;
    }
    let words_value = context.stages.asr.output.get("words")?.clone();
    let words = serde_json::from_value::<Vec<WordTokenDto>>(words_value).ok()?;
    if words.is_empty() {
        return None;
    }
    let segment_total = context
        .stages
        .asr
        .output
        .get("segmentTotal")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize)
        .unwrap_or(words.len());
    let audio_duration_sec = context
        .stages
        .asr
        .output
        .get("audioDurationSec")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let execution_provider = context
        .stages
        .asr
        .output
        .get("provider")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();
    let vad_elapsed_sec = context
        .stages
        .asr
        .metrics
        .get("vadElapsedSec")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let transcribe_elapsed_sec = context
        .stages
        .asr
        .metrics
        .get("transcribeElapsedSec")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    Some(AsrResumeSnapshot {
        words,
        segment_total,
        audio_duration_sec,
        vad_elapsed_sec,
        transcribe_elapsed_sec,
        execution_provider,
    })
}

pub(super) fn load_stage_words(context: &TaskContext, stage: &str) -> Option<Vec<WordTokenDto>> {
    if !stage_is_done(context.stage_status(stage)) {
        return None;
    }
    let output = match stage {
        STAGE_ASR => &context.stages.asr.output,
        STAGE_PUNCTUATE => &context.stages.punctuate.output,
        _ => return None,
    };
    let words_value = output.get("words")?.clone();
    let words = serde_json::from_value::<Vec<WordTokenDto>>(words_value).ok()?;
    if words.is_empty() {
        return None;
    }
    Some(words)
}

pub(super) fn load_segment_snapshot(context: &TaskContext) -> Option<SegmentResumeSnapshot> {
    if !stage_is_done(context.stage_status(STAGE_SEGMENT)) {
        return None;
    }
    let output = &context.stages.segment.output;
    let text = output.get("text")?.as_str()?.to_string();
    let srt = output.get("srt")?.as_str()?.to_string();
    let srt_output_path = output.get("srtOutputPath")?.as_str()?.to_string();
    let segments_value = output.get("segments")?.clone();
    let segments = serde_json::from_value::<Vec<SegmentWithWordsDto>>(segments_value).ok()?;
    if segments.is_empty() {
        return None;
    }
    Some(SegmentResumeSnapshot {
        text,
        srt,
        srt_output_path,
        segments,
    })
}

pub(super) fn load_summarize_snapshot(context: &TaskContext) -> Option<SummarizeSnapshot> {
    if !stage_is_done(context.stage_status(STAGE_SUMMARIZE)) {
        return None;
    }
    let output = &context.stages.summarize.output;
    let theme = output.get("theme")?.as_str()?.trim().to_string();
    if theme.is_empty() {
        return None;
    }
    let terminology_entries = output
        .get("terminologyEntries")
        .cloned()
        .and_then(|value| serde_json::from_value::<Vec<TranslateTerminologyEntry>>(value).ok())
        .unwrap_or_default();
    let terminology_primary_total = output
        .get("terminologyPrimaryTotal")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize)
        .unwrap_or(0);
    let terminology_supporting_total = output
        .get("terminologySupportingTotal")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize)
        .unwrap_or(0);
    Some(SummarizeSnapshot {
        theme,
        terminology_entries,
        terminology_primary_total,
        terminology_supporting_total,
    })
}

pub(super) fn load_translate_snapshot(context: &TaskContext) -> Option<TranslateSnapshot> {
    if !stage_is_done(context.stage_status(STAGE_TRANSLATE)) {
        return None;
    }
    let output = &context.stages.translate.output;
    let source_srt = output.get("sourceSrt")?.as_str()?.to_string();
    let target_srt = output.get("targetSrt")?.as_str()?.to_string();
    let bilingual_srt_source_first = output.get("bilingualSrtSourceFirst")?.as_str()?.to_string();
    let bilingual_srt_target_first = output.get("bilingualSrtTargetFirst")?.as_str()?.to_string();
    let segments =
        serde_json::from_value::<Vec<crate::services::translate::types::TranslateSegment>>(
            output.get("segments")?.clone(),
        )
        .ok()?;
    if segments.is_empty() {
        return None;
    }
    Some(TranslateSnapshot {
        source_srt,
        target_srt,
        bilingual_srt_source_first,
        bilingual_srt_target_first,
        segments,
    })
}

pub(super) fn load_segment_optimize_snapshot(
    context: &TaskContext,
) -> Option<SegmentOptimizeSnapshot> {
    load_segment_optimize_stage_snapshot(context, STAGE_SEGMENT_OPTIMIZE)
}

pub(super) fn has_translated_segments_available(raw: &str) -> bool {
    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(raw) else {
        return false;
    };
    let Some(arr) = parsed.as_array() else {
        return false;
    };
    arr.iter().any(|segment| {
        segment
            .get("translatedText")
            .and_then(|v| v.as_str())
            .map(|v| !v.trim().is_empty())
            .unwrap_or(false)
    })
}

pub(super) fn count_segments_from_json(raw: &str) -> i64 {
    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(raw) else {
        return 0;
    };
    parsed.as_array().map(|arr| arr.len() as i64).unwrap_or(0)
}

pub(super) fn parse_tokens_from_segments(raw: &str) -> Vec<TranslateToken> {
    let segments = parse_final_subtitle_segments(raw);
    let anchored_tokens = segments
        .iter()
        .flat_map(|segment| {
            segment.source_words.iter().filter_map(|word| {
                if word.word.trim().is_empty() {
                    return None;
                }
                Some(TranslateToken {
                    start: word.start_ms as f64 / 1000.0,
                    end: word.end_ms.max(word.start_ms) as f64 / 1000.0,
                    word: word.word.clone(),
                })
            })
        })
        .collect::<Vec<_>>();
    if !anchored_tokens.is_empty() {
        return anchored_tokens;
    }
    segments
        .into_iter()
        .filter_map(|segment| {
            if segment.source_text.trim().is_empty() {
                return None;
            }
            Some(TranslateToken {
                start: segment.start_ms as f64 / 1000.0,
                end: segment.end_ms.max(segment.start_ms) as f64 / 1000.0,
                word: segment.source_text,
            })
        })
        .collect()
}

pub(super) fn to_core_words(words: Vec<WordTokenDto>) -> Vec<WordToken> {
    words
        .into_iter()
        .map(|word| WordToken {
            start: word.start,
            end: word.end,
            word: word.word,
        })
        .collect()
}

pub(super) fn from_core_words(words: Vec<WordToken>) -> Vec<WordTokenDto> {
    words
        .into_iter()
        .map(|word| WordTokenDto {
            start: word.start,
            end: word.end,
            word: word.word,
        })
        .collect()
}

fn load_segment_optimize_stage_snapshot(
    context: &TaskContext,
    stage: &str,
) -> Option<SegmentOptimizeSnapshot> {
    if !stage_is_done(context.stage_status(stage)) {
        return None;
    }
    if stage != STAGE_SEGMENT_OPTIMIZE {
        return None;
    }
    let output = &context.stages.segment_optimize.output;
    let segments =
        serde_json::from_value::<Vec<crate::services::translate::types::TranslateSegment>>(
            output.get("segments")?.clone(),
        )
        .ok()?;
    if segments.is_empty() {
        return None;
    }
    let applied_change_total = output
        .get("appliedChangeTotal")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as usize;
    let report = output.get("report").cloned().unwrap_or(Value::Null);
    let layout_version = report
        .get("layoutVersion")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    if layout_version != SEGMENT_OPTIMIZE_LAYOUT_VERSION {
        return None;
    }
    Some(SegmentOptimizeSnapshot {
        segments,
        report,
        applied_change_total,
        source_srt: output.get("sourceSrt")?.as_str()?.to_string(),
        target_srt: output.get("targetSrt")?.as_str()?.to_string(),
        src_trans_srt: output.get("srcTransSrt")?.as_str()?.to_string(),
        trans_src_srt: output.get("transSrcSrt")?.as_str()?.to_string(),
    })
}

fn stage_is_done(status: &str) -> bool {
    status.trim().eq_ignore_ascii_case("done")
}
