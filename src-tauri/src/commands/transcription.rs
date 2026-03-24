use tauri::Emitter;

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct WordTokenCommandDto {
    pub start: f64,
    pub end: f64,
    pub word: String,
}

#[derive(Debug, serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SegmentWithWordsCommandDto {
    pub start: f64,
    pub end: f64,
    pub text: String,
    pub words: Vec<WordTokenCommandDto>,
}

#[derive(Debug, serde::Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RunPostAsrPipelineCommandRequest {
    pub task_id: String,
    pub audio_path: String,
    pub words: Vec<WordTokenCommandDto>,
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

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunPostAsrPipelineCommandResponse {
    pub text: String,
    pub srt: String,
    pub srt_output_path: String,
    pub words_output_path: String,
    pub segments: Vec<SegmentWithWordsCommandDto>,
    pub words: Vec<WordTokenCommandDto>,
    pub post_asr_elapsed_sec: f64,
}

#[derive(Debug, serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct TranscribePhaseEvent {
    task_id: String,
    phase: String,
}

#[tauri::command]
pub async fn run_post_asr_pipeline(
    app: tauri::AppHandle,
    request: RunPostAsrPipelineCommandRequest,
) -> Result<RunPostAsrPipelineCommandResponse, String> {
    let task_id = request.task_id.clone();
    let service_request = crate::services::transcription::RunPostAsrPipelineRequest {
        task_id: request.task_id,
        audio_path: request.audio_path,
        words: request.words.into_iter().map(to_service_word).collect(),
        subtitle_max_words_per_segment: request.subtitle_max_words_per_segment,
        enable_punctuation_optimization: request.enable_punctuation_optimization,
        translate_api_key: request.translate_api_key,
        translate_base_url: request.translate_base_url,
        translate_model: request.translate_model,
        llm_concurrency: request.llm_concurrency,
    };

    let response = crate::services::transcription::run_post_asr_pipeline(
        service_request,
        move |phase| {
            let _ = app.emit(
                "transcribe-phase",
                TranscribePhaseEvent {
                    task_id: task_id.clone(),
                    phase: phase.to_string(),
                },
            );
        },
    )
    .await?;

    Ok(RunPostAsrPipelineCommandResponse {
        text: response.text,
        srt: response.srt,
        srt_output_path: response.srt_output_path,
        words_output_path: response.words_output_path,
        segments: response
            .segments
            .into_iter()
            .map(|segment| SegmentWithWordsCommandDto {
                start: segment.start,
                end: segment.end,
                text: segment.text,
                words: segment.words.into_iter().map(from_service_word).collect(),
            })
            .collect(),
        words: response.words.into_iter().map(from_service_word).collect(),
        post_asr_elapsed_sec: response.post_asr_elapsed_sec,
    })
}

fn to_service_word(word: WordTokenCommandDto) -> crate::services::transcribe::WordTokenDto {
    crate::services::transcribe::WordTokenDto {
        start: word.start,
        end: word.end,
        word: word.word,
    }
}

fn from_service_word(word: crate::services::transcribe::WordTokenDto) -> WordTokenCommandDto {
    WordTokenCommandDto {
        start: word.start,
        end: word.end,
        word: word.word,
    }
}

fn default_llm_concurrency() -> u32 {
    4
}
