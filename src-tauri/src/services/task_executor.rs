use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::SqlitePool;
use std::sync::{Arc, Mutex};
use tauri::Emitter;
use tauri::async_runtime::spawn_blocking;
use voxtrans_core::subtitle::beautify::beautify_words_for_subtitle;
use voxtrans_core::subtitle::segmenter::WordToken;

use crate::app_state::TaskWorkerRuntime;
use crate::services::file::save_srt;
use crate::services::final_subtitle::{
    FinalSubtitleTrack, FinalSubtitleWord, final_subtitle_segments_from_source_segments,
    final_subtitle_segments_from_translate_segments, final_subtitle_segments_to_srt,
    final_subtitle_words_from_word_dtos, parse_final_subtitle_segments,
};
use crate::services::preferences::load_user_preferences;
use crate::services::task_projection::TaskProjectionState;
use crate::services::task_projection_store::{
    TaskProjectionHydrationInput, hydrate_task_projection as hydrate_projection_from_store,
    persist_task_projection,
};
use crate::services::task_subtitle_composer::{
    WordTimingAnchor, apply_subtitle_beautify_to_segments,
    build_bilingual_srt_from_translate_segments, build_srt_from_translate_segments,
    realign_segments_with_words,
};
use crate::services::task_stage_store::{
    TaskStageSnapshot, load_task_stage_snapshot_rows, persist_task_stage_snapshots,
};
use crate::services::task_stage_runner::run_stage;
use crate::services::task_context::{
    STAGE_ASR, STAGE_COMPOSE, STAGE_INIT, STAGE_PUNCTUATE, STAGE_SEGMENT, STAGE_SEPARATE,
    STAGE_SUMMARIZE, STAGE_TRANSLATE, STAGE_QA, STAGE_QA_LAYOUT, STAGE_QA_QUALITY, TaskContext,
    TaskContextSeed,
};
use crate::services::task_engine::{EnqueueTaskRequest, enqueue_task};
use crate::services::task_log::{TaskLogger, event};
use crate::services::task_worker;
use crate::services::transcribe::{
    BuildSegmentsRequest, SegmentWithWordsDto, TranscribeRequest, TranscribeResponse, WordTokenDto,
    build_segments_from_words,
    transcribe_blocking,
};
use crate::services::transcription::{
    PunctuationConfig,
    optimize_words_with_llm,
};
use crate::services::translate::types::{
    TranslatePipelineRequest, TranslateTerminologyEntry, TranslateToken,
};
use crate::services::translate::{run_translate_summarize, run_translate_with_theme};
use crate::services::translate::segment_optimize::{
    SEGMENT_OPTIMIZE_LAYOUT_VERSION, SegmentOptimizeRequest, run_segment_optimize,
};

const WORKER_EVENT_PREFIX: &str = "VOXTRANS_EVENT:";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecuteTaskRunRequest {
    pub task_id: String,
    #[serde(default)]
    pub intent: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecuteTaskBatchItem {
    pub task_id: String,
    #[serde(default)]
    pub intent: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecuteTaskBatchRequest {
    pub items: Vec<ExecuteTaskBatchItem>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnqueueAndExecuteTaskBatchItem {
    pub id: String,
    pub media_path: String,
    pub name: String,
    pub media_kind: String,
    pub size_bytes: u64,
    pub intent: String,
    #[serde(default)]
    pub source_lang: String,
    #[serde(default)]
    pub target_lang: String,
    #[serde(default)]
    pub max_retries: u32,
    #[serde(default)]
    pub settings_snapshot: serde_json::Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnqueueAndExecuteTaskBatchRequest {
    pub items: Vec<EnqueueAndExecuteTaskBatchItem>,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecuteTaskBatchFailure {
    pub task_id: String,
    pub error: String,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecuteTaskBatchResponse {
    pub succeeded_task_ids: Vec<String>,
    pub failed: Vec<ExecuteTaskBatchFailure>,
}

#[derive(Debug, serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct TranscribeProgressEvent {
    task_id: String,
    current_segment: usize,
    total_segments: usize,
}

#[derive(Debug, serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct SeparateProgressEvent {
    task_id: String,
    percent: u32,
}

#[derive(Debug, serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct TranscribePhaseEvent {
    task_id: String,
    phase: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    phase_detail: Option<String>,
}

#[derive(Debug, serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct WorkspaceSyncHintEvent {
    task_id: String,
}

#[derive(Debug, serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct TranslateProgressEvent {
    task_id: String,
    current_batch: usize,
    total_batches: usize,
}

#[derive(Debug, serde::Serialize)]
struct WorkerEventEnvelope<'a, T: serde::Serialize> {
    event: &'a str,
    payload: &'a T,
}

#[derive(Debug, sqlx::FromRow)]
struct TaskRunExecRow {
    id: String,
    media_path: String,
    media_kind: String,
    size_bytes: i64,
    intent: String,
    source_lang: String,
    target_lang: String,
    settings_snapshot_json: String,
    created_at: i64,
    overall_status: String,
    current_stage: String,
    progress_percent: i64,
    phase_detail: String,
    segment_current: i64,
    segment_total: i64,
    error_message: String,
    result_text: String,
    result_srt: String,
    subtitle_segments_json: String,
    translated_srt: String,
}

pub async fn execute_task_run(
    pool: &SqlitePool,
    app: Option<tauri::AppHandle>,
    request: ExecuteTaskRunRequest,
) -> Result<(), String> {
    if request.task_id.trim().is_empty() {
        TaskLogger::main("unknown").event(
            event::TRANSCRIBE_FAILED,
            Some(&json!({
                "stage": "execute_entry",
                "error": "taskId is required"
            })),
        );
        return Err("taskId is required".to_string());
    }
    let execute_logger = TaskLogger::main(request.task_id.trim().to_string());

    let task = sqlx::query_as::<_, TaskRunExecRow>(
        "SELECT id, media_path, media_kind, size_bytes, intent, source_lang, target_lang,
                settings_snapshot_json, created_at, overall_status, current_stage, progress_percent,
                phase_detail, segment_current, segment_total, error_message, result_text, result_srt,
                subtitle_segments_json, translated_srt
         FROM task_runs WHERE id = ?",
    )
    .bind(request.task_id.trim())
    .fetch_optional(pool)
    .await
    .map_err(|err| err.to_string())?
    .ok_or_else(|| {
        execute_logger.event(
            event::TRANSCRIBE_FAILED,
            Some(&json!({
                "stage": "execute_entry",
                "error": "task not found"
            })),
        );
        "task not found".to_string()
    })?;

    if let Some(intent_override) = request
        .intent
        .as_deref()
        .map(|v| v.trim().to_uppercase())
        .filter(|v| !v.is_empty())
    {
        sqlx::query("UPDATE task_runs SET intent = ?, updated_at = strftime('%s','now') WHERE id = ?")
            .bind(intent_override)
            .bind(&task.id)
            .execute(pool)
            .await
            .map_err(|err| err.to_string())?;
    }

    let settings_snapshot = serde_json::from_str::<serde_json::Value>(&task.settings_snapshot_json)
        .unwrap_or_else(|_| json!({}));
    let mut context = hydrate_task_context(pool, &task, settings_snapshot).await?;
    let mut projection = hydrate_task_projection(&task);
    context.mark_stage_running(STAGE_INIT);
    set_queue_projection(
        &mut context,
        &mut projection,
        "processing",
        "initializing",
        "",
        0,
        0,
        0,
        "",
    );
    persist_task_context(pool, &task.id, &context, &projection).await?;

    let intent = request
        .intent
        .as_deref()
        .map(|v| v.trim().to_uppercase())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| task.intent.trim().to_uppercase());
    let intent = normalize_intent(&intent);
    context.task.intent = intent.clone();
    let with_translate = intent == "TRANSCRIBE_TRANSLATE";
    let run_result = if with_translate
        && load_segment_optimize_snapshot(&context).is_some()
        && has_translated_segments_available(&projection.editor.subtitle_segments_json)
    {
        Ok(done_payload_from_projection(&projection))
    } else if !with_translate
        && stage_is_done(context.stage_status(STAGE_SEGMENT))
        && has_source_segments_available(&projection.editor.subtitle_segments_json)
    {
        Ok(done_payload_from_projection(&projection))
    } else if with_translate
        && has_source_segments_available(&projection.editor.subtitle_segments_json)
    {
        run_translate_from_existing_segments(pool, app.as_ref(), &task, &mut context, &mut projection).await
    } else if let Some(asr_snapshot) = load_asr_resume_snapshot(&context) {
        run_transcribe_and_maybe_translate(
            pool,
            app.as_ref(),
            &task,
            with_translate,
            &mut context,
            &mut projection,
            Some(asr_snapshot),
        )
        .await
    } else {
        run_transcribe_and_maybe_translate(
            pool,
            app.as_ref(),
            &task,
            with_translate,
            &mut context,
            &mut projection,
            None,
        )
        .await
    };

    match run_result {
        Ok(done) => {
            context.mark_stage_done(
                STAGE_COMPOSE,
                json!({
                    "segmentTotal": done.segment_total,
                }),
                Value::Null,
            );
            context.mark_completed();
            set_queue_projection(
                &mut context,
                &mut projection,
                "done",
                "",
                "",
                100,
                done.segment_total.max(0) as u32,
                done.segment_total.max(0) as u32,
                "",
            );
            projection.set_editor(
                done.subtitle_segments_json.clone(),
                done.result_text.clone(),
                done.result_srt.clone(),
                String::new(),
            );
            persist_task_context(pool, &task.id, &context, &projection).await?;
            let src_path = crate::services::task_path::task_src_srt_output_path(
                &task.id,
                std::path::Path::new(&task.media_path),
            )
            .display()
            .to_string();
            let mut outputs = vec![json!({
                "name": "src.srt",
                "path": src_path,
            })];
            if with_translate {
                outputs.push(json!({
                    "name": "trans.srt",
                    "path": crate::services::task_path::task_trans_srt_output_path(
                        &task.id,
                        std::path::Path::new(&task.media_path),
                    ).display().to_string(),
                }));
                outputs.push(json!({
                    "name": "src_trans.srt",
                    "path": crate::services::task_path::task_src_trans_srt_output_path(
                        &task.id,
                        std::path::Path::new(&task.media_path),
                    ).display().to_string(),
                }));
                outputs.push(json!({
                    "name": "trans_src.srt",
                    "path": crate::services::task_path::task_trans_src_srt_output_path(
                        &task.id,
                        std::path::Path::new(&task.media_path),
                    ).display().to_string(),
                }));
            }

            let segment_optimize_changes = context
                .stages
                .qa_layout
                .output
                .get("appliedChangeTotal")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            TaskLogger::main_with_media(task.id.clone(), task.media_path.clone()).event(
                "transcribe.output_summary",
                Some(&json!({
                    "withTranslate": with_translate,
                    "segmentTotal": done.segment_total,
                    "resultTextChars": done.result_text.chars().count(),
                    "resultSrtChars": done.result_srt.chars().count(),
                    "segmentOptimizeAppliedChangeTotal": segment_optimize_changes,
                    "outputs": outputs,
                })),
            );
            TaskLogger::main_with_media(task.id.clone(), task.media_path.clone()).event(
                event::TRANSCRIBE_COMPLETED,
                Some(&json!({
                    "stage": "execute_entry"
                })),
            );
            Ok(())
        }
        Err(err) => {
            let failed_stage = context.runtime.current_stage.clone();
            let stage = if failed_stage.is_empty() {
                STAGE_INIT
            } else {
                failed_stage.as_str()
            };
            context.mark_failed(stage, "TASK_FAILED", &err, true);
            set_queue_projection(
                &mut context,
                &mut projection,
                "error",
                "",
                "",
                0,
                0,
                0,
                &err,
            );
            persist_task_context(pool, &task.id, &context, &projection).await?;
            TaskLogger::main_with_media(task.id.clone(), task.media_path.clone()).event(
                event::TRANSCRIBE_FAILED,
                Some(&json!({
                    "stage": "execute_entry",
                    "error": err
                })),
            );
            Err(err)
        }
    }
}

pub async fn execute_task_run_via_worker(
    pool: &SqlitePool,
    runtime: &Arc<Mutex<TaskWorkerRuntime>>,
    app: tauri::AppHandle,
    request: ExecuteTaskRunRequest,
) -> Result<(), String> {
    let db_path = task_worker::resolve_db_path(pool).await?;
    task_worker::spawn_worker(runtime, &db_path, &request, Some(app))?;
    match task_worker::wait_worker_finish(runtime, request.task_id.trim()).await {
        Ok(()) => Ok(()),
        Err(worker_err) => {
            if let Some(real_err) = load_task_runtime_error(pool, request.task_id.trim()).await {
                return Err(real_err);
            }
            Err(worker_err)
        }
    }
}

pub async fn execute_task_batch_via_worker(
    pool: &SqlitePool,
    runtime: &Arc<Mutex<TaskWorkerRuntime>>,
    app: tauri::AppHandle,
    request: ExecuteTaskBatchRequest,
) -> Result<ExecuteTaskBatchResponse, String> {
    if request.items.is_empty() {
        return Err("items is required".to_string());
    }
    let mut succeeded_task_ids: Vec<String> = Vec::new();
    let mut failed: Vec<ExecuteTaskBatchFailure> = Vec::new();
    for item in request.items {
        let task_id = item.task_id.trim().to_string();
        if task_id.is_empty() {
            continue;
        }
        match execute_task_run_via_worker(
            pool,
            runtime,
            app.clone(),
            ExecuteTaskRunRequest {
                task_id: task_id.clone(),
                intent: item.intent.clone(),
            },
        )
        .await
        {
            Ok(()) => succeeded_task_ids.push(task_id),
            Err(error) => failed.push(ExecuteTaskBatchFailure { task_id, error }),
        }
    }
    Ok(ExecuteTaskBatchResponse {
        succeeded_task_ids,
        failed,
    })
}

pub async fn enqueue_and_execute_task_batch_via_worker(
    pool: &SqlitePool,
    runtime: &Arc<Mutex<TaskWorkerRuntime>>,
    app: tauri::AppHandle,
    request: EnqueueAndExecuteTaskBatchRequest,
) -> Result<ExecuteTaskBatchResponse, String> {
    if request.items.is_empty() {
        return Err("items is required".to_string());
    }

    let mut enqueue_failed: Vec<ExecuteTaskBatchFailure> = Vec::new();
    let mut executable_items: Vec<ExecuteTaskBatchItem> = Vec::new();

    for item in request.items {
        let task_id = item.id.trim().to_string();
        if task_id.is_empty() {
            enqueue_failed.push(ExecuteTaskBatchFailure {
                task_id: String::new(),
                error: "taskId is required".to_string(),
            });
            continue;
        }
        let intent = normalize_intent(&item.intent);
        let enqueue_request = EnqueueTaskRequest {
            id: task_id.clone(),
            media_path: item.media_path,
            name: item.name,
            media_kind: item.media_kind,
            size_bytes: item.size_bytes,
            intent: intent.clone(),
            source_lang: item.source_lang,
            target_lang: item.target_lang,
            max_retries: item.max_retries,
            settings_snapshot: item.settings_snapshot,
        };

        match enqueue_task(pool, enqueue_request).await {
            Ok(_) => executable_items.push(ExecuteTaskBatchItem {
                task_id,
                intent: Some(intent),
            }),
            Err(error) => enqueue_failed.push(ExecuteTaskBatchFailure { task_id, error }),
        }
    }

    if executable_items.is_empty() {
        return Ok(ExecuteTaskBatchResponse {
            succeeded_task_ids: Vec::new(),
            failed: enqueue_failed,
        });
    }

    let mut response = execute_task_batch_via_worker(
        pool,
        runtime,
        app,
        ExecuteTaskBatchRequest {
            items: executable_items,
        },
    )
    .await?;
    response.failed.extend(enqueue_failed);
    Ok(response)
}

fn normalize_intent(raw: &str) -> String {
    let intent = raw.trim().to_uppercase();
    if intent == "TRANSCRIBE_TRANSLATE" {
        "TRANSCRIBE_TRANSLATE".to_string()
    } else {
        "TRANSCRIBE".to_string()
    }
}

struct DonePayload {
    result_text: String,
    result_srt: String,
    subtitle_segments_json: String,
    segment_total: i64,
}

#[derive(Debug, Clone)]
struct AsrResumeSnapshot {
    words: Vec<WordTokenDto>,
    segment_total: usize,
    audio_duration_sec: f64,
    vad_elapsed_sec: f64,
    transcribe_elapsed_sec: f64,
    execution_provider: String,
}

#[derive(Debug, Clone)]
struct SegmentResumeSnapshot {
    text: String,
    srt: String,
    srt_output_path: String,
    segments: Vec<SegmentWithWordsDto>,
}

#[derive(Debug, Clone)]
struct SummarizeSnapshot {
    theme: String,
    terminology_entries: Vec<TranslateTerminologyEntry>,
    terminology_primary_total: usize,
    terminology_supporting_total: usize,
}

#[derive(Debug, Clone)]
struct TranslateSnapshot {
    source_srt: String,
    target_srt: String,
    bilingual_srt_source_first: String,
    bilingual_srt_target_first: String,
    segments: Vec<crate::services::translate::types::TranslateSegment>,
}

#[derive(Debug, Clone)]
struct SegmentOptimizeSnapshot {
    segments: Vec<crate::services::translate::types::TranslateSegment>,
    report: Value,
    applied_change_total: usize,
    source_srt: String,
    target_srt: String,
    src_trans_srt: String,
    trans_src_srt: String,
}

fn save_translation_srt_set(
    task_id: &str,
    media_path: &str,
    source_srt: &str,
    target_srt: &str,
    src_trans_srt: &str,
    trans_src_srt: &str,
) -> Result<(), String> {
    let media_path_obj = std::path::Path::new(media_path);
    let src_output_path = crate::services::task_path::task_src_srt_output_path(task_id, media_path_obj);
    let trans_output_path = crate::services::task_path::task_trans_srt_output_path(task_id, media_path_obj);
    let src_trans_output_path =
        crate::services::task_path::task_src_trans_srt_output_path(task_id, media_path_obj);
    let trans_src_output_path =
        crate::services::task_path::task_trans_src_srt_output_path(task_id, media_path_obj);

    let _ = (task_id, media_path);
    write_srt_file_silent(&src_output_path, source_srt)?;
    write_srt_file_silent(&trans_output_path, target_srt)?;
    write_srt_file_silent(&src_trans_output_path, src_trans_srt)?;
    write_srt_file_silent(&trans_src_output_path, trans_src_srt)?;
    Ok(())
}

fn write_srt_file_silent(path: &std::path::Path, content: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    std::fs::write(path, content.as_bytes()).map_err(|err| err.to_string())
}

async fn run_transcribe_and_maybe_translate(
    pool: &SqlitePool,
    app: Option<&tauri::AppHandle>,
    task: &TaskRunExecRow,
    with_translate: bool,
    context: &mut TaskContext,
    projection: &mut TaskProjectionState,
    _asr_resume: Option<AsrResumeSnapshot>,
) -> Result<DonePayload, String> {
    let settings_before_asr = load_user_preferences(pool).await?.settings;
    let transcribed = run_stage(
        pool,
        &task.id,
        context,
        projection,
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
            let media_path = task.media_path.clone();
            let app_opt = app.cloned();
            let settings = settings_before_asr.clone();
            async move {
                let mut transcribe_audio_path = media_path.clone();
                if !settings.enable_vocal_separation {
                    transcribe_audio_path =
                        crate::services::demucs::prepare_audio_for_asr(&task_id, &media_path)?;
                }
                if settings.enable_vocal_separation {
                    if let Some(app_handle) = app_opt.as_ref() {
                        let _ = app_handle.emit(
                            "transcribe-phase",
                            &TranscribePhaseEvent {
                                task_id: task_id.clone(),
                                phase: "separating".to_string(),
                                phase_detail: None,
                            },
                        );
                    }
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
                    transcribe_audio_path = separated.vocals_path;
                }

                let app_handle = app_opt.clone();
                let progress_task_id = task_id.clone();
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
        persist_task_context_boxed,
    )
    .await?;

    let settings_before_post = load_user_preferences(pool).await?.settings;
    let mut words = transcribed.words.clone();

    words = run_stage(
        pool,
        &task.id,
        context,
        projection,
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
            let words_for_exec = words.clone();
            let media_path = task.media_path.clone();
            let task_id = task.id.clone();
            let settings = settings_before_post.clone();
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
        persist_task_context_boxed,
    )
    .await?;

    let processed = run_stage(
        pool,
        &task.id,
        context,
        projection,
        STAGE_SEGMENT,
        load_segment_snapshot,
        |value| !value.segments.is_empty() && !value.srt.trim().is_empty(),
        || {
            let task_id = task.id.clone();
            let media_path = task.media_path.clone();
            let words_for_exec = words.clone();
            let subtitle_max_words_per_segment = settings_before_post.subtitle_max_words_per_segment;
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
        persist_task_context_boxed,
    )
    .await?;

    if !with_translate {
        let final_segments = final_subtitle_segments_from_source_segments(&processed.segments);
        let final_source_srt =
            final_subtitle_segments_to_srt(&final_segments, FinalSubtitleTrack::Source);
        save_srt(crate::services::file::SaveSrtRequest {
            task_id: Some(task.id.clone()),
            media_path: Some(task.media_path.clone()),
            output_path: processed.srt_output_path.clone(),
            content: final_source_srt.clone(),
        })?;
        return Ok(DonePayload {
            result_text: processed.text,
            result_srt: final_source_srt,
            subtitle_segments_json: serde_json::to_string(&final_segments)
                .map_err(|err| err.to_string())?,
            segment_total: transcribed.segment_total as i64,
        });
    }

    let source_segments_json = final_subtitle_segments_from_source_segments(&processed.segments);
    let source_segments_json =
        serde_json::to_string(&source_segments_json).map_err(|err| err.to_string())?;
    set_queue_projection(
        context,
        projection,
        "processing",
        "translate",
        "",
        99,
        transcribed.segment_total as u32,
        transcribed.segment_total as u32,
        "",
    );
    projection.set_editor(
        source_segments_json.clone(),
        processed.text.clone(),
        processed.srt.clone(),
        String::new(),
    );
    persist_task_context(pool, &task.id, context, projection).await?;
    let translate_request = TranslatePipelineRequest {
        task_id: task.id.clone(),
        media_path: task.media_path.clone(),
        source_lang: task.source_lang.clone(),
        target_lang: task.target_lang.clone(),
        tokens: processed
            .segments
            .iter()
            .map(|segment| TranslateToken {
                start: segment.start,
                end: segment.end,
                word: segment.text.clone(),
            })
            .collect(),
        translate_api_key: settings_before_post.translate_api_key.clone(),
        translate_base_url: settings_before_post.translate_base_url.clone(),
        translate_model: settings_before_post.translate_model.clone(),
        llm_concurrency: settings_before_post.llm_concurrency,
        terminology_entries: if settings_before_post.enable_terminology {
            map_terminology_entries(&settings_before_post.terminology_groups)
        } else {
            Vec::new()
        },
    };
    log_pipeline_stage(task, "summarize", "started", Value::Null);
    let summarize_snapshot = run_stage(
        pool,
        &task.id,
        context,
        projection,
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
        persist_task_context_boxed,
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

    log_pipeline_stage(task, "translate", "started", Value::Null);
    let translate_snapshot = run_stage(
        pool,
        &task.id,
        context,
        projection,
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
        persist_task_context_boxed,
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

    log_pipeline_stage(task, "segment_optimize", "started", Value::Null);
    let segment_optimize_snapshot = run_stage(
        pool,
        &task.id,
        context,
        projection,
        STAGE_QA_LAYOUT,
        load_segment_optimize_snapshot,
        |value| !value.segments.is_empty(),
        || {
            let task_id = task.id.clone();
            let media_path = task.media_path.clone();
            let settings = settings_before_post.clone();
            let input_segments = translate_snapshot.segments.clone();
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
        persist_task_context_boxed,
    )
    .await?;
    let segment_optimize_snapshot = finalize_segment_optimize_timing(
        segment_optimize_snapshot,
        &words
            .iter()
            .map(|w| WordTimingAnchor {
                start: w.start,
                end: w.end,
                word: w.word.clone(),
            })
            .collect::<Vec<_>>(),
    );
    log_pipeline_stage(
        task,
        "segment_optimize",
        "completed",
        json!({
            "appliedChangeTotal": segment_optimize_snapshot.applied_change_total,
            "segmentTotal": segment_optimize_snapshot.segments.len(),
        }),
    );

    let final_segments = apply_subtitle_beautify_to_segments(
        &segment_optimize_snapshot.segments,
        settings_before_post.enable_subtitle_beautify,
    );
    let final_source_words = final_subtitle_words_from_word_dtos(&words);
    let merged = final_subtitle_segments_from_translate_segments(&final_segments, &final_source_words);
    let final_source_srt = final_subtitle_segments_to_srt(&merged, FinalSubtitleTrack::Source);
    let final_target_srt = final_subtitle_segments_to_srt(&merged, FinalSubtitleTrack::Target);
    let final_src_trans_srt =
        final_subtitle_segments_to_srt(&merged, FinalSubtitleTrack::BilingualSourceFirst);
    let final_trans_src_srt =
        final_subtitle_segments_to_srt(&merged, FinalSubtitleTrack::BilingualTargetFirst);

    save_translation_srt_set(
        &task.id,
        &task.media_path,
        &final_source_srt,
        &final_target_srt,
        &final_src_trans_srt,
        &final_trans_src_srt,
    )?;

    let result_text = if projection.editor.result_text.trim().is_empty() {
        processed.text
    } else {
        projection.editor.result_text.clone()
    };

    Ok(DonePayload {
        result_text,
        result_srt: final_source_srt,
        subtitle_segments_json: serde_json::to_string(&merged).map_err(|err| err.to_string())?,
        segment_total: merged.len() as i64,
    })
}

async fn run_translate_from_existing_segments(
    pool: &SqlitePool,
    app: Option<&tauri::AppHandle>,
    task: &TaskRunExecRow,
    context: &mut TaskContext,
    projection: &mut TaskProjectionState,
) -> Result<DonePayload, String> {
    let settings = load_user_preferences(pool).await?.settings;
    let latest_words = if let words @ Some(_) = load_stage_words(context, STAGE_PUNCTUATE) {
        words
    } else {
        load_stage_words(context, STAGE_ASR)
    };

    let tokens = if let Some(words) = latest_words.filter(|w| !w.is_empty()) {
        let built = build_segments_from_words(BuildSegmentsRequest {
            task_id: task.id.clone(),
            audio_path: task.media_path.clone(),
            words: words.clone(),
            subtitle_max_words_per_segment: settings.subtitle_max_words_per_segment,
            segment_mode: "translate_source".to_string(),
        })?;
        let source_segments_json = built
            .segments
            .iter()
            .map(|segment| {
                json!({
                    "startMs": (segment.start * 1000.0).round() as i64,
                    "endMs": (segment.end * 1000.0).round() as i64,
                    "sourceText": segment.text,
                    "translatedText": "",
                })
            })
            .collect::<Vec<_>>();
        let source_segments_json =
            serde_json::to_string(&source_segments_json).map_err(|err| err.to_string())?;
        projection.set_editor(
            source_segments_json,
            built.text.clone(),
            built.srt.clone(),
            String::new(),
        );
        persist_task_context(pool, &task.id, context, projection).await?;
        emit_bridge_event(
            app,
            "workspace-sync-hint",
            &WorkspaceSyncHintEvent {
                task_id: task.id.clone(),
            },
        );
        built
            .segments
            .iter()
            .map(|segment| TranslateToken {
                start: segment.start,
                end: segment.end,
                word: segment.text.clone(),
            })
            .collect::<Vec<_>>()
    } else {
        parse_tokens_from_segments(&projection.editor.subtitle_segments_json)
    };

    if tokens.is_empty() {
        return Err("当前任务没有可翻译内容，请先执行转录".to_string());
    }
    let word_timestamps_for_opt = tokens
        .iter()
        .map(|w| WordTimingAnchor {
            start: w.start,
            end: w.end,
            word: w.word.clone(),
        })
        .collect::<Vec<_>>();
    let translate_request = TranslatePipelineRequest {
        task_id: task.id.clone(),
        media_path: task.media_path.clone(),
        source_lang: task.source_lang.clone(),
        target_lang: task.target_lang.clone(),
        tokens,
        translate_api_key: settings.translate_api_key.clone(),
        translate_base_url: settings.translate_base_url.clone(),
        translate_model: settings.translate_model.clone(),
        llm_concurrency: settings.llm_concurrency,
        terminology_entries: if settings.enable_terminology {
            map_terminology_entries(&settings.terminology_groups)
        } else {
            Vec::new()
        },
    };
    log_pipeline_stage(task, "summarize", "started", Value::Null);
    let summarize_snapshot = run_stage(
        pool,
        &task.id,
        context,
        projection,
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
        persist_task_context_boxed,
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
    log_pipeline_stage(task, "translate", "started", Value::Null);
    let translate_snapshot = run_stage(
        pool,
        &task.id,
        context,
        projection,
        STAGE_TRANSLATE,
        load_translate_snapshot,
        |value| !value.segments.is_empty(),
        || {
            let request = translate_request.clone();
            let summarize = summarize_snapshot.clone();
            let task_id = task.id.clone();
            let progress_app = app.cloned();
            async move {
                emit_bridge_event(
                    progress_app.as_ref(),
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
        persist_task_context_boxed,
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
    log_pipeline_stage(task, "segment_optimize", "started", Value::Null);
    let segment_optimize_snapshot = run_stage(
        pool,
        &task.id,
        context,
        projection,
        STAGE_QA_LAYOUT,
        load_segment_optimize_snapshot,
        |value| !value.segments.is_empty(),
        || {
            let task_id = task.id.clone();
            let media_path = task.media_path.clone();
            let settings = settings.clone();
            let input_segments = translate_snapshot.segments.clone();
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
        persist_task_context_boxed,
    )
    .await?;
    let segment_optimize_snapshot = finalize_segment_optimize_timing(
        segment_optimize_snapshot,
        &word_timestamps_for_opt,
    );
    log_pipeline_stage(
        task,
        "segment_optimize",
        "completed",
        json!({
            "appliedChangeTotal": segment_optimize_snapshot.applied_change_total,
            "segmentTotal": segment_optimize_snapshot.segments.len(),
        }),
    );
    let final_segments = apply_subtitle_beautify_to_segments(
        &segment_optimize_snapshot.segments,
        settings.enable_subtitle_beautify,
    );
    let merged = final_subtitle_segments_from_translate_segments(
        &final_segments,
        &word_timestamps_for_opt
            .iter()
            .map(|word| FinalSubtitleWord {
                start_ms: (word.start * 1000.0).round() as i64,
                end_ms: (word.end * 1000.0).round() as i64,
                word: word.word.clone(),
            })
            .collect::<Vec<_>>(),
    );
    let final_source_srt = final_subtitle_segments_to_srt(&merged, FinalSubtitleTrack::Source);
    let final_target_srt = final_subtitle_segments_to_srt(&merged, FinalSubtitleTrack::Target);
    let final_src_trans_srt =
        final_subtitle_segments_to_srt(&merged, FinalSubtitleTrack::BilingualSourceFirst);
    let final_trans_src_srt =
        final_subtitle_segments_to_srt(&merged, FinalSubtitleTrack::BilingualTargetFirst);
    save_translation_srt_set(
        &task.id,
        &task.media_path,
        &final_source_srt,
        &final_target_srt,
        &final_src_trans_srt,
        &final_trans_src_srt,
    )?;
    set_queue_projection(
        context,
        projection,
        "processing",
        "translate",
        "",
        99,
        merged.len() as u32,
        merged.len() as u32,
        "",
    );
    persist_task_context(pool, &task.id, context, projection).await?;

    Ok(DonePayload {
        result_text: if projection.editor.result_text.trim().is_empty() {
            format!("translated with {}", settings.translate_model)
        } else {
            projection.editor.result_text.clone()
        },
        result_srt: final_source_srt,
        subtitle_segments_json: serde_json::to_string(&merged).map_err(|err| err.to_string())?,
        segment_total: merged.len() as i64,
    })
}

fn has_source_segments_available(raw: &str) -> bool {
    parse_tokens_from_segments(raw)
        .iter()
        .any(|token| !token.word.trim().is_empty())
}

fn load_asr_resume_snapshot(context: &TaskContext) -> Option<AsrResumeSnapshot> {
    if !stage_is_done(context.stage_status(STAGE_ASR)) {
        return None;
    }
    let words_value = context.stages.asr.output.get("words")?.clone();
    let words = serde_json::from_value::<Vec<WordTokenDto>>(words_value).ok()?;
    if words.is_empty() {
        return None;
    }
    let segment_total = context
        .stages
        .asr
        .output
        .get("segmentTotal")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize)
        .unwrap_or(words.len());
    let audio_duration_sec = context
        .stages
        .asr
        .output
        .get("audioDurationSec")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let execution_provider = context
        .stages
        .asr
        .output
        .get("provider")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();
    let vad_elapsed_sec = context
        .stages
        .asr
        .metrics
        .get("vadElapsedSec")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let transcribe_elapsed_sec = context
        .stages
        .asr
        .metrics
        .get("transcribeElapsedSec")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    Some(AsrResumeSnapshot {
        words,
        segment_total,
        audio_duration_sec,
        vad_elapsed_sec,
        transcribe_elapsed_sec,
        execution_provider,
    })
}

fn load_stage_words(context: &TaskContext, stage: &str) -> Option<Vec<WordTokenDto>> {
    if !stage_is_done(context.stage_status(stage)) {
        return None;
    }
    let output = match stage {
        STAGE_PUNCTUATE => &context.stages.punctuate.output,
        _ => return None,
    };
    let words_value = output.get("words")?.clone();
    let words = serde_json::from_value::<Vec<WordTokenDto>>(words_value).ok()?;
    if words.is_empty() {
        return None;
    }
    Some(words)
}

fn load_segment_snapshot(context: &TaskContext) -> Option<SegmentResumeSnapshot> {
    if !stage_is_done(context.stage_status(STAGE_SEGMENT)) {
        return None;
    }
    let output = &context.stages.segment.output;
    let text = output.get("text")?.as_str()?.to_string();
    let srt = output.get("srt")?.as_str()?.to_string();
    let srt_output_path = output.get("srtOutputPath")?.as_str()?.to_string();
    let segments_value = output.get("segments")?.clone();
    let segments = serde_json::from_value::<Vec<SegmentWithWordsDto>>(segments_value).ok()?;
    if segments.is_empty() {
        return None;
    }
    Some(SegmentResumeSnapshot {
        text,
        srt,
        srt_output_path,
        segments,
    })
}

fn load_summarize_snapshot(context: &TaskContext) -> Option<SummarizeSnapshot> {
    if !stage_is_done(context.stage_status(STAGE_SUMMARIZE)) {
        return None;
    }
    let output = &context.stages.summarize.output;
    let theme = output.get("theme")?.as_str()?.trim().to_string();
    if theme.is_empty() {
        return None;
    }
    let terminology_entries = output
        .get("terminologyEntries")
        .cloned()
        .and_then(|value| serde_json::from_value::<Vec<TranslateTerminologyEntry>>(value).ok())
        .unwrap_or_default();
    let terminology_primary_total = output
        .get("terminologyPrimaryTotal")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize)
        .unwrap_or(0);
    let terminology_supporting_total = output
        .get("terminologySupportingTotal")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize)
        .unwrap_or(0);
    Some(SummarizeSnapshot {
        theme,
        terminology_entries,
        terminology_primary_total,
        terminology_supporting_total,
    })
}

fn load_translate_snapshot(context: &TaskContext) -> Option<TranslateSnapshot> {
    if !stage_is_done(context.stage_status(STAGE_TRANSLATE)) {
        return None;
    }
    let output = &context.stages.translate.output;
    let source_srt = output.get("sourceSrt")?.as_str()?.to_string();
    let target_srt = output.get("targetSrt")?.as_str()?.to_string();
    let bilingual_srt_source_first = output.get("bilingualSrtSourceFirst")?.as_str()?.to_string();
    let bilingual_srt_target_first = output.get("bilingualSrtTargetFirst")?.as_str()?.to_string();
    let segments = serde_json::from_value::<Vec<crate::services::translate::types::TranslateSegment>>(
        output.get("segments")?.clone(),
    )
    .ok()?;
    if segments.is_empty() {
        return None;
    }
    Some(TranslateSnapshot {
        source_srt,
        target_srt,
        bilingual_srt_source_first,
        bilingual_srt_target_first,
        segments,
    })
}

fn load_segment_optimize_snapshot(context: &TaskContext) -> Option<SegmentOptimizeSnapshot> {
    load_segment_optimize_stage_snapshot(context, STAGE_QA_LAYOUT)
}

fn load_segment_optimize_stage_snapshot(context: &TaskContext, stage: &str) -> Option<SegmentOptimizeSnapshot> {
    if !stage_is_done(context.stage_status(stage)) {
        return None;
    }
    let output = match stage {
        STAGE_QA_LAYOUT => &context.stages.qa_layout.output,
        STAGE_QA_QUALITY => &context.stages.qa_quality.output,
        STAGE_QA => &context.stages.qa.output,
        _ => return None,
    };
    let segments = serde_json::from_value::<Vec<crate::services::translate::types::TranslateSegment>>(
        output.get("segments")?.clone(),
    )
    .ok()?;
    if segments.is_empty() {
        return None;
    }
    let applied_change_total = output
        .get("appliedChangeTotal")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as usize;
    let report = output.get("report").cloned().unwrap_or(Value::Null);
    let layout_version = report
        .get("layoutVersion")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    if layout_version != SEGMENT_OPTIMIZE_LAYOUT_VERSION {
        return None;
    }
    Some(SegmentOptimizeSnapshot {
        segments,
        report,
        applied_change_total,
        source_srt: output.get("sourceSrt")?.as_str()?.to_string(),
        target_srt: output.get("targetSrt")?.as_str()?.to_string(),
        src_trans_srt: output.get("srcTransSrt")?.as_str()?.to_string(),
        trans_src_srt: output.get("transSrcSrt")?.as_str()?.to_string(),
    })
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

fn has_translated_segments_available(raw: &str) -> bool {
    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(raw) else {
        return false;
    };
    let Some(arr) = parsed.as_array() else {
        return false;
    };
    arr.iter().any(|segment| {
        segment
            .get("translatedText")
            .and_then(|v| v.as_str())
            .map(|v| !v.trim().is_empty())
            .unwrap_or(false)
    })
}

fn log_pipeline_stage(task: &TaskRunExecRow, stage: &str, status: &str, extra: Value) {
    let mut payload = json!({
        "stage": stage,
        "status": status,
    });
    if let Some(obj) = payload.as_object_mut() {
        if let Some(extra_obj) = extra.as_object() {
            for (k, v) in extra_obj {
                obj.insert(k.to_string(), v.clone());
            }
        }
    }
    TaskLogger::main_with_media(task.id.clone(), task.media_path.clone())
        .event("pipeline.stage", Some(&payload));
}

fn stage_is_done(status: &str) -> bool {
    status.trim().eq_ignore_ascii_case("done")
}

fn set_queue_projection(
    context: &mut TaskContext,
    projection: &mut TaskProjectionState,
    status: &str,
    phase: &str,
    phase_detail: &str,
    progress_percent: u32,
    current: u32,
    total: u32,
    error: &str,
) {
    context.runtime.progress_percent = projection.set_queue(
        status,
        phase,
        phase_detail,
        progress_percent,
        current,
        total,
        error,
    );
}

fn done_payload_from_projection(projection: &TaskProjectionState) -> DonePayload {
    let segment_total = count_segments_from_json(&projection.editor.subtitle_segments_json);
    DonePayload {
        result_text: projection.editor.result_text.clone(),
        result_srt: projection.editor.result_srt.clone(),
        subtitle_segments_json: projection.editor.subtitle_segments_json.clone(),
        segment_total,
    }
}

fn count_segments_from_json(raw: &str) -> i64 {
    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(raw) else {
        return 0;
    };
    parsed
        .as_array()
        .map(|arr| arr.len() as i64)
        .unwrap_or(0)
}

fn parse_tokens_from_segments(raw: &str) -> Vec<TranslateToken> {
    let segments = parse_final_subtitle_segments(raw);
    let anchored_tokens = segments
        .iter()
        .flat_map(|segment| {
            segment.source_words.iter().filter_map(|word| {
                if word.word.trim().is_empty() {
                    return None;
                }
                Some(TranslateToken {
                    start: word.start_ms as f64 / 1000.0,
                    end: word.end_ms.max(word.start_ms) as f64 / 1000.0,
                    word: word.word.clone(),
                })
            })
        })
        .collect::<Vec<_>>();
    if !anchored_tokens.is_empty() {
        return anchored_tokens;
    }
    segments
        .into_iter()
        .filter_map(|segment| {
            if segment.source_text.trim().is_empty() {
                return None;
            }
            Some(TranslateToken {
                start: segment.start_ms as f64 / 1000.0,
                end: segment.end_ms.max(segment.start_ms) as f64 / 1000.0,
                word: segment.source_text,
            })
        })
        .collect()
}

fn to_core_words(words: Vec<WordTokenDto>) -> Vec<WordToken> {
    words
        .into_iter()
        .map(|word| WordToken {
            start: word.start,
            end: word.end,
            word: word.word,
        })
        .collect()
}

fn from_core_words(words: Vec<WordToken>) -> Vec<WordTokenDto> {
    words
        .into_iter()
        .map(|word| WordTokenDto {
            start: word.start,
            end: word.end,
            word: word.word,
        })
        .collect()
}

fn map_terminology_entries(
    groups: &[crate::services::preferences::TerminologyGroup],
) -> Vec<TranslateTerminologyEntry> {
    groups
        .iter()
        .flat_map(|group| {
            group.terms.iter().map(|term| TranslateTerminologyEntry {
                source: term.origin.trim().to_string(),
                target: term.target.trim().to_string(),
                note: term.note.trim().to_string(),
            })
        })
        .filter(|entry| !entry.source.is_empty() && !entry.target.is_empty())
        .collect()
}

async fn hydrate_task_context(
    pool: &SqlitePool,
    task: &TaskRunExecRow,
    settings_snapshot: Value,
) -> Result<TaskContext, String> {
    let mut context = TaskContext::new(TaskContextSeed {
        task_id: task.id.clone(),
        intent: task.intent.clone(),
        source_lang: task.source_lang.clone(),
        target_lang: task.target_lang.clone(),
        media_path: task.media_path.clone(),
        media_kind: task.media_kind.clone(),
        media_size_bytes: task.size_bytes.max(0) as u64,
        settings_snapshot,
        created_at: task.created_at,
    });

    context.runtime.status = normalize_runtime_status(&task.overall_status);
    context.runtime.current_stage = task.current_stage.clone();
    context.runtime.progress_percent = task.progress_percent.clamp(0, 100) as u32;

    let rows = load_task_stage_snapshot_rows(pool, &task.id).await?;

    for row in rows {
        let output = serde_json::from_str::<Value>(&row.output_json).unwrap_or(Value::Null);
        let metrics = serde_json::from_str::<Value>(&row.metrics_json).unwrap_or(Value::Null);
        context.set_stage_snapshot(
            &row.stage,
            row.status,
            row.started_at,
            row.finished_at,
            output,
            metrics,
            row.error_code,
            row.error_message,
        );
    }

    Ok(context)
}

fn hydrate_task_projection(task: &TaskRunExecRow) -> TaskProjectionState {
    hydrate_projection_from_store(TaskProjectionHydrationInput {
        overall_status: task.overall_status.clone(),
        current_stage: task.current_stage.clone(),
        progress_percent: task.progress_percent,
        phase_detail: task.phase_detail.clone(),
        segment_current: task.segment_current,
        segment_total: task.segment_total,
        error_message: task.error_message.clone(),
        subtitle_segments_json: task.subtitle_segments_json.clone(),
        result_text: task.result_text.clone(),
        result_srt: task.result_srt.clone(),
        translated_srt: task.translated_srt.clone(),
    })
}

async fn persist_task_context(
    pool: &SqlitePool,
    task_id: &str,
    context: &TaskContext,
    projection: &TaskProjectionState,
) -> Result<(), String> {
    let now = unix_now();
    let is_final = matches!(
        context.runtime.status.as_str(),
        "failed" | "completed"
    );
    persist_task_projection(pool, task_id, &context.runtime, projection, now, is_final).await?;

    let snapshots = [
        (STAGE_INIT, &context.stages.init),
        (STAGE_SEPARATE, &context.stages.separate),
        (STAGE_ASR, &context.stages.asr),
        (STAGE_PUNCTUATE, &context.stages.punctuate),
        (STAGE_SEGMENT, &context.stages.segment),
        (STAGE_SUMMARIZE, &context.stages.summarize),
        (STAGE_TRANSLATE, &context.stages.translate),
        (STAGE_QA, &context.stages.qa),
        (STAGE_QA_LAYOUT, &context.stages.qa_layout),
        (STAGE_QA_QUALITY, &context.stages.qa_quality),
        (STAGE_COMPOSE, &context.stages.compose),
    ]
    .iter()
    .map(|(stage, envelope)| TaskStageSnapshot {
        stage: (*stage).to_string(),
        status: envelope.status.clone(),
        started_at: envelope.started_at,
        finished_at: envelope.finished_at,
        output: envelope.output.clone(),
        metrics: envelope.metrics.clone(),
        error_code: envelope.error.as_ref().map(|e| e.code.clone()).unwrap_or_default(),
        error_message: envelope.error.as_ref().map(|e| e.message.clone()).unwrap_or_default(),
    })
    .collect::<Vec<_>>();
    persist_task_stage_snapshots(pool, task_id, &snapshots, now).await?;

    Ok(())
}

fn persist_task_context_boxed<'a>(
    pool: &'a SqlitePool,
    task_id: &'a str,
    context: &'a TaskContext,
    projection: &'a TaskProjectionState,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + 'a>> {
    Box::pin(persist_task_context(pool, task_id, context, projection))
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn emit_bridge_event<T: serde::Serialize>(
    app: Option<&tauri::AppHandle>,
    event: &str,
    payload: &T,
) {
    if let Some(app_handle) = app {
        let _ = app_handle.emit(event, payload);
        return;
    }
    if let Ok(envelope) = serde_json::to_string(&WorkerEventEnvelope { event, payload }) {
        println!("{WORKER_EVENT_PREFIX}{envelope}");
    }
}

async fn load_task_runtime_error(pool: &SqlitePool, task_id: &str) -> Option<String> {
    let task_error = sqlx::query_scalar::<_, String>(
        "SELECT error_message FROM task_runs WHERE id = ?",
    )
    .bind(task_id)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()?;
    if !task_error.trim().is_empty() {
        return Some(task_error);
    }
    sqlx::query_scalar::<_, String>(
        "SELECT error_message FROM task_stage_runs
         WHERE task_id = ? AND error_message <> ''
         ORDER BY updated_at DESC LIMIT 1",
    )
    .bind(task_id)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
}

fn normalize_runtime_status(overall_status: &str) -> String {
    match overall_status.trim().to_ascii_lowercase().as_str() {
        "queued" => "queued".to_string(),
        "running" => "running".to_string(),
        "failed" => "failed".to_string(),
        "completed" => "completed".to_string(),
        _ => "queued".to_string(),
    }
}
