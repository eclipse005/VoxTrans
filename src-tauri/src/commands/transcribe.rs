use tauri::Emitter;
use tauri::async_runtime::spawn_blocking;

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscribeCommandRequest {
    pub task_id: String,
    pub audio_path: String,
    pub provider: String,
    pub chunk_target_seconds: u32,
    pub model_dir: Option<String>,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscribeCommandResponse {
    pub words: Vec<WordTokenCommandDto>,
    pub segment_total: usize,
    pub segment_durations_sec: Vec<f64>,
    pub audio_duration_sec: f64,
    pub vad_elapsed_sec: f64,
    pub transcribe_elapsed_sec: f64,
    pub execution_provider: String,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct WordTokenCommandDto {
    pub start: f64,
    pub end: f64,
    pub word: String,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SegmentWithWordsCommandDto {
    pub start: f64,
    pub end: f64,
    pub text: String,
    pub words: Vec<WordTokenCommandDto>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildSegmentsCommandRequest {
    pub task_id: String,
    pub audio_path: String,
    pub words: Vec<WordTokenCommandDto>,
    pub subtitle_max_words_per_segment: u32,
    #[serde(default)]
    pub segment_mode: String,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildSegmentsCommandResponse {
    pub text: String,
    pub srt: String,
    pub srt_output_path: String,
    pub segments: Vec<SegmentWithWordsCommandDto>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeparateVocalsCommandRequest {
    pub task_id: String,
    pub audio_path: String,
    pub model: String,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SeparateVocalsCommandResponse {
    pub vocals_path: String,
}

#[derive(Debug, serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct TranscribeProgressEvent {
    task_id: String,
    current_segment: usize,
    total_segments: usize,
}

#[derive(Debug, serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct SeparateProgressEvent {
    task_id: String,
    percent: u32,
}

#[tauri::command]
pub async fn transcribe(
    app: tauri::AppHandle,
    request: TranscribeCommandRequest,
) -> Result<TranscribeCommandResponse, String> {
    spawn_blocking(move || {
        let task_id = request.task_id.clone();
        let app_handle = app.clone();
        crate::services::transcribe::transcribe_blocking(
            crate::services::transcribe::TranscribeRequest {
                task_id: request.task_id,
                audio_path: request.audio_path,
                provider: request.provider,
                chunk_target_seconds: request.chunk_target_seconds,
                model_dir: request.model_dir,
            },
            move |current, total| {
                let _ = app_handle.emit(
                    "transcribe-progress",
                    TranscribeProgressEvent {
                        task_id: task_id.clone(),
                        current_segment: current,
                        total_segments: total,
                    },
                );
            },
        )
        .map(|response| TranscribeCommandResponse {
            words: response.words.into_iter().map(from_service_word).collect(),
            segment_total: response.segment_total,
            segment_durations_sec: response.segment_durations_sec,
            audio_duration_sec: response.audio_duration_sec,
            vad_elapsed_sec: response.vad_elapsed_sec,
            transcribe_elapsed_sec: response.transcribe_elapsed_sec,
            execution_provider: response.execution_provider,
        })
    })
    .await
    .map_err(|err| err.to_string())?
}

#[tauri::command]
pub fn build_segments_from_words(
    request: BuildSegmentsCommandRequest,
) -> Result<BuildSegmentsCommandResponse, String> {
    crate::services::transcribe::build_segments_from_words(
        crate::services::transcribe::BuildSegmentsRequest {
            task_id: request.task_id,
            audio_path: request.audio_path,
            words: request.words.into_iter().map(to_service_word).collect(),
            subtitle_max_words_per_segment: request.subtitle_max_words_per_segment,
            segment_mode: request.segment_mode,
        },
    )
    .map(|response| BuildSegmentsCommandResponse {
        text: response.text,
        srt: response.srt,
        srt_output_path: response.srt_output_path,
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
    })
}

#[tauri::command]
pub async fn separate_vocals(
    app: tauri::AppHandle,
    request: SeparateVocalsCommandRequest,
) -> Result<SeparateVocalsCommandResponse, String> {
    spawn_blocking(move || {
        let task_id = request.task_id.clone();
        let app_handle = app.clone();
        crate::services::demucs::separate_vocals_blocking(
            crate::services::demucs::SeparateVocalsRequest {
                task_id: request.task_id,
                audio_path: request.audio_path,
                model: request.model,
            },
            move |percent| {
                let _ = app_handle.emit(
                    "separate-progress",
                    SeparateProgressEvent {
                        task_id: task_id.clone(),
                        percent,
                    },
                );
            },
        )
        .map(|response| SeparateVocalsCommandResponse {
            vocals_path: response.vocals_path,
        })
    })
    .await
    .map_err(|err| err.to_string())?
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
