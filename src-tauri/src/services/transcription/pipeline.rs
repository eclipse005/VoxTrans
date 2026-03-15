use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::services::task_log::{TaskLogTarget, append_event_best_effort, event};
use crate::services::transcribe::{
    BuildSegmentsRequest, SegmentWithWordsDto, WordTokenDto, build_segments_from_words,
};

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RunPostAsrPipelineRequest {
    pub task_id: String,
    pub audio_path: String,
    pub words: Vec<WordTokenDto>,
    pub subtitle_max_words_per_segment: u32,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunPostAsrPipelineResponse {
    pub text: String,
    pub srt: String,
    pub srt_output_path: String,
    pub segments: Vec<SegmentWithWordsDto>,
    pub words: Vec<WordTokenDto>,
}

pub async fn run_post_asr_pipeline<F>(
    request: RunPostAsrPipelineRequest,
    mut on_phase: F,
) -> Result<RunPostAsrPipelineResponse, String>
where
    F: FnMut(&str),
{
    let log_target = TaskLogTarget::main(request.task_id.clone(), request.audio_path.clone());
    let started_at = std::time::Instant::now();
    append_event_best_effort(
        &log_target,
        event::TRANSCRIBE_POST_ASR_STARTED,
        Some(&json!({
            "wordCount": request.words.len(),
            "subtitleMaxWordsPerSegment": request.subtitle_max_words_per_segment,
        })),
    );

    let words = request.words.clone();
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
            append_event_best_effort(
                &log_target,
                event::TRANSCRIBE_FAILED,
                Some(&json!({
                    "phase": "post_asr",
                    "error": err,
                })),
            );
            return Err(err);
        }
    };

    append_event_best_effort(
        &log_target,
        event::TRANSCRIBE_POST_ASR_COMPLETED,
        Some(&json!({
            "segmentTotal": built.segments.len(),
            "elapsedSec": round2(started_at.elapsed().as_secs_f64()),
        })),
    );

    Ok(RunPostAsrPipelineResponse {
        text: built.text,
        srt: built.srt,
        srt_output_path: built.srt_output_path,
        segments: built.segments,
        words,
    })
}

fn round2(value: f64) -> f64 {
    if !value.is_finite() {
        return 0.0;
    }
    (value * 100.0).round() / 100.0
}
