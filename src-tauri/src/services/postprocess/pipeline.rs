use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use voxtrans_core::subtitle::segmenter::plain_text_from_segments;
use voxtrans_core::subtitle::srt::{SegmentWord, SubtitleSegment, to_srt_from_segments};

use crate::services::preferences::{HotwordCorrection, LlmSettings};
use crate::services::transcribe::{
    BuildSegmentsRequest, SegmentWithWordsDto, WordTokenDto, build_segments_from_words,
};

use super::hotword::{run_hotword_correction, should_run_hotword_correction};
use super::punctuation::run_punctuation_restore;
use super::types::{HotwordStats, PunctuationStats, TimedHotwordSegment};

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RunPostAsrPipelineRequest {
    pub task_id: String,
    pub audio_path: String,
    pub words: Vec<WordTokenDto>,
    pub auto_punc: bool,
    pub threads: u32,
    pub llm: LlmSettings,
    pub hotword_correction: HotwordCorrection,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunPostAsrPipelineResponse {
    pub text: String,
    pub srt: String,
    pub srt_output_path: String,
    pub segments: Vec<SegmentWithWordsDto>,
    pub words: Vec<WordTokenDto>,
    pub punctuation: PunctuationStats,
    pub hotword: HotwordStats,
}

pub async fn run_post_asr_pipeline<F>(
    request: RunPostAsrPipelineRequest,
    pool: &SqlitePool,
    mut on_phase: F,
) -> Result<RunPostAsrPipelineResponse, String>
where
    F: FnMut(&str),
{
    let mut words = request.words.clone();
    let punctuation = if request.auto_punc {
        on_phase("punctuation");
        run_punctuation_restore(
            &mut words,
            request.threads,
            &request.llm,
            Some(pool),
            Some((&request.task_id, &request.audio_path)),
        )
        .await?
    } else {
        PunctuationStats::default()
    };

    let built = build_segments_from_words(BuildSegmentsRequest {
        task_id: request.task_id.clone(),
        audio_path: request.audio_path.clone(),
        words: words.clone(),
    })?;

    let mut segments = to_timed_segments(&built.segments);
    let hotword = if should_run_hotword_correction(&request.hotword_correction, &request.llm) {
        on_phase("hotword");
        run_hotword_correction(
            &mut segments,
            &request.hotword_correction,
            &request.llm,
            pool,
            &request.task_id,
            &request.audio_path,
        )
        .await?
    } else {
        HotwordStats::default()
    };

    let final_words = flatten_words(&segments);
    let final_segments = to_core_segments(&segments);
    let text = plain_text_from_segments(&final_segments);
    let srt = to_srt_from_segments(&final_segments);
    let segments_response = final_segments
        .iter()
        .map(|segment| SegmentWithWordsDto {
            start: segment.start_sec,
            end: segment.end_sec,
            text: segment.text.clone(),
            words: segment
                .words
                .iter()
                .map(|w| WordTokenDto {
                    start: w.start,
                    end: w.end,
                    word: w.word.clone(),
                })
                .collect(),
        })
        .collect::<Vec<_>>();

    Ok(RunPostAsrPipelineResponse {
        text,
        srt,
        srt_output_path: built.srt_output_path,
        segments: segments_response,
        words: final_words,
        punctuation,
        hotword,
    })
}

fn to_timed_segments(segments: &[SegmentWithWordsDto]) -> Vec<TimedHotwordSegment> {
    segments
        .iter()
        .map(|segment| TimedHotwordSegment {
            start_ms: (segment.start * 1000.0).round() as i64,
            end_ms: (segment.end * 1000.0).round() as i64,
            source_text: segment.text.clone(),
            words: segment.words.clone(),
        })
        .collect()
}

fn flatten_words(segments: &[TimedHotwordSegment]) -> Vec<WordTokenDto> {
    segments.iter().flat_map(|s| s.words.clone()).collect()
}

fn to_core_segments(segments: &[TimedHotwordSegment]) -> Vec<SubtitleSegment> {
    segments
        .iter()
        .map(|segment| SubtitleSegment {
            start_sec: (segment.start_ms as f64 / 1000.0).max(0.0),
            end_sec: (segment.end_ms as f64 / 1000.0).max(segment.start_ms as f64 / 1000.0),
            text: segment.source_text.clone(),
            words: segment
                .words
                .iter()
                .map(|w| SegmentWord {
                    start: w.start,
                    end: w.end,
                    word: w.word.clone(),
                })
                .collect(),
        })
        .collect()
}
