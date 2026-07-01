use std::path::Path;

use tauri::{AppHandle, Manager};

use crate::commands::translate_types::BuildTranslationSegmentCommand;
use crate::db::store::TaskStore;
use crate::domain::error::{WorkspaceError, WorkspaceResult};
use crate::domain::task::adapters::{
    workspace_subtitle_segments_from_step2_segments,
    workspace_subtitle_segments_from_translation_segments,
};
use crate::services::workspace_subtitle::{
    WorkspaceSubtitleSegment, serialize_segments,
};

use super::patch_task_item;
use super::progress::done_task_progress_state;

fn parse_segments(
    json: &str,
) -> Vec<crate::services::workspace_subtitle::WorkspaceSubtitleSegment> {
    serde_json::from_str(json).unwrap_or_default()
}

async fn persist_segments_to_db(
    app: &AppHandle,
    task_id: &str,
    subtitle_segments_json: &str,
) -> WorkspaceResult<()> {
    let store = app.state::<TaskStore>().inner().clone();
    let segments = parse_segments(subtitle_segments_json);
    store
        .replace_segments(task_id, &segments)
        .await
        .map_err(|e| WorkspaceError::TaskFailed(format!("persist segments {task_id}: {e}")))
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn finish_transcribe_only(
    app: &AppHandle,
    task_id: &str,
    media_path: &str,
    step2_segments: &[crate::commands::transcription::GroupedSentenceSegmentCommandDto],
    step2_srt: String,
    source_text: String,
    enable_subtitle_beautify: bool,
    subtitle_length_preset: &str,
    target_lang: &str,
) -> WorkspaceResult<()> {
    let mut workspace_segments = workspace_subtitle_segments_from_step2_segments(step2_segments);
    // Centralized beautify: SRT files, DB rows, and the in-app subtitle
    // editor all consume the same beautified segments. Doing it here
    // (instead of inside the SRT writer) keeps editor output and
    // exported SRTs consistent.
    if enable_subtitle_beautify {
        crate::services::subtitle_beautify::beautify_workspace_segments(
            &mut workspace_segments,
            subtitle_length_preset,
            target_lang,
        );
    }
    let subtitle_segments_json = serialize_segments(&workspace_segments);
    let store = app.state::<TaskStore>().inner();
    write_completion_srts(store, task_id, media_path, &workspace_segments, false)?;

    // Write segments to DB BEFORE marking the task done. If segments
    // fail, the task stays in its previous status and the next restart
    // will recover it as orphan-processing -> error rather than seeing
    // a "done" task with an empty segments table.
    persist_segments_to_db(app, task_id, &subtitle_segments_json).await?;
    patch_task_item(app, task_id, |task| {
        task.item.transcribe_status = "done".to_string();
        task.item.task_progress = done_task_progress_state();
        task.item.transcribe_error = String::new();
        task.item.result_text = source_text.clone();
        task.item.result_srt = step2_srt.clone();
        task.item.subtitle_segments_json = subtitle_segments_json.clone();
    })
    .await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn finish_translate_with_step5(
    app: &AppHandle,
    task_id: &str,
    media_path: &str,
    segments: &[BuildTranslationSegmentCommand],
    source_text: String,
    enable_subtitle_beautify: bool,
    subtitle_length_preset: &str,
    target_lang: &str,
) -> WorkspaceResult<()> {
    let mut workspace_segments = workspace_subtitle_segments_from_translation_segments(segments);
    // See finish_transcribe_only for the centralized beautify rationale.
    if enable_subtitle_beautify {
        crate::services::subtitle_beautify::beautify_workspace_segments(
            &mut workspace_segments,
            subtitle_length_preset,
            target_lang,
        );
    }
    let subtitle_segments_json = serialize_segments(&workspace_segments);
    let store = app.state::<TaskStore>().inner();
    write_completion_srts(store, task_id, media_path, &workspace_segments, true)?;

    // See `finish_transcribe_only` for the ordering rationale.
    persist_segments_to_db(app, task_id, &subtitle_segments_json).await?;
    patch_task_item(app, task_id, |task| {
        task.item.transcribe_status = "done".to_string();
        task.item.task_progress = done_task_progress_state();
        task.item.transcribe_error = String::new();
        task.item.result_text = source_text.clone();
        task.item.result_srt = String::new();
        task.item.subtitle_segments_json = subtitle_segments_json.clone();
    })
    .await?;
    Ok(())
}

fn write_completion_srts(
    store: &TaskStore,
    task_id: &str,
    media_path: &str,
    segments: &[WorkspaceSubtitleSegment],
    include_translation_variants: bool,
) -> Result<(), String> {
    // NB: `segments` is already beautified by the caller when
    // enable_subtitle_beautify is on (see finish_transcribe_only /
    // finish_translate_with_step5). Do not beautify again here -- doing
    // so would double-apply trim/merge and cost a redundant pass.
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
        srt_segments.clone(),
        include_translation_variants,
    )?;

    // Flat SRT output to output/ root directory
    if let Ok(settings) = crate::services::preferences::load_saved_settings_from_default_path(store) {
        if settings.flat_srt_output && !settings.flat_srt_items.is_empty() {
            let flat_items: Vec<crate::services::subtitle_srt::ExportSrtItem> = settings
                .flat_srt_items
                .iter()
                .filter_map(|s| crate::services::subtitle_srt::ExportSrtItem::parse(s.as_str()))
                .collect();
            if !flat_items.is_empty() {
                let output_dir = crate::services::output::resolve_output_dir();
                if output_dir.is_dir() {
                    let file_stem = Path::new(media_path)
                        .file_stem()
                        .map(|s| s.to_string_lossy().to_string())
                        .filter(|s| !s.is_empty())
                        .unwrap_or_else(|| task_id.to_string());
                    let safe_stem =
                        crate::services::task_path::sanitize_filename_component(&file_stem);
                    if let Err(e) = crate::services::subtitle_srt::write_flat_variants_to_directory(
                        &output_dir,
                        &safe_stem,
                        &srt_segments,
                        &flat_items,
                    ) {
                        eprintln!("[warn] flat SRT output failed: {e}");
                    }
                }
            }
        }
    }

    Ok(())
}
