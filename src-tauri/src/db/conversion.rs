//! Row <-> business object conversion.
//!
//! `from_business` and `to_business` are pure functions, tested in isolation.
//! They do not touch the database.

use crate::commands::workspace::{
    WorkspaceQueueItem, WorkspaceTaskProgressState, WorkspaceTaskStageState,
};
use crate::db::models::{SettingsRow, SubtitleSegmentRow, SubtitleWordRow, TaskRow};
use crate::services::preferences_types::{
    AlignModel, AsrModel, DemucsModel, Locale, Provider, SavedSettings, SubtitleBurnMode,
    SubtitleLengthPreset,
};
use crate::services::workspace_subtitle::WorkspaceSubtitleSegment;

pub fn settings_from_row(row: SettingsRow) -> SavedSettings {
    SavedSettings {
        provider: Provider::parse(&row.provider),
        chunk_target_seconds: row.chunk_target_seconds,
        subtitle_length_preset: SubtitleLengthPreset::parse(&row.subtitle_length_preset),
        asr_model: AsrModel::parse(&row.asr_model),
        align_model: AlignModel::parse(&row.align_model),
        demucs_model: DemucsModel::parse(&row.demucs_model),
        enable_vocal_separation: row.enable_vocal_separation,
        translate_api_key: row.translate_api_key,
        translate_base_url: row.translate_base_url,
        translate_model: row.translate_model,
        llm_concurrency: row.llm_concurrency,
        // terminology_groups is composed in store.rs from terminology tables.
        terminology_groups: Vec::new(),
        active_terminology_group_id: row.active_terminology_group_id,
        enable_subtitle_beautify: row.enable_subtitle_beautify,
        enable_click_sound: row.enable_click_sound,
        auto_burn_hard_subtitle: row.auto_burn_hard_subtitle,
        subtitle_burn_mode: SubtitleBurnMode::parse(&row.subtitle_burn_mode),
        subtitle_render_style: row.subtitle_render_style,
        flat_srt_output: row.flat_srt_output,
        enable_vision_assist: row.enable_vision_assist,
        locale: Locale::parse(&row.locale),
        // flat_srt_items is composed in store.rs from the flat_srt_items table.
        flat_srt_items: Vec::new(),
        models_dir: row.models_dir,
    }
}

pub fn row_from_settings(settings: &SavedSettings) -> SettingsRow {
    SettingsRow {
        provider: settings.provider.as_str().to_string(),
        chunk_target_seconds: settings.chunk_target_seconds,
        subtitle_length_preset: settings.subtitle_length_preset.as_str().to_string(),
        asr_model: settings.asr_model.as_str().to_string(),
        align_model: settings.align_model.as_str().to_string(),
        demucs_model: settings.demucs_model.as_str().to_string(),
        enable_vocal_separation: settings.enable_vocal_separation,
        translate_api_key: settings.translate_api_key.clone(),
        translate_base_url: settings.translate_base_url.clone(),
        translate_model: settings.translate_model.clone(),
        llm_concurrency: settings.llm_concurrency,
        active_terminology_group_id: settings.active_terminology_group_id.clone(),
        enable_subtitle_beautify: settings.enable_subtitle_beautify,
        enable_click_sound: settings.enable_click_sound,
        auto_burn_hard_subtitle: settings.auto_burn_hard_subtitle,
        subtitle_burn_mode: settings.subtitle_burn_mode.as_str().to_string(),
        subtitle_render_style: settings.subtitle_render_style.clone(),
        flat_srt_output: settings.flat_srt_output,
        enable_vision_assist: settings.enable_vision_assist,
        locale: settings.locale.as_str().to_string(),
        models_dir: settings.models_dir.clone(),
        updated_at: now_ms(),
    }
}

fn now_ms() -> i64 {
    crate::db::now_ms()
}

pub fn row_from_segment(
    task_id: &str,
    idx: i32,
    seg: &WorkspaceSubtitleSegment,
) -> (SubtitleSegmentRow, Vec<SubtitleWordRow>) {
    let segment_id = format!("{task_id}-seg-{idx}");
    let now = now_ms();
    let seg_row = SubtitleSegmentRow {
        id: segment_id.clone(),
        task_id: task_id.to_string(),
        idx,
        start_ms: seg.start_ms,
        end_ms: seg.end_ms,
        source_text: seg.source_text.clone(),
        translated_text: seg.translated_text.clone(),
        updated_at: now,
    };
    let word_rows: Vec<SubtitleWordRow> = seg
        .source_words
        .iter()
        .enumerate()
        .map(|(i, w)| SubtitleWordRow {
            id: format!("{segment_id}-word-{i}"),
            segment_id: segment_id.clone(),
            idx: i as i32,
            start_ms: w.start_ms,
            end_ms: w.end_ms,
            word: w.word.clone(),
            updated_at: now,
        })
        .collect();
    (seg_row, word_rows)
}

/// Wrapper-only fields that live on `WorkspaceTaskRecord` but not on
/// `WorkspaceQueueItem`. `task_from_row` returns them alongside the queue
/// item so the hydrate path can rebuild a complete record.
#[derive(Debug, Clone)]
pub struct TaskMetaExtras {
    pub intent: String,
    pub max_retries: u32,
    pub subtitle_length_preset: String,
    pub enable_subtitle_beautify: bool,
    /// JSON-serialized `Vec<TerminologyGroup>` (frozen at enqueue time).
    pub terminology_groups_json: String,
    /// 入队顺序号；INSERT 时由 `next_enqueue_seq` 取号写入，UPDATE 不变。
    pub enqueue_seq: i64,
}

impl Default for TaskMetaExtras {
    fn default() -> Self {
        Self {
            intent: String::new(),
            max_retries: 0,
            subtitle_length_preset: String::new(),
            enable_subtitle_beautify: true,
            terminology_groups_json: "[]".to_string(),
            enqueue_seq: 0,
        }
    }
}

pub fn task_from_row(row: TaskRow) -> (WorkspaceQueueItem, TaskMetaExtras) {
    let item = WorkspaceQueueItem {
        id: row.id,
        path: row.media_path,
        name: row.name,
        media_kind: row.media_kind,
        size_bytes: row.size_bytes,
        source_lang: row.source_lang,
        target_lang: row.target_lang,
        transcribe_status: row.transcribe_status,
        task_progress: WorkspaceTaskProgressState {
            stage: WorkspaceTaskStageState {
                code: row.task_progress_stage_code,
                label: row.task_progress_stage_label,
                order: row.task_progress_stage_order,
                detail: row.task_progress_detail,
                current: row.task_progress_current,
                total: row.task_progress_total,
            },
        },
        transcribe_error: row.transcribe_error,
        result_text: row.result_text,
        result_srt: row.result_srt,
        subtitle_segments_json: String::new(), // filled by meta.rs during hydrate or by callers directly
        llm_total_tokens: row.llm_total_tokens,
        terminology_group_id: row.terminology_group_id,
    };
    let extras = TaskMetaExtras {
        intent: row.intent,
        max_retries: row.max_retries,
        subtitle_length_preset: row.subtitle_length_preset,
        enable_subtitle_beautify: row.enable_subtitle_beautify,
        terminology_groups_json: row.terminology_groups_json,
        enqueue_seq: row.enqueue_seq,
    };
    (item, extras)
}

pub fn row_from_task(item: &WorkspaceQueueItem, extras: &TaskMetaExtras) -> TaskRow {
    TaskRow {
        id: item.id.clone(),
        media_path: item.path.clone(),
        name: item.name.clone(),
        media_kind: item.media_kind.clone(),
        size_bytes: item.size_bytes,
        source_lang: item.source_lang.clone(),
        target_lang: item.target_lang.clone(),
        transcribe_status: item.transcribe_status.clone(),
        task_progress_stage_code: item.task_progress.stage.code.clone(),
        task_progress_stage_label: item.task_progress.stage.label.clone(),
        task_progress_stage_order: item.task_progress.stage.order,
        task_progress_detail: item.task_progress.stage.detail.clone(),
        task_progress_current: item.task_progress.stage.current,
        task_progress_total: item.task_progress.stage.total,
        transcribe_error: item.transcribe_error.clone(),
        result_text: item.result_text.clone(),
        result_srt: item.result_srt.clone(),
        llm_total_tokens: item.llm_total_tokens,
        intent: extras.intent.clone(),
        max_retries: extras.max_retries,
        subtitle_length_preset: extras.subtitle_length_preset.clone(),
        enable_subtitle_beautify: extras.enable_subtitle_beautify,
        terminology_groups_json: extras.terminology_groups_json.clone(),
        terminology_group_id: item.terminology_group_id.clone(),
        enqueue_seq: extras.enqueue_seq,
        updated_at: now_ms(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::preferences_types::{
        AlignModel, AsrModel, DemucsModel, Provider, SubtitleBurnMode, SubtitleLengthPreset,
        SubtitleRenderStyle,
    };

    fn sample_settings() -> SavedSettings {
        SavedSettings {
            provider: Provider::Cpu,
            chunk_target_seconds: 30,
            subtitle_length_preset: SubtitleLengthPreset::Standard,
            asr_model: AsrModel::Qwen3Asr06B,
            align_model: AlignModel::Qwen3ForcedAligner06B,
            demucs_model: DemucsModel::HtdemucsFt,
            enable_vocal_separation: true,
            translate_api_key: "k".into(),
            translate_base_url: "https://api.example.com".into(),
            translate_model: "gpt-4o".into(),
            llm_concurrency: 4,
            terminology_groups: Vec::new(),
            active_terminology_group_id: String::new(),
            enable_subtitle_beautify: true,
            enable_click_sound: true,
            auto_burn_hard_subtitle: false,
            subtitle_burn_mode: SubtitleBurnMode::BilingualSourceFirst,
            subtitle_render_style: SubtitleRenderStyle::default(),
            flat_srt_output: false,
            flat_srt_items: Vec::new(),
            enable_vision_assist: false,
            locale: Locale::ZhCn,
            models_dir: None,
        }
    }

    #[test]
    fn settings_roundtrip_preserves_all_fields() {
        let original = sample_settings();
        let row = row_from_settings(&original);
        let restored = settings_from_row(row);

        assert_eq!(restored.provider, original.provider);
        assert_eq!(restored.chunk_target_seconds, original.chunk_target_seconds);
        assert_eq!(restored.subtitle_length_preset, original.subtitle_length_preset);
        assert_eq!(restored.asr_model, original.asr_model);
        assert_eq!(restored.align_model, original.align_model);
        assert_eq!(restored.demucs_model, original.demucs_model);
        assert_eq!(restored.enable_vocal_separation, original.enable_vocal_separation);
        assert_eq!(restored.translate_api_key, original.translate_api_key);
        assert_eq!(restored.translate_base_url, original.translate_base_url);
        assert_eq!(restored.translate_model, original.translate_model);
        assert_eq!(restored.llm_concurrency, original.llm_concurrency);
        assert_eq!(restored.active_terminology_group_id, original.active_terminology_group_id);
        assert_eq!(restored.enable_subtitle_beautify, original.enable_subtitle_beautify);
        assert_eq!(restored.enable_click_sound, original.enable_click_sound);
        assert_eq!(restored.auto_burn_hard_subtitle, original.auto_burn_hard_subtitle);
        assert_eq!(restored.subtitle_burn_mode, original.subtitle_burn_mode);
        assert_eq!(restored.flat_srt_output, original.flat_srt_output);

        // nested SubtitleRenderStyle.source
        assert_eq!(restored.subtitle_render_style.source.font_family, original.subtitle_render_style.source.font_family);
        assert_eq!(restored.subtitle_render_style.source.font_size, original.subtitle_render_style.source.font_size);
        assert_eq!(restored.subtitle_render_style.source.outline, original.subtitle_render_style.source.outline);

        // nested SubtitleRenderStyle.target
        assert_eq!(restored.subtitle_render_style.target.font_family, original.subtitle_render_style.target.font_family);

        // nested SubtitleRenderStyle.layout
        assert_eq!(restored.subtitle_render_style.layout.margin_v, original.subtitle_render_style.layout.margin_v);
        assert_eq!(restored.subtitle_render_style.layout.alignment, original.subtitle_render_style.layout.alignment);
        assert_eq!(restored.subtitle_render_style.layout.bilingual_line_gap, original.subtitle_render_style.layout.bilingual_line_gap);

        // composed fields are empty here (filled by store.rs)
        assert!(restored.terminology_groups.is_empty());
        assert!(restored.flat_srt_items.is_empty());
    }

    #[test]
    fn task_roundtrip_preserves_core_fields() {
        let original = WorkspaceQueueItem {
            id: "task-1".into(),
            path: "/tmp/a.mp3".into(),
            name: "a.mp3".into(),
            media_kind: "audio".into(),
            size_bytes: 1024,
            source_lang: "en".into(),
            target_lang: "zh-CN".into(),
            transcribe_status: "processing".into(),
            task_progress: WorkspaceTaskProgressState {
                stage: WorkspaceTaskStageState {
                    code: "recognizing".into(),
                    label: "语音识别中".into(),
                    order: 30,
                    detail: "chunk 5/10".into(),
                    current: 5,
                    total: 10,
                },
            },
            transcribe_error: "".into(),
            result_text: "hello".into(),
            result_srt: "1\n00:00:00,000 --> 00:00:01,000\nhello".into(),
            subtitle_segments_json: "[]".into(),
            llm_total_tokens: 42,
            terminology_group_id: "g1".into(),
        };
        let extras = TaskMetaExtras {
            intent: "TRANSCRIBE_TRANSLATE".into(),
            max_retries: 3,
            subtitle_length_preset: "long".into(),
            enable_subtitle_beautify: false,
            terminology_groups_json: r#"[{"id":"g","name":"x","terms":[]}]"#.into(),
            enqueue_seq: 0,
        };
        let row = row_from_task(&original, &extras);
        let (restored_item, restored_extras) = task_from_row(row);

        assert_eq!(restored_item.id, original.id);
        assert_eq!(restored_item.path, original.path);
        assert_eq!(restored_item.transcribe_status, original.transcribe_status);
        assert_eq!(restored_item.task_progress.stage.code, original.task_progress.stage.code);
        assert_eq!(restored_item.task_progress.stage.label, original.task_progress.stage.label);
        assert_eq!(restored_item.task_progress.stage.order, original.task_progress.stage.order);
        assert_eq!(restored_item.task_progress.stage.detail, original.task_progress.stage.detail);
        assert_eq!(restored_item.task_progress.stage.current, original.task_progress.stage.current);
        assert_eq!(restored_item.task_progress.stage.total, original.task_progress.stage.total);
        assert_eq!(restored_item.llm_total_tokens, original.llm_total_tokens);
        assert_eq!(restored_item.terminology_group_id, original.terminology_group_id);

        assert_eq!(restored_item.subtitle_segments_json, String::new());

        // Wrapper extras roundtrip.
        assert_eq!(restored_extras.intent, "TRANSCRIBE_TRANSLATE");
        assert_eq!(restored_extras.max_retries, 3);
        assert_eq!(restored_extras.subtitle_length_preset, "long");
        assert!(!restored_extras.enable_subtitle_beautify);
        assert_eq!(
            restored_extras.terminology_groups_json,
            r#"[{"id":"g","name":"x","terms":[]}]"#
        );
    }
}
