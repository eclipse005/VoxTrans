use serde_json::{Value, json};
use sqlx::SqlitePool;
use tauri::async_runtime::JoinHandle;
use tauri::async_runtime::spawn_blocking;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use voxtrans_core::subtitle::beautify::beautify_words_for_subtitle;

use crate::services::preferences::SavedSettings;
use crate::services::task_context::{
    STAGE_ASR, STAGE_PUNCTUATE, STAGE_SEGMENT, STAGE_SEGMENT_OPTIMIZE, STAGE_SEPARATE,
    STAGE_SUMMARIZE, STAGE_TRANSLATE, TaskContext,
};
use crate::services::task_projection::{TaskProjectionEditorState, TaskProjectionState};
use crate::services::task_stage_handlers::StageHandlers;
use crate::services::task_subtitle_composer::{
    WordTimingAnchor, build_bilingual_srt_from_translate_segments,
    build_srt_from_translate_segments, realign_segments_with_words,
};
use crate::services::translate::segment_optimize::{SegmentOptimizeRequest, run_segment_optimize};
use crate::services::translate::types::TranslatePipelineRequest;
use crate::services::translate::{run_translate_summarize, run_translate_with_theme};

use super::events::emit_task_state_changed;
use super::log_pipeline_stage;
use super::runtime::{TaskRunExecRow, persist_task_context_boxed};
use super::state::{
    SegmentOptimizeSnapshot, SegmentResumeSnapshot, SummarizeSnapshot, TranslateSnapshot,
    from_core_words, load_asr_resume_snapshot, load_segment_optimize_snapshot,
    load_segment_snapshot, load_stage_words, load_summarize_snapshot, load_translate_snapshot,
    to_core_words,
};
use crate::services::transcribe::{
    BuildSegmentsRequest, TranscribeRequest, TranscribeResponse, WordTokenDto,
    build_segments_from_words, transcribe_blocking,
};
use crate::services::transcription::{PunctuationConfig, optimize_words_with_llm};

pub(super) async fn run_asr_stage(
    pool: &SqlitePool,
    app: Option<&tauri::AppHandle>,
    task: &TaskRunExecRow,
    context: &mut TaskContext,
    projection: &TaskProjectionState,
    transcribe_audio_path: String,
    settings: &SavedSettings,
) -> Result<TranscribeResponse, String> {
    let editor_snapshot = projection.editor.clone();
    StageHandlers::new(
        pool,
        &task.id,
        context,
        projection,
        persist_task_context_boxed,
    )
    .run(
        STAGE_ASR,
        |ctx| {
            load_asr_resume_snapshot(ctx).map(|snapshot| TranscribeResponse {
                words: snapshot.words,
                segment_total: snapshot.segment_total,
                segment_durations_sec: Vec::new(),
                audio_duration_sec: snapshot.audio_duration_sec,
                vad_elapsed_sec: snapshot.vad_elapsed_sec,
                transcribe_elapsed_sec: snapshot.transcribe_elapsed_sec,
                execution_provider: snapshot.execution_provider,
            })
        },
        |value| !value.words.is_empty(),
        || {
            let task_id = task.id.clone();
            let task_snapshot = task.clone();
            let app_opt = app.cloned();
            let pool = pool.clone();
            let settings = settings.clone();
            let transcribe_audio_path = transcribe_audio_path.clone();
            let editor_snapshot = editor_snapshot.clone();
            async move {
                let transcribe_req = TranscribeRequest {
                    task_id: task_id.clone(),
                    audio_path: transcribe_audio_path,
                    provider: settings.provider.clone(),
                    chunk_target_seconds: settings.chunk_target_seconds,
                    model_dir: None,
                };
                let progress = StageProgressReporter::new(
                    pool.clone(),
                    app_opt.clone(),
                    task_snapshot.clone(),
                    editor_snapshot.clone(),
                );
                let progress_tx = progress.sender();
                let transcribed = spawn_blocking(move || {
                    transcribe_blocking(transcribe_req, |current, total| {
                        let display_current = current.max(1);
                        let display_total = total.max(display_current);
                        let progress_percent =
                            percent_from_position(display_current as u32, display_total as u32);
                        progress_tx.publish(StageProgressSnapshot {
                            current_stage: STAGE_ASR,
                            queue_phase: "recognizing",
                            phase_detail: format!("{display_current}/{display_total}"),
                            progress_percent,
                            current: display_current as u32,
                            total: display_total as u32,
                        });
                    })
                })
                .await;
                progress.finish().await?;
                let transcribed = transcribed.map_err(|err| err.to_string())??;
                Ok(transcribed)
            }
        },
        |value| {
            json!({
                "segmentTotal": value.segment_total,
                "audioDurationSec": value.audio_duration_sec,
                "provider": value.execution_provider,
                "words": value.words,
            })
        },
        |value| {
            json!({
                "transcribeElapsedSec": value.transcribe_elapsed_sec,
                "vadElapsedSec": value.vad_elapsed_sec,
            })
        },
    )
    .await
}

pub(super) async fn run_separate_stage(
    pool: &SqlitePool,
    app: Option<&tauri::AppHandle>,
    task: &TaskRunExecRow,
    context: &mut TaskContext,
    projection: &TaskProjectionState,
    settings: &SavedSettings,
) -> Result<String, String> {
    let editor_snapshot = projection.editor.clone();
    StageHandlers::new(
        pool,
        &task.id,
        context,
        projection,
        persist_task_context_boxed,
    )
    .run(
        STAGE_SEPARATE,
        |ctx| {
            ctx.stages
                .separate
                .output
                .get("vocalsPath")
                .and_then(Value::as_str)
                .map(|v| v.to_string())
        },
        |value| !value.trim().is_empty(),
        || {
            let task_id = task.id.clone();
            let task_snapshot = task.clone();
            let media_path = task.media_path.clone();
            let app_opt = app.cloned();
            let pool = pool.clone();
            let settings = settings.clone();
            let editor_snapshot = editor_snapshot.clone();
            async move {
                if !settings.enable_vocal_separation {
                    return spawn_blocking(move || {
                        crate::services::demucs::prepare_audio_for_asr(&task_id, &media_path)
                    })
                    .await
                    .map_err(|err| err.to_string())?;
                }
                publish_processing_snapshot(
                    &pool,
                    app_opt.as_ref(),
                    &task_snapshot,
                    STAGE_SEPARATE,
                    "separating",
                    "",
                    0,
                    0,
                    0,
                    &editor_snapshot,
                )
                .await?;
                let progress = StageProgressReporter::new(
                    pool.clone(),
                    app_opt.clone(),
                    task_snapshot.clone(),
                    editor_snapshot.clone(),
                );
                let progress_tx = progress.sender();
                let req = crate::services::demucs::SeparateVocalsRequest {
                    task_id: task_id.clone(),
                    audio_path: media_path.clone(),
                    model: settings.demucs_model.clone(),
                };
                let separated = spawn_blocking(move || {
                    crate::services::demucs::separate_vocals_blocking(req, |percent| {
                        progress_tx.publish(StageProgressSnapshot {
                            current_stage: STAGE_SEPARATE,
                            queue_phase: "separating",
                            phase_detail: format!("{percent}%"),
                            progress_percent: percent,
                            current: percent,
                            total: 100,
                        });
                    })
                })
                .await;
                progress.finish().await?;
                let separated = separated.map_err(|err| err.to_string())??;
                Ok(separated.vocals_path)
            }
        },
        |value| json!({ "vocalsPath": value }),
        |_| Value::Null,
    )
    .await
}

pub(super) async fn run_punctuate_stage(
    pool: &SqlitePool,
    app: Option<&tauri::AppHandle>,
    task: &TaskRunExecRow,
    context: &mut TaskContext,
    projection: &TaskProjectionState,
    words: &[WordTokenDto],
    settings: &SavedSettings,
) -> Result<Vec<WordTokenDto>, String> {
    if !settings.enable_punctuation_optimization {
        return Ok(words.to_vec());
    }
    let editor_snapshot = projection.editor.clone();
    StageHandlers::new(
        pool,
        &task.id,
        context,
        projection,
        persist_task_context_boxed,
    )
    .run(
        STAGE_PUNCTUATE,
        |ctx| load_stage_words(ctx, STAGE_PUNCTUATE),
        |value| !value.is_empty(),
        || {
            let words_for_exec = words.to_vec();
            let media_path = task.media_path.clone();
            let task_id = task.id.clone();
            let settings = settings.clone();
            let editor_snapshot = editor_snapshot.clone();
            async move {
                publish_processing_snapshot(
                    pool,
                    app,
                    task,
                    STAGE_PUNCTUATE,
                    "punctuate",
                    "",
                    99,
                    0,
                    0,
                    &editor_snapshot,
                )
                .await?;
                let optimized_words = optimize_words_with_llm(
                    &task_id,
                    &media_path,
                    beautify_words_for_subtitle(to_core_words(words_for_exec)),
                    &PunctuationConfig {
                        enabled: settings.enable_punctuation_optimization,
                        base_url: settings.translate_base_url.clone(),
                        api_key: settings.translate_api_key.clone(),
                        model: settings.translate_model.clone(),
                        llm_concurrency: settings.llm_concurrency,
                    },
                )
                .await?;
                Ok(from_core_words(optimized_words))
            }
        },
        |value| json!({ "wordTotal": value.len(), "words": value }),
        |_| Value::Null,
    )
    .await
}

pub(super) async fn run_segment_stage(
    pool: &SqlitePool,
    app: Option<&tauri::AppHandle>,
    task: &TaskRunExecRow,
    context: &mut TaskContext,
    projection: &TaskProjectionState,
    words: &[WordTokenDto],
    subtitle_max_words_per_segment: u32,
    with_translate: bool,
) -> Result<SegmentResumeSnapshot, String> {
    let editor_snapshot = projection.editor.clone();
    StageHandlers::new(
        pool,
        &task.id,
        context,
        projection,
        persist_task_context_boxed,
    )
    .run(
        STAGE_SEGMENT,
        load_segment_snapshot,
        |value| !value.segments.is_empty() && !value.srt.trim().is_empty(),
        || {
            let task_id = task.id.clone();
            let media_path = task.media_path.clone();
            let words_for_exec = words.to_vec();
            let editor_snapshot = editor_snapshot.clone();
            async move {
                publish_processing_snapshot(
                    pool,
                    app,
                    task,
                    STAGE_SEGMENT,
                    "segment",
                    "",
                    99,
                    0,
                    0,
                    &editor_snapshot,
                )
                .await?;
                let built = build_segments_from_words(BuildSegmentsRequest {
                    task_id,
                    audio_path: media_path,
                    words: words_for_exec,
                    subtitle_max_words_per_segment,
                    segment_mode: if with_translate {
                        "translate_source".to_string()
                    } else {
                        "transcribe".to_string()
                    },
                })?;
                Ok(SegmentResumeSnapshot {
                    text: built.text,
                    srt: built.srt,
                    srt_output_path: built.srt_output_path,
                    segments: built.segments,
                })
            }
        },
        |value| {
            json!({
                "segmentTotal": value.segments.len(),
                "sourceSrtPath": value.srt_output_path,
                "text": value.text,
                "srt": value.srt,
                "srtOutputPath": value.srt_output_path,
                "segments": value.segments,
            })
        },
        |_| Value::Null,
    )
    .await
}

pub(super) async fn run_summarize_stage(
    pool: &SqlitePool,
    app: Option<&tauri::AppHandle>,
    task: &TaskRunExecRow,
    context: &mut TaskContext,
    projection: &TaskProjectionState,
    translate_request: &TranslatePipelineRequest,
) -> Result<SummarizeSnapshot, String> {
    log_pipeline_stage(task, "summarize", "started", Value::Null);
    let editor_snapshot = projection.editor.clone();
    let summarize_snapshot = StageHandlers::new(
        pool,
        &task.id,
        context,
        projection,
        persist_task_context_boxed,
    )
    .run(
        STAGE_SUMMARIZE,
        load_summarize_snapshot,
        |value| !value.theme.trim().is_empty(),
        || {
            let request = translate_request.clone();
            let task_snapshot = task.clone();
            let app_handle = app.cloned();
            let pool = pool.clone();
            let editor_snapshot = editor_snapshot.clone();
            async move {
                publish_processing_snapshot(
                    &pool,
                    app_handle.as_ref(),
                    &task_snapshot,
                    STAGE_SUMMARIZE,
                    "summarize",
                    "",
                    99,
                    0,
                    0,
                    &editor_snapshot,
                )
                .await?;
                let (theme, terminology_entries, primary_total, supporting_total) =
                    run_translate_summarize(&request).await?;
                Ok(SummarizeSnapshot {
                    theme,
                    terminology_entries,
                    terminology_primary_total: primary_total,
                    terminology_supporting_total: supporting_total,
                })
            }
        },
        |value| {
            json!({
                "theme": value.theme,
                "terminologyEntries": value.terminology_entries,
                "terminologyPrimaryTotal": value.terminology_primary_total,
                "terminologySupportingTotal": value.terminology_supporting_total,
            })
        },
        |_| Value::Null,
    )
    .await?;
    log_pipeline_stage(
        task,
        "summarize",
        "completed",
        json!({
            "theme": summarize_snapshot.theme,
            "terminologyInputTotal": translate_request.terminology_entries.len(),
            "terminologyPrimaryTotal": summarize_snapshot.terminology_primary_total,
            "terminologySupportingTotal": summarize_snapshot.terminology_supporting_total,
            "terminologyOutputTotal": summarize_snapshot.terminology_entries.len(),
        }),
    );
    Ok(summarize_snapshot)
}

pub(super) async fn run_translate_stage(
    pool: &SqlitePool,
    app: Option<&tauri::AppHandle>,
    task: &TaskRunExecRow,
    context: &mut TaskContext,
    projection: &TaskProjectionState,
    translate_request: &TranslatePipelineRequest,
    summarize_snapshot: &SummarizeSnapshot,
) -> Result<TranslateSnapshot, String> {
    log_pipeline_stage(task, "translate", "started", Value::Null);
    let editor_snapshot = projection.editor.clone();
    let translate_snapshot = StageHandlers::new(
        pool,
        &task.id,
        context,
        projection,
        persist_task_context_boxed,
    )
    .run(
        STAGE_TRANSLATE,
        load_translate_snapshot,
        |value| !value.segments.is_empty(),
        || {
            let request = translate_request.clone();
            let summarize = summarize_snapshot.clone();
            let task_snapshot = task.clone();
            let phase_app = app.cloned();
            let pool = pool.clone();
            let editor_snapshot = editor_snapshot.clone();
            async move {
                let progress = StageProgressReporter::new(
                    pool.clone(),
                    phase_app.clone(),
                    task_snapshot.clone(),
                    editor_snapshot.clone(),
                );
                progress.publish(StageProgressSnapshot {
                    current_stage: STAGE_TRANSLATE,
                    queue_phase: "translate",
                    phase_detail: String::new(),
                    progress_percent: 99,
                    current: 0,
                    total: 0,
                });
                let progress_tx = progress.sender();
                let mut on_progress = move |current_batch: usize, total_batches: usize| {
                    let display_current = current_batch.max(1);
                    let display_total = total_batches.max(display_current);
                    progress_tx.publish(StageProgressSnapshot {
                        current_stage: STAGE_TRANSLATE,
                        queue_phase: "translate",
                        phase_detail: format!("{display_current}/{display_total}"),
                        progress_percent: percent_from_position(
                            display_current as u32,
                            display_total as u32,
                        ),
                        current: display_current as u32,
                        total: display_total as u32,
                    });
                };
                let translated = run_translate_with_theme(
                    request,
                    summarize.theme,
                    summarize.terminology_entries,
                    &mut on_progress,
                )
                .await;
                drop(on_progress);
                progress.finish().await?;
                let translated = translated?;
                Ok(TranslateSnapshot {
                    source_srt: translated.source_srt,
                    target_srt: translated.target_srt,
                    bilingual_srt_source_first: translated.bilingual_srt_source_first,
                    bilingual_srt_target_first: translated.bilingual_srt_target_first,
                    segments: translated.segments,
                })
            }
        },
        |value| {
            json!({
                "translatedSegmentTotal": value.segments.len(),
                "batchSize": 20,
                "sourceSrt": value.source_srt,
                "targetSrt": value.target_srt,
                "bilingualSrtSourceFirst": value.bilingual_srt_source_first,
                "bilingualSrtTargetFirst": value.bilingual_srt_target_first,
                "segments": value.segments,
            })
        },
        |_| Value::Null,
    )
    .await?;
    log_pipeline_stage(
        task,
        "translate",
        "completed",
        json!({
            "translatedSegmentTotal": translate_snapshot.segments.len(),
        }),
    );
    Ok(translate_snapshot)
}

pub(super) async fn run_segment_optimize_stage(
    pool: &SqlitePool,
    app: Option<&tauri::AppHandle>,
    task: &TaskRunExecRow,
    context: &mut TaskContext,
    projection: &TaskProjectionState,
    settings: &SavedSettings,
    segments: Vec<crate::services::translate::types::TranslateSegment>,
    word_timestamps: &[WordTimingAnchor],
) -> Result<SegmentOptimizeSnapshot, String> {
    log_pipeline_stage(task, "segment_optimize", "started", Value::Null);
    let editor_snapshot = projection.editor.clone();
    let mut snapshot = StageHandlers::new(
        pool,
        &task.id,
        context,
        projection,
        persist_task_context_boxed,
    )
    .run(
        STAGE_SEGMENT_OPTIMIZE,
        load_segment_optimize_snapshot,
        |value| !value.segments.is_empty(),
        || {
            let task_id = task.id.clone();
            let task_snapshot = task.clone();
            let media_path = task.media_path.clone();
            let settings = settings.clone();
            let input_segments = segments.clone();
            let app_handle = app.cloned();
            let pool = pool.clone();
            let editor_snapshot = editor_snapshot.clone();
            async move {
                publish_processing_snapshot(
                    &pool,
                    app_handle.as_ref(),
                    &task_snapshot,
                    STAGE_SEGMENT_OPTIMIZE,
                    "segment_optimize",
                    "",
                    99,
                    0,
                    0,
                    &editor_snapshot,
                )
                .await?;
                let segment_optimize_result = run_segment_optimize(SegmentOptimizeRequest {
                    task_id: task_id.clone(),
                    media_path: media_path.clone(),
                    source_lang: task.source_lang.clone(),
                    target_lang: task.target_lang.clone(),
                    translate_api_key: settings.translate_api_key.clone(),
                    translate_base_url: settings.translate_base_url.clone(),
                    translate_model: settings.translate_model.clone(),
                    llm_concurrency: settings.llm_concurrency,
                    source_max_words_per_segment: settings.subtitle_max_words_per_segment,
                    target_reference_len: settings.subtitle_length_reference,
                    segments: input_segments,
                })
                .await
                .map_err(|err| format!("segment optimize failed: {err}"))?;
                Ok(SegmentOptimizeSnapshot {
                    segments: segment_optimize_result.segments,
                    report: segment_optimize_result.report,
                    applied_change_total: segment_optimize_result.applied_changes.len(),
                    source_srt: segment_optimize_result.source_srt,
                    target_srt: segment_optimize_result.target_srt,
                    src_trans_srt: segment_optimize_result.bilingual_srt_source_first,
                    trans_src_srt: segment_optimize_result.bilingual_srt_target_first,
                })
            }
        },
        |value| {
            json!({
                "appliedChangeTotal": value.applied_change_total,
                "report": value.report,
                "segments": value.segments,
                "sourceSrt": value.source_srt,
                "targetSrt": value.target_srt,
                "srcTransSrt": value.src_trans_srt,
                "transSrcSrt": value.trans_src_srt,
            })
        },
        |_| Value::Null,
    )
    .await?;
    snapshot = finalize_segment_optimize_timing(snapshot, word_timestamps);
    log_pipeline_stage(
        task,
        "segment_optimize",
        "completed",
        json!({
            "appliedChangeTotal": snapshot.applied_change_total,
            "segmentTotal": snapshot.segments.len(),
        }),
    );
    Ok(snapshot)
}

#[derive(Debug)]
struct StageProgressSnapshot {
    current_stage: &'static str,
    queue_phase: &'static str,
    phase_detail: String,
    progress_percent: u32,
    current: u32,
    total: u32,
}

#[derive(Debug)]
enum StageProgressMessage {
    Snapshot(StageProgressSnapshot),
    Close,
}

#[derive(Clone)]
struct StageProgressSender {
    tx: UnboundedSender<StageProgressMessage>,
}

impl StageProgressSender {
    fn publish(&self, snapshot: StageProgressSnapshot) {
        let _ = self.tx.send(StageProgressMessage::Snapshot(snapshot));
    }
}

struct StageProgressReporter {
    tx: UnboundedSender<StageProgressMessage>,
    drain_task: JoinHandle<Result<(), String>>,
}

impl StageProgressReporter {
    fn new(
        pool: SqlitePool,
        app: Option<tauri::AppHandle>,
        task: TaskRunExecRow,
        editor: TaskProjectionEditorState,
    ) -> Self {
        let (tx, rx) = unbounded_channel();
        let drain_task = tauri::async_runtime::spawn(async move {
            drain_progress_snapshots(pool, app, task, editor, rx).await
        });
        Self { tx, drain_task }
    }

    fn sender(&self) -> StageProgressSender {
        StageProgressSender {
            tx: self.tx.clone(),
        }
    }

    fn publish(&self, snapshot: StageProgressSnapshot) {
        self.sender().publish(snapshot);
    }

    async fn finish(self) -> Result<(), String> {
        let _ = self.tx.send(StageProgressMessage::Close);
        self.drain_task.await.map_err(|err| err.to_string())?
    }
}

async fn publish_processing_snapshot(
    pool: &SqlitePool,
    app: Option<&tauri::AppHandle>,
    task: &TaskRunExecRow,
    current_stage: &str,
    queue_phase: &str,
    phase_detail: &str,
    progress_percent: u32,
    current: u32,
    total: u32,
    editor: &TaskProjectionEditorState,
) -> Result<(), String> {
    let now = unix_now();
    let persisted_progress = progress_percent.clamp(0, 100);
    let result = sqlx::query(
        "UPDATE task_runs
         SET overall_status = 'running',
             current_stage = ?,
             progress_percent = ?,
             phase_detail = ?,
             segment_current = ?,
             segment_total = ?,
             error_message = '',
             started_at = COALESCE(started_at, ?),
             updated_at = ?
         WHERE id = ?
           AND overall_status NOT IN ('failed', 'completed')",
    )
    .bind(current_stage)
    .bind(persisted_progress as i64)
    .bind(phase_detail)
    .bind(current as i64)
    .bind(total as i64)
    .bind(now)
    .bind(now)
    .bind(&task.id)
    .execute(pool)
    .await
    .map_err(|err| err.to_string())?;
    if result.rows_affected() == 0 {
        return Ok(());
    }
    emit_task_state_changed(
        app,
        &build_progress_state_changed_event(
            task,
            queue_phase,
            phase_detail,
            persisted_progress,
            current,
            total,
            editor,
        ),
    );
    Ok(())
}

/// Minimum interval between progress updates to avoid DB write flooding
const MIN_PROGRESS_INTERVAL_MS: u64 = 500;

async fn drain_progress_snapshots(
    pool: SqlitePool,
    app: Option<tauri::AppHandle>,
    task: TaskRunExecRow,
    editor: TaskProjectionEditorState,
    mut rx: UnboundedReceiver<StageProgressMessage>,
) -> Result<(), String> {
    let mut last_publish_ms: u64 = 0;
    let mut pending_snapshot: Option<StageProgressSnapshot> = None;

    while let Some(message) = rx.recv().await {
        match message {
            StageProgressMessage::Snapshot(snapshot) => {
                // Throttle: only publish at most once per MIN_PROGRESS_INTERVAL_MS
                let now_ms = (unix_now() as u64).saturating_mul(1000); // convert to milliseconds
                let elapsed = now_ms.saturating_sub(last_publish_ms);

                if elapsed >= MIN_PROGRESS_INTERVAL_MS {
                    // Enough time has passed, publish immediately
                    last_publish_ms = now_ms;
                    publish_processing_snapshot(
                        &pool,
                        app.as_ref(),
                        &task,
                        snapshot.current_stage,
                        snapshot.queue_phase,
                        &snapshot.phase_detail,
                        snapshot.progress_percent,
                        snapshot.current,
                        snapshot.total,
                        &editor,
                    )
                    .await?;
                } else {
                    // Too soon, buffer the latest snapshot
                    pending_snapshot = Some(snapshot);
                }
            }
            StageProgressMessage::Close => {
                // On close, publish any pending snapshot
                if let Some(snapshot) = pending_snapshot.take() {
                    publish_processing_snapshot(
                        &pool,
                        app.as_ref(),
                        &task,
                        snapshot.current_stage,
                        snapshot.queue_phase,
                        &snapshot.phase_detail,
                        snapshot.progress_percent,
                        snapshot.current,
                        snapshot.total,
                        &editor,
                    )
                    .await?;
                }
                break;
            }
        }
    }
    Ok(())
}

fn build_progress_state_changed_event(
    task: &TaskRunExecRow,
    queue_phase: &str,
    phase_detail: &str,
    progress_percent: u32,
    current: u32,
    total: u32,
    editor: &TaskProjectionEditorState,
) -> super::events::TaskStateChangedEvent {
    super::events::TaskStateChangedEvent {
        id: task.id.clone(),
        path: task.media_path.clone(),
        name: task.name.clone(),
        media_kind: task.media_kind.clone(),
        size_bytes: task.size_bytes.max(0) as u64,
        transcribe_status: "processing".to_string(),
        transcribe_progress: progress_percent,
        transcribe_segment_current: current,
        transcribe_segment_total: total,
        transcribe_phase: queue_phase.to_string(),
        transcribe_phase_detail: phase_detail.to_string(),
        transcribe_error: String::new(),
        result_text: editor.result_text.clone(),
        result_srt: editor.result_srt.clone(),
        subtitle_segments_json: editor.subtitle_segments_json.clone(),
    }
}

fn percent_from_position(current: u32, total: u32) -> u32 {
    if total == 0 {
        return 0;
    }
    ((current.min(total) * 100) / total).clamp(0, 100)
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

fn finalize_segment_optimize_timing(
    mut snapshot: SegmentOptimizeSnapshot,
    word_timestamps: &[WordTimingAnchor],
) -> SegmentOptimizeSnapshot {
    let align_result = realign_segments_with_words(&mut snapshot.segments, word_timestamps);
    snapshot.source_srt = build_srt_from_translate_segments(&snapshot.segments, false);
    snapshot.target_srt = build_srt_from_translate_segments(&snapshot.segments, true);
    snapshot.src_trans_srt = build_bilingual_srt_from_translate_segments(&snapshot.segments, true);
    snapshot.trans_src_srt = build_bilingual_srt_from_translate_segments(&snapshot.segments, false);
    if let Some(report) = snapshot.report.as_object_mut() {
        report.insert("timingFinalized".to_string(), Value::Bool(true));
        report.insert("timingAlignResult".to_string(), align_result);
    }
    snapshot
}
