use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::PathBuf;

use crate::services::task_log::{TaskLogger, event};
use crate::services::transcribe::{
    BuildSegmentsRequest, SegmentWithWordsDto, WordTokenDto, build_segments_from_words,
};
use voxtrans_core::subtitle::beautify::beautify_words_for_subtitle;
use voxtrans_core::subtitle::segmenter::WordToken;
use super::punctuation::{PunctuationConfig, optimize_words_with_llm};

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RunPostAsrPipelineRequest {
    pub task_id: String,
    pub audio_path: String,
    pub words: Vec<WordTokenDto>,
    pub subtitle_max_words_per_segment: u32,
    #[serde(default)]
    pub enable_punctuation_optimization: bool,
    #[serde(default)]
    pub translate_api_key: String,
    #[serde(default)]
    pub translate_base_url: String,
    #[serde(default)]
    pub translate_model: String,
    #[serde(default = "default_llm_concurrency")]
    pub llm_concurrency: u32,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunPostAsrPipelineResponse {
    pub text: String,
    pub srt: String,
    pub srt_output_path: String,
    pub words_output_path: String,
    pub segments: Vec<SegmentWithWordsDto>,
    pub words: Vec<WordTokenDto>,
    pub post_asr_elapsed_sec: f64,
}

pub async fn run_post_asr_pipeline<F>(
    request: RunPostAsrPipelineRequest,
    mut on_phase: F,
) -> Result<RunPostAsrPipelineResponse, String>
where
    F: FnMut(&str),
{
    let logger = TaskLogger::main_with_media(request.task_id.clone(), request.audio_path.clone());
    let started_at = std::time::Instant::now();

    let base_words = beautify_words_for_subtitle(to_core_words(request.words.clone()));
    on_phase("punctuate");
    let words = from_core_words(
        optimize_words_with_llm(
            &request.task_id,
            &request.audio_path,
            base_words,
            &PunctuationConfig {
                enabled: request.enable_punctuation_optimization,
                base_url: request.translate_base_url.clone(),
                api_key: request.translate_api_key.clone(),
                model: request.translate_model.clone(),
                llm_concurrency: request.llm_concurrency,
            },
        )
        .await,
    );
    let words_output_path = save_words_json(&request.task_id, &request.audio_path, &words)?;
    logger.event(
        "transcribe.words_saved",
        Some(&json!({
            "outputPath": words_output_path,
            "wordTotal": words.len(),
        })),
    );

    on_phase("segment");
    let built = build_segments_from_words(BuildSegmentsRequest {
        task_id: request.task_id.clone(),
        audio_path: request.audio_path.clone(),
        words: words.clone(),
        subtitle_max_words_per_segment: request.subtitle_max_words_per_segment,
    });
    let built = match built {
        Ok(v) => v,
        Err(err) => {
            logger.event(
                event::TRANSCRIBE_FAILED,
                Some(&json!({
                    "phase": "post_asr",
                    "error": err,
                })),
            );
            return Err(err);
        }
    };

    Ok(RunPostAsrPipelineResponse {
        text: built.text,
        srt: built.srt,
        srt_output_path: built.srt_output_path,
        words_output_path,
        segments: built.segments,
        words,
        post_asr_elapsed_sec: round2(started_at.elapsed().as_secs_f64()),
    })
}

fn default_llm_concurrency() -> u32 {
    4
}

fn round2(value: f64) -> f64 {
    if !value.is_finite() {
        return 0.0;
    }
    (value * 100.0).round() / 100.0
}

fn to_core_words(words: Vec<WordTokenDto>) -> Vec<WordToken> {
    words
        .into_iter()
        .map(|word| WordToken {
            start: word.start,
            end: word.end,
            word: word.word,
        })
        .collect()
}

fn from_core_words(words: Vec<WordToken>) -> Vec<WordTokenDto> {
    words
        .into_iter()
        .map(|word| WordTokenDto {
            start: word.start,
            end: word.end,
            word: word.word,
        })
        .collect()
}

fn save_words_json(task_id: &str, audio_path: &str, words: &[WordTokenDto]) -> Result<String, String> {
    let media_path = PathBuf::from(audio_path);
    let output_path = crate::services::task_path::task_words_output_path(task_id, &media_path);
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let content = serde_json::to_vec_pretty(words).map_err(|err| err.to_string())?;
    std::fs::write(&output_path, content).map_err(|err| err.to_string())?;
    Ok(output_path.display().to_string())
}
