use serde::{Deserialize, Serialize};

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
    let words = request.words.clone();
    on_phase("segment");
    let built = build_segments_from_words(BuildSegmentsRequest {
        task_id: request.task_id.clone(),
        audio_path: request.audio_path.clone(),
        words: words.clone(),
        subtitle_max_words_per_segment: request.subtitle_max_words_per_segment,
    })?;

    Ok(RunPostAsrPipelineResponse {
        text: built.text,
        srt: built.srt,
        srt_output_path: built.srt_output_path,
        segments: built.segments,
        words,
    })
}
