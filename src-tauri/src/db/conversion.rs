//! Row <-> business object conversion.
//!
//! `from_business` and `to_business` are pure functions, tested in isolation.
//! They do not touch the database.

use crate::commands::workspace::{
    WorkspaceQueueItem, WorkspaceTaskProgressState, WorkspaceTaskStageState,
};
use crate::db::models::{SettingsRow, SubtitleSegmentRow, SubtitleWordRow, TaskRow};
use crate::services::preferences_types::{
    SavedSettings, SubtitleLayoutStyle, SubtitleLineStyle, SubtitleRenderStyle,
};
use crate::services::workspace_subtitle::WorkspaceSubtitleSegment;

pub fn settings_from_row(row: SettingsRow) -> SavedSettings {
    SavedSettings {
        provider: row.provider,
        chunk_target_seconds: row.chunk_target_seconds,
        subtitle_length_preset: row.subtitle_length_preset,
        asr_model: row.asr_model,
        align_model: row.align_model,
        demucs_model: row.demucs_model,
        enable_vocal_separation: row.enable_vocal_separation,
        translate_api_key: row.translate_api_key,
        translate_base_url: row.translate_base_url,
        translate_model: row.translate_model,
        llm_concurrency: row.llm_concurrency,
        // terminology_groups is composed in store.rs from terminology tables.
        terminology_groups: Vec::new(),
        enable_terminology: row.enable_terminology,
        enable_subtitle_beautify: row.enable_subtitle_beautify,
        enable_click_sound: row.enable_click_sound,
        auto_burn_hard_subtitle: row.auto_burn_hard_subtitle,
        subtitle_burn_mode: row.subtitle_burn_mode,
        subtitle_render_style: SubtitleRenderStyle {
            source: SubtitleLineStyle {
                font_family: row.source_font_family,
                font_size: row.source_font_size,
                primary_color: row.source_primary_color,
                outline_color: row.source_outline_color,
                back_color: row.source_back_color,
                outline: row.source_outline,
                shadow: row.source_shadow,
                border_style: row.source_border_style,
                border_opacity: row.source_border_opacity,
            },
            target: SubtitleLineStyle {
                font_family: row.target_font_family,
                font_size: row.target_font_size,
                primary_color: row.target_primary_color,
                outline_color: row.target_outline_color,
                back_color: row.target_back_color,
                outline: row.target_outline,
                shadow: row.target_shadow,
                border_style: row.target_border_style,
                border_opacity: row.target_border_opacity,
            },
            layout: SubtitleLayoutStyle {
                margin_v: row.margin_v,
                alignment: row.alignment,
                bilingual_line_gap: row.bilingual_line_gap,
            },
        },
        flat_srt_output: row.flat_srt_output,
        // flat_srt_items is composed in store.rs from the flat_srt_items table.
        flat_srt_items: Vec::new(),
    }
}

pub fn row_from_settings(settings: &SavedSettings) -> SettingsRow {
    SettingsRow {
        provider: settings.provider.clone(),
        chunk_target_seconds: settings.chunk_target_seconds,
        subtitle_length_preset: settings.subtitle_length_preset.clone(),
        asr_model: settings.asr_model.clone(),
        align_model: settings.align_model.clone(),
        demucs_model: settings.demucs_model.clone(),
        enable_vocal_separation: settings.enable_vocal_separation,
        translate_api_key: settings.translate_api_key.clone(),
        translate_base_url: settings.translate_base_url.clone(),
        translate_model: settings.translate_model.clone(),
        llm_concurrency: settings.llm_concurrency,
        enable_terminology: settings.enable_terminology,
        enable_subtitle_beautify: settings.enable_subtitle_beautify,
        enable_click_sound: settings.enable_click_sound,
        auto_burn_hard_subtitle: settings.auto_burn_hard_subtitle,
        subtitle_burn_mode: settings.subtitle_burn_mode.clone(),
        source_font_family: settings.subtitle_render_style.source.font_family.clone(),
        source_font_size: settings.subtitle_render_style.source.font_size,
        source_primary_color: settings.subtitle_render_style.source.primary_color.clone(),
        source_outline_color: settings.subtitle_render_style.source.outline_color.clone(),
        source_back_color: settings.subtitle_render_style.source.back_color.clone(),
        source_outline: settings.subtitle_render_style.source.outline,
        source_shadow: settings.subtitle_render_style.source.shadow,
        source_border_style: settings.subtitle_render_style.source.border_style.clone(),
        source_border_opacity: settings.subtitle_render_style.source.border_opacity,
        target_font_family: settings.subtitle_render_style.target.font_family.clone(),
        target_font_size: settings.subtitle_render_style.target.font_size,
        target_primary_color: settings.subtitle_render_style.target.primary_color.clone(),
        target_outline_color: settings.subtitle_render_style.target.outline_color.clone(),
        target_back_color: settings.subtitle_render_style.target.back_color.clone(),
        target_outline: settings.subtitle_render_style.target.outline,
        target_shadow: settings.subtitle_render_style.target.shadow,
        target_border_style: settings.subtitle_render_style.target.border_style.clone(),
        target_border_opacity: settings.subtitle_render_style.target.border_opacity,
        margin_v: settings.subtitle_render_style.layout.margin_v,
        alignment: settings.subtitle_render_style.layout.alignment,
        bilingual_line_gap: settings.subtitle_render_style.layout.bilingual_line_gap,
        flat_srt_output: settings.flat_srt_output,
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
#[derive(Debug, Clone, Default)]
pub struct TaskMetaExtras {
    pub intent: String,
    pub max_retries: u32,
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
    };
    let extras = TaskMetaExtras {
        intent: row.intent,
        max_retries: row.max_retries,
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
        updated_at: now_ms(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_settings() -> SavedSettings {
        SavedSettings {
            provider: "openai".into(),
            chunk_target_seconds: 30,
            subtitle_length_preset: "default".into(),
            asr_model: "Qwen3-ASR".into(),
            align_model: "Qwen3-ForcedAligner-0.6B".into(),
            demucs_model: "htdemucs".into(),
            enable_vocal_separation: true,
            translate_api_key: "k".into(),
            translate_base_url: "https://api.example.com".into(),
            translate_model: "gpt-4o".into(),
            llm_concurrency: 4,
            terminology_groups: Vec::new(),
            enable_terminology: true,
            enable_subtitle_beautify: true,
            enable_click_sound: true,
            auto_burn_hard_subtitle: false,
            subtitle_burn_mode: "bilingualSourceFirst".into(),
            subtitle_render_style: SubtitleRenderStyle::default(),
            flat_srt_output: false,
            flat_srt_items: Vec::new(),
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
        assert_eq!(restored.enable_terminology, original.enable_terminology);
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
        };
        let extras = TaskMetaExtras {
            intent: "TRANSCRIBE_TRANSLATE".into(),
            max_retries: 3,
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

        assert_eq!(restored_item.subtitle_segments_json, String::new());

        // Wrapper extras roundtrip.
        assert_eq!(restored_extras.intent, "TRANSCRIBE_TRANSLATE");
        assert_eq!(restored_extras.max_retries, 3);
    }
}
