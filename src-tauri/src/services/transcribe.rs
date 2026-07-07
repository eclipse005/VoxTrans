use crate::services::task_log::{TaskLogger, event};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::PathBuf;
use voxtrans_core::subtitle::segmenter::{WordToken, normalize_word_tokens};

mod asr_align;
pub(crate) use asr_align::{FreshSegmentResult, TranscribeProgressStage};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscribeRequest {
    pub task_id: String,
    pub audio_path: String,
    pub source_lang: String,
    #[serde(default = "default_asr_model")]
    pub asr_model: String,
    #[serde(default = "default_align_model")]
    pub align_model: String,
    pub provider: String,
    pub chunk_target_seconds: u32,
    pub model_dir: Option<String>,
    /// Precomputed ASR segment results: `Vec<(segment_index, text)>`.
    /// These segments will be skipped during ASR.
    #[serde(default)]
    pub precomputed_asr_segments: Vec<(usize, String)>,
    /// Precomputed alignment results: `Vec<(segment_index, ForcedAlignResult)>`.
    /// These segments will be skipped during alignment.
    #[serde(default)]
    pub precomputed_alignment: Vec<(usize, qwen_forced_aligner_rs::ForcedAlignResult)>,
}

fn default_asr_model() -> String {
    crate::services::model::DEFAULT_ASR_MODEL.to_string()
}

fn default_align_model() -> String {
    "Qwen3-ForcedAligner-0.6B".to_string()
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscribeResponse {
    pub words: Vec<WordTokenDto>,
    pub text: String,
    pub aligned_text: String,
    pub segment_total: usize,
    pub segment_durations_sec: Vec<f64>,
    pub audio_duration_sec: f64,
    pub vad_elapsed_sec: f64,
    pub vad_speech_segments: Vec<(f64, f64)>,
    pub transcribe_elapsed_sec: f64,
    pub timing_sec: TranscribeTimingSecDto,
    pub rtf_x: f64,
    pub rtf_breakdown_x: TranscribeRtfBreakdownDto,
    pub execution_provider: String,
    /// Freshly computed ASR results: `Vec<(segment_index, text)>`.
    /// Only includes segments that were NOT precomputed.
    pub new_asr_segments: Vec<(usize, String)>,
}

#[derive(Debug, Serialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub struct TranscribeTimingSecDto {
    pub prepare_elapsed_sec: f64,
    pub vad_elapsed_sec: f64,
    pub temp_wav_write_sec: f64,
    pub asr_load_sec: f64,
    pub asr_transcribe_sec: f64,
    pub qwen_load_sec: f64,
    pub qwen_align_sec: f64,
    pub punctuation_map_sec: f64,
    pub total_elapsed_sec: f64,
}

#[derive(Debug, Serialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub struct TranscribeRtfBreakdownDto {
    pub total: f64,
    pub asr_stage: f64,
    pub asr_transcribe: f64,
    pub qwen_stage: f64,
    pub qwen_align: f64,
    pub model_only: f64,
}

#[derive(Debug, Clone, Copy)]
struct TranscribePhaseMetrics {
    timing_sec: TranscribeTimingSecDto,
    rtf_x: TranscribeRtfBreakdownDto,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct WordTokenDto {
    pub start: f64,
    pub end: f64,
    pub word: String,
}

pub(crate) fn transcribe_blocking<F>(
    request: TranscribeRequest,
    mut on_progress: F,
) -> Result<TranscribeResponse, String>
where
    F: FnMut(crate::services::transcribe::asr_align::TranscribeProgressStage, usize, usize, Option<crate::services::transcribe::asr_align::FreshSegmentResult>),
{
    let logger = TaskLogger::main_with_media(request.task_id.clone(), request.audio_path.clone());
    let chunk_target_seconds = request.chunk_target_seconds.clamp(30, 60);
    append_transcribe_log(
        &logger,
        event::TRANSCRIBE_STARTED,
        json!({
            "asrModel": request.asr_model,
            "alignModel": request.align_model,
            "chunkTargetSeconds": chunk_target_seconds,
            "provider": request.provider,
            "mediaPath": request.audio_path,
        }),
    );

    let output = asr_align::transcribe_with_asr_and_qwen(
        asr_align::AsrAlignRequest {
            audio_path: PathBuf::from(&request.audio_path),
            source_lang: request.source_lang.clone(),
            asr_model: normalize_asr_model(&request.asr_model),
            align_model: normalize_align_model(&request.align_model),
            provider: request.provider.clone(),
            chunk_target_seconds,
            model_dir: request.model_dir.as_ref().map(PathBuf::from),
            precomputed_asr: request.precomputed_asr_segments,
            precomputed_alignment: request.precomputed_alignment,
        },
        |stage, current, total, fresh_result| {
            on_progress(stage, current, total, fresh_result);
        },
    );
    let output = match output {
        Ok(v) => v,
        Err(raw_err) => {
            let technical = format!(
                "{} (audio: {}, modelDir: {})",
                raw_err,
                request.audio_path,
                request
                    .model_dir
                    .as_deref()
                    .unwrap_or("<resolved model directory>")
            );
            let user_message =
                map_transcribe_error(&technical, &request.provider, chunk_target_seconds);
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
    let metrics = build_phase_metrics(output.audio_duration_sec, output.timing);
    let text = output.text;
    let aligned_text = output.aligned_text;
    let words = normalize_word_tokens(output.words);

    let response = TranscribeResponse {
        words: words.iter().map(word_to_dto).collect(),
        text,
        aligned_text,
        segment_total: output.segment_summaries.len(),
        segment_durations_sec: output
            .segment_summaries
            .iter()
            .map(|s| (s.duration_sec * 100.0).round() / 100.0)
            .collect(),
        audio_duration_sec: output.audio_duration_sec,
        vad_elapsed_sec: output.vad_elapsed_sec,
        vad_speech_segments: output.vad_speech_segments,
        transcribe_elapsed_sec: output.transcribe_elapsed_sec,
        timing_sec: metrics.timing_sec,
        rtf_x: metrics.rtf_x.total,
        rtf_breakdown_x: metrics.rtf_x,
        execution_provider: output.execution_provider.to_string(),
        new_asr_segments: output.new_asr_results,
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
            "timingSec": &response.timing_sec,
            "rtfX": response.rtf_x,
            "rtfBreakdownX": &response.rtf_breakdown_x,
        }),
    );

    Ok(response)
}

fn normalize_asr_model(raw: &str) -> String {
    let value = raw.trim();
    if value.is_empty() {
        crate::services::model::DEFAULT_ASR_MODEL.to_string()
    } else {
        value.to_string()
    }
}

fn normalize_align_model(raw: &str) -> String {
    let value = raw.trim();
    if value.is_empty() {
        "Qwen3-ForcedAligner-0.6B".to_string()
    } else {
        value.to_string()
    }
}

fn word_to_dto(word: &WordToken) -> WordTokenDto {
    WordTokenDto {
        start: word.start,
        end: word.end,
        word: word.word.clone(),
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

fn build_phase_metrics(
    audio_duration_sec: f64,
    timing: asr_align::AsrAlignTiming,
) -> TranscribePhaseMetrics {
    let asr_stage_sec = timing.asr_load_sec + timing.temp_wav_write_sec + timing.asr_transcribe_sec;
    let qwen_stage_sec = timing.qwen_load_sec + timing.qwen_align_sec + timing.punctuation_map_sec;
    let model_only_sec = timing.asr_transcribe_sec + timing.qwen_align_sec;

    TranscribePhaseMetrics {
        timing_sec: TranscribeTimingSecDto {
            prepare_elapsed_sec: round2(timing.prepare_elapsed_sec),
            vad_elapsed_sec: round2(timing.vad_elapsed_sec),
            temp_wav_write_sec: round2(timing.temp_wav_write_sec),
            asr_load_sec: round2(timing.asr_load_sec),
            asr_transcribe_sec: round2(timing.asr_transcribe_sec),
            qwen_load_sec: round2(timing.qwen_load_sec),
            qwen_align_sec: round2(timing.qwen_align_sec),
            punctuation_map_sec: round2(timing.punctuation_map_sec),
            total_elapsed_sec: round2(timing.total_elapsed_sec),
        },
        rtf_x: TranscribeRtfBreakdownDto {
            total: round2(calculate_rtf_x(
                audio_duration_sec,
                timing.total_elapsed_sec,
            )),
            asr_stage: round2(calculate_rtf_x(audio_duration_sec, asr_stage_sec)),
            asr_transcribe: round2(calculate_rtf_x(
                audio_duration_sec,
                timing.asr_transcribe_sec,
            )),
            qwen_stage: round2(calculate_rtf_x(audio_duration_sec, qwen_stage_sec)),
            qwen_align: round2(calculate_rtf_x(audio_duration_sec, timing.qwen_align_sec)),
            model_only: round2(calculate_rtf_x(audio_duration_sec, model_only_sec)),
        },
    }
}

fn map_transcribe_error(raw: &str, provider: &str, chunk_target_seconds: u32) -> String {
    let lowered = raw.to_lowercase();
    let gpu_oom = lowered.contains("887a0006")
        || lowered.contains("dmlexecutionprovider")
        || lowered.contains("onnx runtime error")
            && lowered.contains("gpu")
            && lowered.contains("invalid")
        || lowered.contains("lstm node");

    if gpu_oom {
        return format!(
            "Transcription failed: insufficient GPU/video memory ({}). Please reduce 'segment duration' in settings and retry (current {} seconds, recommended 30-60).",
            if provider.eq_ignore_ascii_case("cuda") {
                "GPU"
            } else {
                provider
            },
            chunk_target_seconds.clamp(30, 60)
        );
    }

    raw.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_metrics_include_timing_and_rtf_breakdowns() {
        let metrics = build_phase_metrics(
            200.0,
            asr_align::AsrAlignTiming {
                prepare_elapsed_sec: 5.0,
                vad_elapsed_sec: 0.5,
                temp_wav_write_sec: 2.0,
                asr_load_sec: 3.0,
                asr_transcribe_sec: 10.0,
                qwen_load_sec: 4.0,
                qwen_align_sec: 20.0,
                punctuation_map_sec: 1.0,
                total_elapsed_sec: 50.0,
            },
        );

        assert_eq!(metrics.timing_sec.total_elapsed_sec, 50.0);
        assert_eq!(metrics.rtf_x.total, 4.0);
        assert_eq!(metrics.rtf_x.asr_transcribe, 20.0);
        assert_eq!(metrics.rtf_x.qwen_align, 10.0);
        assert_eq!(metrics.rtf_x.model_only, 6.67);
        assert_eq!(metrics.rtf_x.asr_stage, 13.33);
        assert_eq!(metrics.rtf_x.qwen_stage, 8.0);
    }
}
