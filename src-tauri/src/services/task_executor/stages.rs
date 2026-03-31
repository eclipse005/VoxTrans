use serde_json::{Value, json};
use sqlx::SqlitePool;
use tauri::async_runtime::spawn_blocking;
use voxtrans_core::subtitle::beautify::beautify_words_for_subtitle;

use crate::services::preferences::SavedSettings;
use crate::services::task_context::{
    STAGE_ASR, STAGE_PUNCTUATE, STAGE_SEGMENT, STAGE_SEGMENT_OPTIMIZE, STAGE_SEPARATE, STAGE_SUMMARIZE,
    STAGE_TRANSLATE, TaskContext,
};
use crate::services::task_projection::TaskProjectionState;
use crate::services::task_stage_handlers::StageHandlers;
use crate::services::task_subtitle_composer::{
    WordTimingAnchor, build_bilingual_srt_from_translate_segments, build_srt_from_translate_segments,
    realign_segments_with_words,
};
use crate::services::translate::segment_optimize::{SegmentOptimizeRequest, run_segment_optimize};
use crate::services::translate::types::TranslatePipelineRequest;
use crate::services::translate::{run_translate_summarize, run_translate_with_theme};

use super::state::{
    SegmentOptimizeSnapshot, SegmentResumeSnapshot, SummarizeSnapshot, TranslateSnapshot,
    from_core_words, load_asr_resume_snapshot, load_segment_optimize_snapshot, load_segment_snapshot,
    load_stage_words, load_summarize_snapshot, load_translate_snapshot, to_core_words,
};
use super::events::{
    SeparateProgressEvent, TranscribePhaseEvent, TranscribeProgressEvent, TranslateProgressEvent,
    emit_bridge_event,
};
use super::{
    log_pipeline_stage,
};
use super::runtime::{TaskRunExecRow, persist_task_context_boxed};
use crate::services::transcribe::{
    BuildSegmentsRequest, TranscribeRequest, TranscribeResponse, WordTokenDto, build_segments_from_words,
    transcribe_blocking,
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
            let app_opt = app.cloned();
            let settings = settings.clone();
            let transcribe_audio_path = transcribe_audio_path.clone();
            async move {
                let app_handle = app_opt.clone();
                let progress_task_id = task_id.clone();
                emit_bridge_event(
                    app_opt.as_ref(),
                    "transcribe-phase",
                    &TranscribePhaseEvent {
                        task_id: task_id.clone(),
                        phase: "recognizing".to_string(),
                        phase_detail: None,
                    },
                );
                let transcribe_req = TranscribeRequest {
                    task_id: task_id.clone(),
                    audio_path: transcribe_audio_path,
                    provider: settings.provider.clone(),
                    chunk_target_seconds: settings.chunk_target_seconds,
                    model_dir: None,
                };
                let transcribed = spawn_blocking(move || {
                    transcribe_blocking(transcribe_req, |current, total| {
                        emit_bridge_event(
                            app_handle.as_ref(),
                            "transcribe-progress",
                            &TranscribeProgressEvent {
                                task_id: progress_task_id.clone(),
                                current_segment: current,
                                total_segments: total,
                            },
                        );
                    })
                })
                .await
                .map_err(|err| err.to_string())??;
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
            let media_path = task.media_path.clone();
            let app_opt = app.cloned();
            let settings = settings.clone();
            async move {
                if !settings.enable_vocal_separation {
                    return crate::services::demucs::prepare_audio_for_asr(&task_id, &media_path);
                }
                emit_bridge_event(
                    app_opt.as_ref(),
                    "transcribe-phase",
                    &TranscribePhaseEvent {
                        task_id: task_id.clone(),
                        phase: "separating".to_string(),
                        phase_detail: None,
                    },
                );

                let app_handle = app_opt.clone();
                let req = crate::services::demucs::SeparateVocalsRequest {
                    task_id: task_id.clone(),
                    audio_path: media_path.clone(),
                    model: settings.demucs_model.clone(),
                };
                let progress_task_id = task_id.clone();
                let separated = spawn_blocking(move || {
                    crate::services::demucs::separate_vocals_blocking(req, |percent| {
                        emit_bridge_event(
                            app_handle.as_ref(),
                            "separate-progress",
                            &SeparateProgressEvent {
                                task_id: progress_task_id.clone(),
                                percent,
                            },
                        );
                    })
                })
                .await
                .map_err(|err| err.to_string())??;
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
            emit_bridge_event(
                app,
                "transcribe-phase",
                &TranscribePhaseEvent {
                    task_id: task.id.clone(),
                    phase: "punctuate".to_string(),
                    phase_detail: None,
                },
            );
            let words_for_exec = words.to_vec();
            let media_path = task.media_path.clone();
            let task_id = task.id.clone();
            let settings = settings.clone();
            async move {
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
    task: &TaskRunExecRow,
    context: &mut TaskContext,
    projection: &TaskProjectionState,
    words: &[WordTokenDto],
    subtitle_max_words_per_segment: u32,
    with_translate: bool,
) -> Result<SegmentResumeSnapshot, String> {
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
            async move {
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
            let task_id = task.id.clone();
            let app_handle = app.cloned();
            async move {
                emit_bridge_event(
                    app_handle.as_ref(),
                    "transcribe-phase",
                    &TranscribePhaseEvent {
                        task_id,
                        phase: "summarize".to_string(),
                        phase_detail: None,
                    },
                );
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
            let task_id = task.id.clone();
            let phase_app = app.cloned();
            let progress_app = app.cloned();
            async move {
                emit_bridge_event(
                    phase_app.as_ref(),
                    "transcribe-phase",
                    &TranscribePhaseEvent {
                        task_id: task_id.clone(),
                        phase: "translate".to_string(),
                        phase_detail: None,
                    },
                );
                let mut on_progress = move |current_batch: usize, total_batches: usize| {
                    emit_bridge_event(
                        progress_app.as_ref(),
                        "translate-progress",
                        &TranslateProgressEvent {
                            task_id: task_id.clone(),
                            current_batch,
                            total_batches,
                        },
                    );
                };
                let translated = run_translate_with_theme(
                    request,
                    summarize.theme,
                    summarize.terminology_entries,
                    &mut on_progress,
                )
                .await?;
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
            let media_path = task.media_path.clone();
            let settings = settings.clone();
            let input_segments = segments.clone();
            let app_handle = app.cloned();
            async move {
                emit_bridge_event(
                    app_handle.as_ref(),
                    "transcribe-phase",
                    &TranscribePhaseEvent {
                        task_id: task_id.clone(),
                        phase: "segment_optimize".to_string(),
                        phase_detail: None,
                    },
                );
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
