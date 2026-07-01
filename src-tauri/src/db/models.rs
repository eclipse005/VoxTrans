//! Row structs mirroring the SQLite schema.
//!
//! Each struct derives `sqlx::FromRow` so reads use `query_as`, removing
//! the manual `r.get::<i64, _>("col") as u32` boilerplate. Conversions to
//! business objects live in `super::conversion`.

use crate::services::preferences_types::SubtitleRenderStyle;

/// One row of the `settings` table (singleton, id=1).
///
/// `subtitle_render_style` is decoded from the `subtitle_render_style_json`
/// TEXT column via `#[sqlx(json)]` — the 18 per-field columns it used to
/// occupy were collapsed in migration 20260611000001.
#[derive(Debug, Clone, sqlx::FromRow)]
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
    pub active_terminology_group_id: String,
    pub enable_subtitle_beautify: bool,
    pub enable_click_sound: bool,
    pub auto_burn_hard_subtitle: bool,
    pub subtitle_burn_mode: String,
    #[sqlx(json, rename = "subtitle_render_style_json")]
    pub subtitle_render_style: SubtitleRenderStyle,
    pub flat_srt_output: bool,
    pub updated_at: i64,
}

#[derive(Debug, Clone, sqlx::FromRow)]
#[allow(dead_code)]
pub struct FlatSrtItemRow {
    pub id: i64,
    pub value: String,
    pub updated_at: i64,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct TaskRow {
    pub id: String,
    pub media_path: String,
    pub name: String,
    pub media_kind: String,
    #[sqlx(try_from = "i64")]
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
    #[sqlx(try_from = "i64")]
    pub llm_total_tokens: u64,
    pub intent: String,
    pub max_retries: u32,
    // Frozen-at-enqueue settings (see migration 20260610000001):
    pub subtitle_length_preset: String,
    pub enable_subtitle_beautify: bool,
    /// JSON-serialized `Vec<TerminologyGroup>` snapshot taken at enqueue
    /// time. This is the FROZEN copy that the pipeline reads during
    /// execution -- editing the global terminology library (the
    /// `terminology_groups` / `terminology_terms` tables that
    /// `save_settings` writes) does NOT affect already-enqueued tasks.
    /// See the `terminology_frozen_contract_no_cross_writes` test in
    /// `db::store` for the invariant.
    pub terminology_groups_json: String,
    /// Per-task selected terminology group id ("" = none). The matching
    /// group's terms are frozen into `terminology_groups_json` at enqueue.
    pub terminology_group_id: String,
    /// 入队顺序，单调递增；只在 INSERT 时赋值，ON CONFLICT 不更新。
    /// `load_all_tasks` 按 `ORDER BY enqueue_seq ASC` 返回稳定顺序。
    pub enqueue_seq: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SubtitleSegmentRow {
    pub id: String,
    pub task_id: String,
    pub idx: i32,
    #[sqlx(try_from = "i64")]
    pub start_ms: u64,
    #[sqlx(try_from = "i64")]
    pub end_ms: u64,
    pub source_text: String,
    pub translated_text: String,
    pub updated_at: i64,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SubtitleWordRow {
    pub id: String,
    pub segment_id: String,
    pub idx: i32,
    #[sqlx(try_from = "i64")]
    pub start_ms: u64,
    #[sqlx(try_from = "i64")]
    pub end_ms: u64,
    pub word: String,
    pub updated_at: i64,
}

#[derive(Debug, Clone, sqlx::FromRow)]
#[allow(dead_code)]
pub struct TerminologyGroupRow {
    pub id: String,
    pub name: String,
    pub updated_at: i64,
}

#[derive(Debug, Clone, sqlx::FromRow)]
#[allow(dead_code)]
pub struct TerminologyTermRow {
    pub id: String,
    pub group_id: String,
    pub origin: String,
    pub target: String,
    pub note: String,
    pub updated_at: i64,
}
