use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::services::task_log::{TaskLogger, event};
use super::correction::{
    CorrectionConfig, CorrectionTerminologyEntry, correct_words_with_llm,
};
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
    #[serde(default = "default_source_lang")]
    pub source_lang: String,
    #[serde(default)]
    pub enable_punctuation_optimization: bool,
    #[serde(default = "default_true")]
    pub enable_asr_correction: bool,
    #[serde(default)]
    pub enable_terminology: bool,
    #[serde(default)]
    pub terminology_entries: Vec<CorrectionTerminologyEntryDto>,
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
        .await?,
    );

    let words = if request.enable_asr_correction {
        on_phase("correct");
        from_core_words(
            correct_words_with_llm(
                &request.task_id,
                &request.audio_path,
                to_core_words(words),
                &CorrectionConfig {
                    source_lang: request.source_lang.clone(),
                    base_url: request.translate_base_url.clone(),
                    api_key: request.translate_api_key.clone(),
                    model: request.translate_model.clone(),
                    terminology_entries: if request.enable_terminology {
                        map_correction_terminology_entries(&request.terminology_entries)
                    } else {
                        Vec::new()
                    },
                },
            )
            .await?,
        )
    } else {
        words
    };
    let words_output_path = String::new();

    on_phase("segment");
    let built = build_segments_from_words(BuildSegmentsRequest {
        task_id: request.task_id.clone(),
        audio_path: request.audio_path.clone(),
        words: words.clone(),
        subtitle_max_words_per_segment: request.subtitle_max_words_per_segment,
        segment_mode: "transcribe".to_string(),
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

fn default_source_lang() -> String {
    "auto".to_string()
}

fn default_true() -> bool {
    true
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

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CorrectionTerminologyEntryDto {
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub target: String,
    #[serde(default)]
    pub note: String,
}

fn map_correction_terminology_entries(
    items: &[CorrectionTerminologyEntryDto],
) -> Vec<CorrectionTerminologyEntry> {
    items
        .iter()
        .map(|item| CorrectionTerminologyEntry {
            source: item.source.trim().to_string(),
            target: item.target.trim().to_string(),
            note: item.note.trim().to_string(),
        })
        .filter(|entry| !entry.source.is_empty() && !entry.target.is_empty())
        .collect()
}
