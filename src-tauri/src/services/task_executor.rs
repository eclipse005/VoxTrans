use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::SqlitePool;
use std::sync::{Arc, Mutex};
use tauri::Emitter;
use tauri::async_runtime::spawn_blocking;

use crate::app_state::TaskWorkerRuntime;
use crate::services::file::save_srt;
use crate::services::preferences::load_user_preferences;
use crate::services::task_context::{
    STAGE_ASR, STAGE_COMPOSE, STAGE_INIT, STAGE_PUNCTUATE, STAGE_SEGMENT, STAGE_SEPARATE,
    STAGE_SUMMARIZE, STAGE_TRANSLATE, TaskContext, TaskContextSeed,
};
use crate::services::task_engine::{EnqueueTaskRequest, enqueue_task};
use crate::services::task_log::{TaskLogger, event};
use crate::services::task_worker;
use crate::services::transcribe::{TranscribeRequest, transcribe_blocking};
use crate::services::transcription::{RunPostAsrPipelineRequest, run_post_asr_pipeline};
use crate::services::translate::types::{
    TranslatePipelineRequest, TranslateTerminologyEntry, TranslateToken,
};
use crate::services::translate::run_translate_pipeline_with_phase;

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
    context_json: String,
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
    execute_logger.event(
        event::TRANSCRIBE_STARTED,
        Some(&json!({
            "stage": "execute_entry",
            "intentOverride": request.intent
        })),
    );

    let task = sqlx::query_as::<_, TaskRunExecRow>(
        "SELECT id, media_path, media_kind, size_bytes, intent, source_lang, target_lang,
                settings_snapshot_json, created_at, context_json
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
    let mut context = TaskContext::parse_or_new(
        &task.context_json,
        TaskContextSeed {
            task_id: task.id.clone(),
            intent: task.intent.clone(),
            source_lang: task.source_lang.clone(),
            target_lang: task.target_lang.clone(),
            media_path: task.media_path.clone(),
            media_kind: task.media_kind.clone(),
            media_size_bytes: task.size_bytes.max(0) as u64,
            settings_snapshot,
            created_at: task.created_at,
        },
    );
    context.mark_stage_running(STAGE_INIT);
    context.set_queue_projection("processing", "initializing", 0, 0, 0, "");
    persist_task_context(pool, &task.id, &context).await?;

    let intent = request
        .intent
        .as_deref()
        .map(|v| v.trim().to_uppercase())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| task.intent.trim().to_uppercase());
    context.task.intent = intent.clone();
    let run_result = if intent == "TRANSLATE_ONLY" {
        run_translate_only(pool, app.as_ref(), &task, &mut context).await
    } else if intent == "TRANSCRIBE_TRANSLATE" {
        run_transcribe_and_maybe_translate(pool, app.as_ref(), &task, true, &mut context).await
    } else {
        run_transcribe_and_maybe_translate(pool, app.as_ref(), &task, false, &mut context).await
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
            context.set_queue_projection(
                "done",
                "",
                100,
                done.segment_total.max(0) as u32,
                done.segment_total.max(0) as u32,
                "",
            );
            context.set_editor_projection(
                done.subtitle_segments_json.clone(),
                done.result_text.clone(),
                done.result_srt.clone(),
                String::new(),
            );
            persist_task_context(pool, &task.id, &context).await?;
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
            context.set_queue_projection("error", "", 0, 0, 0, &err);
            persist_task_context(pool, &task.id, &context).await?;
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
    task_worker::wait_worker_finish(runtime, request.task_id.trim()).await
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
    if intent == "TRANSLATE_ONLY" || intent == "TRANSCRIBE_TRANSLATE" {
        intent
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

async fn run_transcribe_and_maybe_translate(
    pool: &SqlitePool,
    app: Option<&tauri::AppHandle>,
    task: &TaskRunExecRow,
    with_translate: bool,
    context: &mut TaskContext,
) -> Result<DonePayload, String> {
    let mut transcribe_audio_path = task.media_path.clone();
    let settings_before_asr = load_user_preferences(pool).await?.settings;
    if settings_before_asr.enable_vocal_separation {
        context.mark_stage_running(STAGE_SEPARATE);
        emit_bridge_event(
            app,
            "transcribe-phase",
            &TranscribePhaseEvent {
                task_id: task.id.clone(),
                phase: "separating".to_string(),
            },
        );
        let app_handle = app.cloned();
        let req = crate::services::demucs::SeparateVocalsRequest {
            task_id: task.id.clone(),
            audio_path: task.media_path.clone(),
            model: settings_before_asr.demucs_model.clone(),
        };
        let task_id = task.id.clone();
        let separated = spawn_blocking(move || {
            crate::services::demucs::separate_vocals_blocking(req, |percent| {
                emit_bridge_event(
                    app_handle.as_ref(),
                    "separate-progress",
                    &SeparateProgressEvent {
                        task_id: task_id.clone(),
                        percent,
                    },
                );
            })
        })
        .await
        .map_err(|err| err.to_string())??;
        transcribe_audio_path = separated.vocals_path;
        context.mark_stage_done(
            STAGE_SEPARATE,
            json!({ "enabled": true }),
            Value::Null,
        );
    }

    context.mark_stage_running(STAGE_ASR);
    let app_handle = app.cloned();
    let task_id = task.id.clone();
    let transcribe_req = TranscribeRequest {
        task_id: task.id.clone(),
        audio_path: transcribe_audio_path,
        provider: settings_before_asr.provider,
        chunk_target_seconds: settings_before_asr.chunk_target_seconds,
        model_dir: None,
    };
    let transcribed = spawn_blocking(move || {
        transcribe_blocking(transcribe_req, |current, total| {
            emit_bridge_event(
                app_handle.as_ref(),
                "transcribe-progress",
                &TranscribeProgressEvent {
                    task_id: task_id.clone(),
                    current_segment: current,
                    total_segments: total,
                },
            );
        })
    })
    .await
    .map_err(|err| err.to_string())??;
    context.mark_stage_done(
        STAGE_ASR,
        json!({
            "segmentTotal": transcribed.segment_total,
            "audioDurationSec": transcribed.audio_duration_sec,
            "provider": transcribed.execution_provider,
        }),
        json!({
            "transcribeElapsedSec": transcribed.transcribe_elapsed_sec,
            "vadElapsedSec": transcribed.vad_elapsed_sec,
        }),
    );

    let settings_before_post = load_user_preferences(pool).await?.settings;
    context.mark_stage_running(STAGE_PUNCTUATE);
    context.mark_stage_running(STAGE_SEGMENT);
    let app_handle = app.cloned();
    let phase_task_id = task.id.clone();
    let processed = run_post_asr_pipeline(
        RunPostAsrPipelineRequest {
            task_id: task.id.clone(),
            audio_path: task.media_path.clone(),
            words: transcribed.words.clone(),
            subtitle_max_words_per_segment: settings_before_post.subtitle_max_words_per_segment,
            enable_punctuation_optimization: settings_before_post.enable_punctuation_optimization,
            translate_api_key: settings_before_post.translate_api_key.clone(),
            translate_base_url: settings_before_post.translate_base_url.clone(),
            translate_model: settings_before_post.translate_model.clone(),
            llm_concurrency: settings_before_post.llm_concurrency,
        },
        move |phase| {
            emit_bridge_event(
                app_handle.as_ref(),
                "transcribe-phase",
                &TranscribePhaseEvent {
                    task_id: phase_task_id.clone(),
                    phase: phase.to_string(),
                },
            );
        },
    )
    .await?;
    context.mark_stage_done(
        STAGE_PUNCTUATE,
        json!({
            "wordTotal": processed.words.len(),
        }),
        json!({
            "postAsrElapsedSec": processed.post_asr_elapsed_sec
        }),
    );
    context.mark_stage_done(
        STAGE_SEGMENT,
        json!({
            "segmentTotal": processed.segments.len(),
            "sourceSrtPath": processed.srt_output_path,
        }),
        Value::Null,
    );

    if !with_translate {
        save_srt(crate::services::file::SaveSrtRequest {
            task_id: Some(task.id.clone()),
            media_path: Some(task.media_path.clone()),
            output_path: processed.srt_output_path.clone(),
            content: processed.srt.clone(),
        })?;
        let segments = processed
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

        return Ok(DonePayload {
            result_text: processed.text,
            result_srt: processed.srt,
            subtitle_segments_json: serde_json::to_string(&segments).map_err(|err| err.to_string())?,
            segment_total: transcribed.segment_total as i64,
        });
    }

    let source_segments_json = processed
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
    context.set_queue_projection(
        "processing",
        "translate",
        99,
        transcribed.segment_total as u32,
        transcribed.segment_total as u32,
        "",
    );
    context.set_editor_projection(
        source_segments_json.clone(),
        processed.text.clone(),
        processed.srt.clone(),
        String::new(),
    );
    persist_task_context(pool, &task.id, context).await?;
    context.mark_stage_running(STAGE_SUMMARIZE);
    context.mark_stage_running(STAGE_TRANSLATE);
    context.set_queue_projection(
        "processing",
        "summarize",
        99,
        transcribed.segment_total as u32,
        transcribed.segment_total as u32,
        "",
    );
    persist_task_context(pool, &task.id, context).await?;

    let translate_phase_task_id = task.id.clone();
    let translate_phase_app = app.cloned();
    let translate_progress_task_id = task.id.clone();
    let translate_progress_app = app.cloned();
    let translated = run_translate_pipeline_with_phase(TranslatePipelineRequest {
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
    }, move |phase| {
        emit_bridge_event(
            translate_phase_app.as_ref(),
            "transcribe-phase",
            &TranscribePhaseEvent {
                task_id: translate_phase_task_id.clone(),
                phase: phase.to_string(),
            },
        );
    }, move |current_batch, total_batches| {
        emit_bridge_event(
            translate_progress_app.as_ref(),
            "translate-progress",
            &TranslateProgressEvent {
                task_id: translate_progress_task_id.clone(),
                current_batch,
                total_batches,
            },
        );
    })
    .await?;
    context.mark_stage_done(
        STAGE_SUMMARIZE,
        json!({
            "topicSummary": translated.style_topic_summary,
            "toneStrategy": translated.style_tone_strategy,
        }),
        Value::Null,
    );
    context.mark_stage_done(
        STAGE_TRANSLATE,
        json!({
            "translatedSegmentTotal": translated.segments.len(),
            "batchSize": 20,
        }),
        Value::Null,
    );
    context.set_queue_projection(
        "processing",
        "translate",
        99,
        transcribed.segment_total as u32,
        transcribed.segment_total as u32,
        "",
    );
    persist_task_context(pool, &task.id, context).await?;

    save_srt(crate::services::file::SaveSrtRequest {
        task_id: Some(task.id.clone()),
        media_path: Some(task.media_path.clone()),
        output_path: processed.srt_output_path.clone(),
        content: translated.source_srt.clone(),
    })?;

    let target_output_path = crate::services::task_path::task_srt_output_path_for_lang(
        &task.id,
        std::path::Path::new(&task.media_path),
        &task.target_lang,
    );
    save_srt(crate::services::file::SaveSrtRequest {
        task_id: Some(task.id.clone()),
        media_path: Some(task.media_path.clone()),
        output_path: target_output_path.display().to_string(),
        content: translated.target_srt.clone(),
    })?;

    let merged = translated
        .segments
        .iter()
        .map(|segment| {
            json!({
                "startMs": segment.start_ms as i64,
                "endMs": segment.end_ms as i64,
                "sourceText": segment.source_text,
                "translatedText": segment.translated_text,
            })
        })
        .collect::<Vec<_>>();

    let result_text = if context.projections.editor.result_text.trim().is_empty() {
        processed.text
    } else {
        context.projections.editor.result_text.clone()
    };

    Ok(DonePayload {
        result_text,
        result_srt: translated.source_srt,
        subtitle_segments_json: serde_json::to_string(&merged).map_err(|err| err.to_string())?,
        segment_total: transcribed.segment_total as i64,
    })
}

async fn run_translate_only(
    pool: &SqlitePool,
    app: Option<&tauri::AppHandle>,
    task: &TaskRunExecRow,
    context: &mut TaskContext,
) -> Result<DonePayload, String> {
    let tokens = parse_tokens_from_segments(&context.projections.editor.subtitle_segments_json);
    if tokens.is_empty() {
        return Err("当前任务没有可翻译内容，请先执行转录".to_string());
    }
    let settings = load_user_preferences(pool).await?.settings;
    context.mark_stage_running(STAGE_SUMMARIZE);
    context.mark_stage_running(STAGE_TRANSLATE);
    let translate_phase_task_id = task.id.clone();
    let translate_phase_app = app.cloned();
    let translate_progress_task_id = task.id.clone();
    let translate_progress_app = app.cloned();
    let translated = run_translate_pipeline_with_phase(TranslatePipelineRequest {
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
    }, move |phase| {
        emit_bridge_event(
            translate_phase_app.as_ref(),
            "transcribe-phase",
            &TranscribePhaseEvent {
                task_id: translate_phase_task_id.clone(),
                phase: phase.to_string(),
            },
        );
    }, move |current_batch, total_batches| {
        emit_bridge_event(
            translate_progress_app.as_ref(),
            "translate-progress",
            &TranslateProgressEvent {
                task_id: translate_progress_task_id.clone(),
                current_batch,
                total_batches,
            },
        );
    })
    .await?;
    context.mark_stage_done(
        STAGE_SUMMARIZE,
        json!({
            "topicSummary": translated.style_topic_summary,
            "toneStrategy": translated.style_tone_strategy,
        }),
        Value::Null,
    );
    context.mark_stage_done(
        STAGE_TRANSLATE,
        json!({
            "translatedSegmentTotal": translated.segments.len(),
            "batchSize": 20,
        }),
        Value::Null,
    );
    let target_output_path = crate::services::task_path::task_srt_output_path_for_lang(
        &task.id,
        std::path::Path::new(&task.media_path),
        &task.target_lang,
    );
    save_srt(crate::services::file::SaveSrtRequest {
        task_id: Some(task.id.clone()),
        media_path: Some(task.media_path.clone()),
        output_path: target_output_path.display().to_string(),
        content: translated.target_srt.clone(),
    })?;

    let merged = translated
        .segments
        .iter()
        .map(|segment| {
            json!({
                "startMs": segment.start_ms as i64,
                "endMs": segment.end_ms as i64,
                "sourceText": segment.source_text,
                "translatedText": segment.translated_text,
            })
        })
        .collect::<Vec<_>>();
    context.set_queue_projection("processing", "translate", 99, merged.len() as u32, merged.len() as u32, "");
    persist_task_context(pool, &task.id, context).await?;

    Ok(DonePayload {
        result_text: if context.projections.editor.result_text.trim().is_empty() {
            format!("translated with {}", settings.translate_model)
        } else {
            context.projections.editor.result_text.clone()
        },
        result_srt: if context.projections.editor.result_srt.trim().is_empty() {
            translated.source_srt
        } else {
            context.projections.editor.result_srt.clone()
        },
        subtitle_segments_json: serde_json::to_string(&merged).map_err(|err| err.to_string())?,
        segment_total: merged.len() as i64,
    })
}

fn parse_tokens_from_segments(raw: &str) -> Vec<TranslateToken> {
    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(raw) else {
        return Vec::new();
    };
    let Some(arr) = parsed.as_array() else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|segment| {
            let start_ms = segment.get("startMs").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let end_ms = segment.get("endMs").and_then(|v| v.as_f64()).unwrap_or(start_ms);
            let source_text = segment
                .get("sourceText")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            if source_text.is_empty() {
                return None;
            }
            Some(TranslateToken {
                start: start_ms / 1000.0,
                end: end_ms / 1000.0,
                word: source_text,
            })
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

async fn persist_task_context(
    pool: &SqlitePool,
    task_id: &str,
    context: &TaskContext,
) -> Result<(), String> {
    let context_json = context.to_json_string()?;
    let now = unix_now();
    let is_final = matches!(
        context.runtime.status.as_str(),
        "failed" | "completed" | "cancelled"
    );
    sqlx::query(
        "UPDATE task_runs
         SET context_json = ?,
             started_at = CASE
                 WHEN started_at IS NULL AND ? = 'running' THEN ?
                 ELSE started_at
             END,
             finished_at = CASE
                 WHEN ? = 1 THEN ?
                 ELSE NULL
             END,
             updated_at = ?
         WHERE id = ?",
    )
    .bind(context_json)
    .bind(&context.runtime.status)
    .bind(now)
    .bind(if is_final { 1 } else { 0 })
    .bind(now)
    .bind(now)
    .bind(task_id)
    .execute(pool)
    .await
    .map_err(|err| err.to_string())?;
    Ok(())
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
