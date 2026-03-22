use crate::services::task_log::{TaskLogger, event};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::PathBuf;
use voxtrans_core::subtitle::segmenter::{
    WordToken, normalize_word_tokens, plain_text_from_segments, split_english_segments,
    split_translate_segments, words_from_timed_tokens,
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
    pub vad_elapsed_sec: f64,
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

#[derive(Debug, Serialize, Deserialize, Clone)]
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
    pub subtitle_max_words_per_segment: u32,
    #[serde(default)]
    pub segment_mode: String,
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
    let logger = TaskLogger::main_with_media(request.task_id.clone(), request.audio_path.clone());
    append_transcribe_log(
        &logger,
        event::TRANSCRIBE_STARTED,
        json!({
            "chunkTargetSeconds": request.chunk_target_seconds,
            "provider": request.provider,
            "mediaPath": request.audio_path,
        }),
    );

    let mut options = TranscribeOptions::default();
    let audio_path = PathBuf::from(&request.audio_path);
    options.audio_path = audio_path;
    options.provider = match Provider::from_id(&request.provider) {
        Some(provider) => provider,
        None => {
            let err = format!(
                "unsupported provider: {} (supported: {})",
                request.provider,
                Provider::supported_ids().join(", ")
            );
            append_transcribe_log(
                &logger,
                event::TRANSCRIBE_FAILED,
                json!({ "error": err }),
            );
            return Err(err);
        }
    };
    options.timestamp_mode = TimestampKind::Words;
    options.chunk_target_seconds = request.chunk_target_seconds.clamp(30, 300) as f64;
    options.model_dir = crate::services::model::resolve_engine_model_dir(
        crate::services::model::ModelTarget::Asr,
    );

    if let Some(model_dir) = request.model_dir.as_ref() {
        options.model_dir = PathBuf::from(model_dir);
    }

    let output = voxtrans_core::transcribe_with_parakeet_v2_with_progress(&options, |current, total| {
        on_progress(current, total);
    });
    let output = match output {
        Ok(v) => v,
        Err(raw_err) => {
            let technical = format!(
                "{} (audio: {}, modelDir: {})",
                raw_err,
                options.audio_path.display(),
                options.model_dir.display()
            );
            let user_message = map_transcribe_error(
                &technical,
                &request.provider,
                request.chunk_target_seconds,
            );
            append_transcribe_log(
                &logger,
                event::TRANSCRIBE_FAILED,
                json!({
                    "error": user_message,
                    "detail": technical,
                }),
            );
            return Err(user_message);
        }
    };
    let words = normalize_word_tokens(words_from_timed_tokens(&output.tokens));

    let response = TranscribeResponse {
        words: words.iter().map(word_to_dto).collect(),
        segment_total: output.segment_summaries.len(),
        segment_durations_sec: output
            .segment_summaries
            .iter()
            .map(|s| (s.duration_sec * 100.0).round() / 100.0)
            .collect(),
        audio_duration_sec: output.audio_duration_sec,
        vad_elapsed_sec: output.vad_elapsed_sec,
        transcribe_elapsed_sec: output.transcribe_elapsed_sec,
        execution_provider: output.execution_provider.to_string(),
    };

    append_transcribe_log(
        &logger,
        event::TRANSCRIBE_COMPLETED,
        json!({
            "phase": "transcribe",
            "provider": response.execution_provider,
            "segmentTotal": response.segment_total,
            "segmentDurationsSec": response.segment_durations_sec,
            "audioDurationSec": round2(response.audio_duration_sec),
            "vadElapsedSec": round2(response.vad_elapsed_sec),
            "transcribeElapsedSec": round2(response.transcribe_elapsed_sec),
            "rtfX": round2(calculate_rtf_x(response.audio_duration_sec, response.transcribe_elapsed_sec)),
        }),
    );

    Ok(response)
}

pub fn build_segments_from_words(
    request: BuildSegmentsRequest,
) -> Result<BuildSegmentsResponse, String> {
    let audio_path = PathBuf::from(&request.audio_path);
    let srt_output_path =
        crate::services::task_path::task_srt_output_path(&request.task_id, &audio_path);

    let words: Vec<WordToken> = request.words.into_iter().map(dto_to_word).collect();
    let segment_mode = request.segment_mode.trim().to_lowercase();
    let segments = if segment_mode == "translate_source" {
        split_translate_segments(&words, request.subtitle_max_words_per_segment as usize)
    } else {
        split_english_segments(&words, request.subtitle_max_words_per_segment as usize)
    };
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

fn append_transcribe_log(logger: &TaskLogger, event_type: &str, payload: serde_json::Value) {
    logger.event(event_type, Some(&payload));
}

fn round2(value: f64) -> f64 {
    if !value.is_finite() {
        return 0.0;
    }
    (value * 100.0).round() / 100.0
}

fn calculate_rtf_x(audio_duration_sec: f64, transcribe_elapsed_sec: f64) -> f64 {
    if !audio_duration_sec.is_finite()
        || !transcribe_elapsed_sec.is_finite()
        || audio_duration_sec <= 0.0
        || transcribe_elapsed_sec <= 0.0
    {
        return 0.0;
    }
    audio_duration_sec / transcribe_elapsed_sec
}

fn map_transcribe_error(raw: &str, provider: &str, chunk_target_seconds: u32) -> String {
    let lowered = raw.to_lowercase();
    let directml_oom =
        lowered.contains("887a0006")
            || lowered.contains("dmlexecutionprovider")
            || lowered.contains("onnx runtime error")
                && lowered.contains("gpu")
                && lowered.contains("invalid")
            || lowered.contains("lstm node");

    if directml_oom {
        return format!(
            "转录失败：显存/图形资源不足（{}）。请在设置中将“分段时长”调小后重试（当前 {} 秒，建议 60-120 秒）。",
            if provider.eq_ignore_ascii_case("directml") {
                "DirectML"
            } else {
                "GPU"
            },
            chunk_target_seconds.clamp(30, 300)
        );
    }

    raw.to_string()
}
