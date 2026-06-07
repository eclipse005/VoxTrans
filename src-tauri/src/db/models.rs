//! Row structs mirroring the 7 tables in migrations/20260607000001_init.sql.
//!
//! Each struct is `pub` and derives `Debug, Clone`. Field names match SQL
//! column names exactly. Conversions to/from business objects live in
//! `super::conversion`.

#[derive(Debug, Clone)]
pub struct SettingsRow {
    pub provider: String,
    pub chunk_target_seconds: u32,
    pub subtitle_length_preset: String,
    pub asr_model: String,
    pub align_model: String,
    pub demucs_model: String,
    pub enable_vocal_separation: bool,
    pub translate_api_key: String,
    pub translate_base_url: String,
    pub translate_model: String,
    pub llm_concurrency: u32,
    pub enable_terminology: bool,
    pub enable_subtitle_beautify: bool,
    pub enable_click_sound: bool,
    pub auto_burn_hard_subtitle: bool,
    pub subtitle_burn_mode: String,
    pub source_font_family: String,
    pub source_font_size: u32,
    pub source_primary_color: String,
    pub source_outline_color: String,
    pub source_back_color: String,
    pub source_outline: f64,
    pub source_shadow: f64,
    pub source_border_style: String,
    pub source_border_opacity: u8,
    pub target_font_family: String,
    pub target_font_size: u32,
    pub target_primary_color: String,
    pub target_outline_color: String,
    pub target_back_color: String,
    pub target_outline: f64,
    pub target_shadow: f64,
    pub target_border_style: String,
    pub target_border_opacity: u8,
    pub margin_v: u32,
    pub alignment: u8,
    pub bilingual_line_gap: u32,
    pub flat_srt_output: bool,
    pub updated_at: i64,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct FlatSrtItemRow {
    pub id: i64,
    pub value: String,
    pub updated_at: i64,
}

#[derive(Debug, Clone)]
pub struct TaskRow {
    pub id: String,
    pub media_path: String,
    pub name: String,
    pub media_kind: String,
    pub size_bytes: u64,
    pub source_lang: String,
    pub target_lang: String,
    pub transcribe_status: String,
    pub task_progress_stage_code: String,
    pub task_progress_stage_label: String,
    pub task_progress_stage_order: u32,
    pub task_progress_detail: String,
    pub task_progress_current: u32,
    pub task_progress_total: u32,
    pub transcribe_error: String,
    pub result_text: String,
    pub result_srt: String,
    pub llm_total_tokens: u64,
    pub intent: String,
    pub max_retries: u32,
    pub settings_snapshot_provider: String,
    pub settings_snapshot_asr_model: String,
    pub settings_snapshot_align_model: String,
    pub settings_snapshot_demucs_model: String,
    pub settings_snapshot_translate_api_key: String,
    pub settings_snapshot_translate_base_url: String,
    pub settings_snapshot_translate_model: String,
    pub settings_snapshot_llm_concurrency: u32,
    pub settings_snapshot_chunk_target_seconds: u32,
    pub settings_snapshot_enable_vocal_separation: bool,
    pub updated_at: i64,
}

#[derive(Debug, Clone)]
pub struct SubtitleSegmentRow {
    pub id: String,
    pub task_id: String,
    pub idx: i32,
    pub start_ms: u64,
    pub end_ms: u64,
    pub source_text: String,
    pub translated_text: String,
    pub updated_at: i64,
}

#[derive(Debug, Clone)]
pub struct SubtitleWordRow {
    pub id: String,
    pub segment_id: String,
    pub idx: i32,
    pub start_ms: u64,
    pub end_ms: u64,
    pub word: String,
    pub updated_at: i64,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct TerminologyGroupRow {
    pub id: String,
    pub name: String,
    pub updated_at: i64,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct TerminologyTermRow {
    pub id: String,
    pub group_id: String,
    pub origin: String,
    pub target: String,
    pub note: String,
    pub updated_at: i64,
}
