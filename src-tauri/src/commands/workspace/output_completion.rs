use std::path::Path;

use tauri::{AppHandle, Manager};

use crate::commands::translate_types::BuildTranslationSegmentCommand;
use crate::db::store::TaskStore;
use crate::domain::error::{WorkspaceError, WorkspaceResult};
use crate::domain::task::adapters::{
    workspace_subtitle_segments_from_step2_segments,
    workspace_subtitle_segments_from_translation_segments,
};
use crate::services::preferences::load_saved_settings_from_default_path;
use crate::services::subtitle_render::{BurnHardSubtitleRequest, burn_hard_subtitle};
use crate::services::task_log::TaskLogger;
use crate::services::workspace_subtitle::{
    WorkspaceSubtitleSegment, serialize_segments,
};

use super::patch_task_item;
use super::progress::{done_task_progress_state, report_task_stage};
use super::TaskStage;

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
    media_kind: &str,
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
    write_completion_srts(
        store,
        task_id,
        media_path,
        media_kind,
        &workspace_segments,
        false,
    )?;

    // Write segments to DB BEFORE marking the task done. If segments
    // fail, the task stays in its previous status and the next restart
    // will recover it as orphan-processing -> error rather than seeing
    // a "done" task with an empty segments table.
    persist_segments_to_db(app, task_id, &subtitle_segments_json).await?;

    // Auto burn-in is a live setting (user-toggleable), gated to video
    // tasks. It runs after the segments are persisted but before marking
    // done so the "burning" stage is visible. Failures never fail the task
    // — subtitles/SRT are already complete; burning is an add-on.
    maybe_burn_hard_subtitle(app, task_id, media_path, media_kind, &workspace_segments).await;

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
    media_kind: &str,
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
    write_completion_srts(
        store,
        task_id,
        media_path,
        media_kind,
        &workspace_segments,
        true,
    )?;

    // See `finish_transcribe_only` for the ordering rationale.
    persist_segments_to_db(app, task_id, &subtitle_segments_json).await?;

    // See finish_transcribe_only for the burn rationale and ordering.
    maybe_burn_hard_subtitle(app, task_id, media_path, media_kind, &workspace_segments).await;

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

/// Auto burn-in hard subtitles into the source video when the user has enabled
/// `auto_burn_hard_subtitle` and the task is a video. Audio tasks are skipped
/// silently (the setting is documented as "video only"). This is best-effort:
/// any error is logged but never propagated, so a failed burn cannot fail a
/// task whose subtitles are already complete.
async fn maybe_burn_hard_subtitle(
    app: &AppHandle,
    task_id: &str,
    media_path: &str,
    media_kind: &str,
    segments: &[WorkspaceSubtitleSegment],
) {
    if media_kind.trim() != "video" {
        return;
    }

    let store = app.state::<TaskStore>().inner();
    let saved = match load_saved_settings_from_default_path(store) {
        Ok(settings) => settings,
        Err(_) => return,
    };
    if !saved.auto_burn_hard_subtitle {
        return;
    }

    // Surface the burn as its own stage so the UI shows progress instead of
    // an apparent hang while ffmpeg re-encodes the whole video.
    let _ = report_task_stage(app, task_id, TaskStage::Burning, "", 0, 1).await;

    let request = BurnHardSubtitleRequest {
        task_id: task_id.to_string(),
        media_path: media_path.to_string(),
        subtitle_segments: segments.to_vec(),
        burn_mode: saved.subtitle_burn_mode,
        style: saved.subtitle_render_style,
    };
    let logger = TaskLogger::main_with_media(task_id.to_string(), media_path.to_string());
    let task_id_owned = task_id.to_string();
    let join = tauri::async_runtime::spawn_blocking(move || burn_hard_subtitle(request)).await;

    match join {
        Ok(Ok(response)) => {
            logger.event(
                "subtitle.burn.completed",
                Some(&serde_json::json!({ "taskId": task_id_owned, "outputPath": response.output_path })),
            );
        }
        Ok(Err(err)) => {
            logger.event(
                "subtitle.burn.failed",
                Some(&serde_json::json!({ "taskId": task_id_owned, "error": err })),
            );
        }
        Err(err) => {
            logger.event(
                "subtitle.burn.failed",
                Some(&serde_json::json!({ "taskId": task_id_owned, "error": format!("burn task join error: {err}") })),
            );
        }
    }
}

fn write_completion_srts(
    store: &TaskStore,
    task_id: &str,
    media_path: &str,
    media_kind: &str,
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
    // SRT-import tasks already keep the original file in the task folder;
    // do not emit a redundant src.srt on completion.
    let is_subtitle_task = media_kind.trim() == "subtitle"
        || crate::services::subtitle_import::is_srt_path(media_path);
    let include_source = !is_subtitle_task;
    crate::services::subtitle_srt::write_task_output_variants_for_completion_with_options(
        task_id,
        Path::new(media_path),
        srt_segments.clone(),
        include_translation_variants,
        include_source,
    )?;

    // Flat SRT output to output/ root directory
    if let Ok(settings) = crate::services::preferences::load_saved_settings_from_default_path(store) {
        if settings.flat_srt_output && !settings.flat_srt_items.is_empty() {
            let flat_items: Vec<crate::services::subtitle_srt::ExportSrtItem> = settings
                .flat_srt_items
                .iter()
                .filter_map(|s| crate::services::subtitle_srt::ExportSrtItem::parse(s.as_str()))
                // SRT tasks: skip flat "source" — original is already the imported file.
                .filter(|item| include_source || *item != crate::services::subtitle_srt::ExportSrtItem::Source)
                .collect();
            if !flat_items.is_empty() {
                let output_dir = crate::services::output::resolve_output_dir();
                if output_dir.is_dir() {
                    let file_stem = resolve_flat_output_stem(media_path, task_id);
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

/// Prefer the original media/subtitle stem for flat filenames.
fn resolve_flat_output_stem(media_path: &str, task_id: &str) -> String {
    let path = Path::new(media_path);
    if let Some(stem) = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .filter(|s| !s.is_empty())
    {
        return stem;
    }
    if let Some(parent_name) = path
        .parent()
        .and_then(|p| p.file_name())
        .map(|s| s.to_string_lossy().to_string())
    {
        let suffix = format!("_{task_id}");
        if let Some(stripped) = parent_name.strip_suffix(&suffix) {
            if !stripped.is_empty() {
                return stripped.to_string();
            }
        }
        if !parent_name.is_empty() {
            return parent_name;
        }
    }
    task_id.to_string()
}
