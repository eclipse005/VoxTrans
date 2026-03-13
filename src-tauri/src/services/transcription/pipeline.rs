use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use voxtrans_core::subtitle::segmenter::plain_text_from_segments;
use voxtrans_core::subtitle::srt::to_srt_from_segments;

use crate::services::preferences::{HotwordCorrection, LlmSettings};
use crate::services::transcribe::{
    BuildSegmentsRequest, SegmentWithWordsDto, WordTokenDto, build_segments_from_words,
};

use super::domain::{HotwordStats, PunctuationStats};
use super::mapper::{flatten_words, to_core_segments, to_segment_words_dto, to_timed_segments};
use super::stages::{
    hotword::{run_stage as run_hotword_stage, should_run_hotword_correction},
    punctuation::run_stage as run_punctuation_stage,
};

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
    pub stage_outcomes: Vec<StageOutcome>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct StageOutcome {
    pub stage: String,
    pub executed: bool,
    pub warnings: Vec<String>,
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
    let mut stage_outcomes = Vec::new();
    let punctuation = if request.auto_punc {
        on_phase("punctuation");
        let result = run_punctuation_stage(
            &mut words,
            request.threads,
            &request.llm,
            Some(pool),
            Some((&request.task_id, &request.audio_path)),
        )
        .await?;
        stage_outcomes.push(StageOutcome {
            stage: "punctuation".to_string(),
            executed: result.executed,
            warnings: result.warnings.clone(),
        });
        result.metrics
    } else {
        stage_outcomes.push(StageOutcome {
            stage: "punctuation".to_string(),
            executed: false,
            warnings: vec!["未启用 AI 标点增强".to_string()],
        });
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
        let result = run_hotword_stage(
            &mut segments,
            &request.hotword_correction,
            &request.llm,
            pool,
            &request.task_id,
            &request.audio_path,
        )
        .await?;
        stage_outcomes.push(StageOutcome {
            stage: "hotword".to_string(),
            executed: result.executed,
            warnings: result.warnings.clone(),
        });
        result.metrics
    } else {
        stage_outcomes.push(StageOutcome {
            stage: "hotword".to_string(),
            executed: false,
            warnings: vec!["未启用热词矫正或未配置可用 LLM/术语组".to_string()],
        });
        HotwordStats::default()
    };

    let final_words = flatten_words(&segments);
    let final_segments = to_core_segments(&segments);
    let text = plain_text_from_segments(&final_segments);
    let srt = to_srt_from_segments(&final_segments);
    let segments_response = to_segment_words_dto(&final_segments);

    Ok(RunPostAsrPipelineResponse {
        text,
        srt,
        srt_output_path: built.srt_output_path,
        segments: segments_response,
        words: final_words,
        punctuation,
        hotword,
        stage_outcomes,
    })
}
