use std::path::Path;

use tauri::AppHandle;

use crate::commands::translate_types::BuildTranslationSegmentCommand;
use crate::services::workspace_subtitle::{WorkspaceSubtitleSegment, serialize_segments};

use super::adapters::{
    workspace_subtitle_segments_from_step2_segments,
    workspace_subtitle_segments_from_translation_segments,
};
use super::patch_task_item;
use super::progress::done_task_progress_state;

pub(super) fn finish_transcribe_only(
    app: &AppHandle,
    task_id: &str,
    media_path: &str,
    step2_segments: &[crate::commands::transcription::GroupedSentenceSegmentCommandDto],
    step2_srt: String,
    source_text: String,
    enable_subtitle_beautify: bool,
    subtitle_length_reference: u32,
    target_lang: &str,
) -> Result<(), String> {
    let workspace_segments = workspace_subtitle_segments_from_step2_segments(step2_segments);
    let subtitle_segments_json = serialize_segments(&workspace_segments);
    write_completion_srts(
        task_id,
        media_path,
        &workspace_segments,
        false,
        enable_subtitle_beautify,
        subtitle_length_reference,
        target_lang,
    )?;

    patch_task_item(app, task_id, |task| {
        task.item.transcribe_status = "done".to_string();
        task.item.task_progress = done_task_progress_state();
        task.item.transcribe_error = String::new();
        task.item.result_text = source_text.clone();
        task.item.result_srt = step2_srt.clone();
        task.item.subtitle_segments_json = subtitle_segments_json.clone();
    })
}

pub(super) fn finish_translate_with_step5(
    app: &AppHandle,
    task_id: &str,
    media_path: &str,
    segments: &[BuildTranslationSegmentCommand],
    source_text: String,
    enable_subtitle_beautify: bool,
    subtitle_length_reference: u32,
    target_lang: &str,
) -> Result<(), String> {
    let workspace_segments = workspace_subtitle_segments_from_translation_segments(segments);
    let subtitle_segments_json = serialize_segments(&workspace_segments);
    write_completion_srts(
        task_id,
        media_path,
        &workspace_segments,
        true,
        enable_subtitle_beautify,
        subtitle_length_reference,
        target_lang,
    )?;

    patch_task_item(app, task_id, |task| {
        task.item.transcribe_status = "done".to_string();
        task.item.task_progress = done_task_progress_state();
        task.item.transcribe_error = String::new();
        task.item.result_text = source_text.clone();
        task.item.result_srt = String::new();
        task.item.subtitle_segments_json = subtitle_segments_json.clone();
    })
}

fn write_completion_srts(
    task_id: &str,
    media_path: &str,
    segments: &[WorkspaceSubtitleSegment],
    include_translation_variants: bool,
    enable_subtitle_beautify: bool,
    subtitle_length_reference: u32,
    target_lang: &str,
) -> Result<(), String> {
    let srt_segments = segments
        .iter()
        .map(
            |segment| crate::services::subtitle_srt::SubtitleSrtSegment {
                start_ms: segment.start_ms,
                end_ms: segment.end_ms,
                source_text: segment.source_text.clone(),
                translated_text: segment.translated_text.clone(),
            },
        )
        .collect::<Vec<_>>();
    crate::services::subtitle_srt::write_task_output_variants_for_completion(
        task_id,
        Path::new(media_path),
        srt_segments,
        include_translation_variants,
        crate::services::subtitle_srt::SubtitleBeautifyOptions {
            enabled: enable_subtitle_beautify,
            subtitle_length_reference,
            target_lang: target_lang.to_string(),
        },
    )?;
    Ok(())
}
