use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use voxtrans_core::subtitle::segmenter::{
    WordToken, normalize_word_tokens, plain_text_from_segments, split_english_segments,
    words_from_timed_tokens,
};
use voxtrans_core::subtitle::srt::to_srt_from_segments;
use voxtrans_core::{Provider, TimestampKind, TranscribeOptions};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscribeRequest {
    pub task_id: String,
    pub audio_path: String,
    pub provider: String,
    pub chunk_target_seconds: u32,
    pub model_dir: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscribeResponse {
    pub words: Vec<WordTokenDto>,
    pub segment_total: usize,
    pub segment_durations_sec: Vec<f64>,
    pub audio_duration_sec: f64,
    pub transcribe_elapsed_sec: f64,
    pub execution_provider: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct WordTokenDto {
    pub start: f64,
    pub end: f64,
    pub word: String,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SegmentWithWordsDto {
    pub start: f64,
    pub end: f64,
    pub text: String,
    pub words: Vec<WordTokenDto>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildSegmentsRequest {
    pub task_id: String,
    pub audio_path: String,
    pub words: Vec<WordTokenDto>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildSegmentsResponse {
    pub text: String,
    pub srt: String,
    pub srt_output_path: String,
    pub segments: Vec<SegmentWithWordsDto>,
}

pub fn transcribe_blocking<F>(
    request: TranscribeRequest,
    mut on_progress: F,
) -> Result<TranscribeResponse, String>
where
    F: FnMut(usize, usize),
{
    let mut options = TranscribeOptions::default();
    let audio_path = PathBuf::from(&request.audio_path);
    options.audio_path = audio_path;
    options.provider = match request.provider.to_ascii_lowercase().as_str() {
        "cpu" => Provider::Cpu,
        "cuda" => Provider::Cuda,
        other => return Err(format!("unsupported provider: {other}")),
    };
    options.timestamp_mode = TimestampKind::Words;
    options.chunk_target_seconds = request.chunk_target_seconds.clamp(60, 1800) as f64;
    options.model_dir = crate::services::model::resolve_model_dir();

    if let Some(model_dir) = request.model_dir {
        options.model_dir = PathBuf::from(model_dir);
    }

    let output =
        voxtrans_core::transcribe_with_parakeet_v2_with_progress(&options, |current, total| {
            on_progress(current, total);
        })
        .map_err(|err| err.to_string())?;
    let words = normalize_word_tokens(words_from_timed_tokens(&output.tokens));

    Ok(TranscribeResponse {
        words: words.iter().map(word_to_dto).collect(),
        segment_total: output.segment_summaries.len(),
        segment_durations_sec: output
            .segment_summaries
            .iter()
            .map(|s| (s.duration_sec * 100.0).round() / 100.0)
            .collect(),
        audio_duration_sec: output.audio_duration_sec,
        transcribe_elapsed_sec: output.transcribe_elapsed_sec,
        execution_provider: output.execution_provider.to_string(),
    })
}

pub fn build_segments_from_words(
    request: BuildSegmentsRequest,
) -> Result<BuildSegmentsResponse, String> {
    let audio_path = PathBuf::from(&request.audio_path);
    let srt_output_path =
        crate::services::task_path::task_srt_output_path(&request.task_id, &audio_path);

    let words: Vec<WordToken> = request.words.into_iter().map(dto_to_word).collect();
    let segments = split_english_segments(&words);
    let srt = to_srt_from_segments(&segments);
    let text = plain_text_from_segments(&segments);

    if let Some(parent) = srt_output_path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }

    let segments_response: Vec<SegmentWithWordsDto> =
        segments.iter().map(segment_to_response).collect();

    Ok(BuildSegmentsResponse {
        text,
        srt,
        srt_output_path: srt_output_path.display().to_string(),
        segments: segments_response,
    })
}

fn dto_to_word(response: WordTokenDto) -> WordToken {
    WordToken {
        start: response.start,
        end: response.end,
        word: response.word,
    }
}

fn word_to_dto(word: &WordToken) -> WordTokenDto {
    WordTokenDto {
        start: word.start,
        end: word.end,
        word: word.word.clone(),
    }
}

fn segment_to_response(
    segment: &voxtrans_core::subtitle::srt::SubtitleSegment,
) -> SegmentWithWordsDto {
    SegmentWithWordsDto {
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
    }
}
