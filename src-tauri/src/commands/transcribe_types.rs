#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscribeCommandRequest {
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
}

fn default_asr_model() -> String {
    crate::services::model::DEFAULT_ASR_MODEL.to_string()
}

fn default_align_model() -> String {
    "Qwen3-ForcedAligner-0.6B".to_string()
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscribeCommandResponse {
    pub words: Vec<WordTokenCommandDto>,
    pub text: String,
    pub aligned_text: String,
    pub segment_total: usize,
    pub segment_durations_sec: Vec<f64>,
    pub audio_duration_sec: f64,
    pub vad_elapsed_sec: f64,
    pub transcribe_elapsed_sec: f64,
    pub timing_sec: crate::services::transcribe::TranscribeTimingSecDto,
    pub rtf_x: f64,
    pub rtf_breakdown_x: crate::services::transcribe::TranscribeRtfBreakdownDto,
    pub execution_provider: String,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct WordTokenCommandDto {
    pub start: f64,
    pub end: f64,
    pub word: String,
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
pub(super) struct TranscribeProgressEvent {
    pub(super) task_id: String,
    pub(super) phase: String,
    pub(super) current_segment: usize,
    pub(super) total_segments: usize,
}

#[derive(Debug, serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(super) struct SeparateProgressEvent {
    pub(super) task_id: String,
    pub(super) percent: u32,
}
