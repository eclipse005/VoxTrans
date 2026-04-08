use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::SqlitePool;
use std::sync::{Arc, Mutex};
use tauri::async_runtime::spawn_blocking;
mod events;
pub use events::TaskStateChangedEvent;
mod runtime;
mod stages;
mod state;

use self::events::{WorkspaceSyncHintEvent, emit_bridge_event, emit_task_state_changed};
use self::runtime::{
    TaskRunExecRow, build_task_state_changed_event, hydrate_task_context, hydrate_task_projection,
    load_task_runtime_error, persist_task_context, set_queue_projection,
};
use self::stages::{
    run_asr_stage, run_punctuate_stage, run_segment_optimize_stage, run_segment_stage,
    run_separate_stage, run_summarize_stage, run_translate_stage,
};
use self::state::{
    AsrResumeSnapshot, count_segments_from_json, has_source_segments_available,
    has_translated_segments_available, load_asr_resume_snapshot, load_segment_optimize_snapshot,
    load_stage_words, parse_tokens_from_segments,
};
use crate::app_state::TaskWorkerRuntime;
use crate::services::file::save_srt;
use crate::services::final_subtitle::{
    FinalSubtitleTrack, FinalSubtitleWord, final_subtitle_segments_from_source_segments,
    final_subtitle_segments_from_translate_segments, final_subtitle_segments_to_srt,
    final_subtitle_words_from_word_dtos,
};
use crate::services::preferences::load_user_preferences;
use crate::services::subtitle_render;
use crate::services::task_context::{
    STAGE_ASR, STAGE_BURNING, STAGE_COMPOSE, STAGE_INIT, STAGE_PUNCTUATE, STAGE_SEGMENT,
    TaskContext,
};
use crate::services::task_engine::{EnqueueTaskRequest, enqueue_task};
use crate::services::task_log::{TaskLogger, event};
use crate::services::task_projection::TaskProjectionState;
use crate::services::task_subtitle_composer::{
    WordTimingAnchor, apply_subtitle_beautify_to_segments,
};
use crate::services::task_worker;
use crate::services::transcribe::{BuildSegmentsRequest, build_segments_from_words};
use crate::services::translate::types::{
    TranslatePipelineRequest, TranslateTerminologyEntry, TranslateToken,
};

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
        "SELECT id, name, media_path, media_kind, size_bytes, intent, source_lang, target_lang,
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
        sqlx::query(
            "UPDATE task_runs SET intent = ?, updated_at = strftime('%s','now') WHERE id = ?",
        )
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
    emit_task_state_changed(
        app.as_ref(),
        &build_task_state_changed_event(&task, &projection),
    );

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
        run_translate_from_existing_segments(
            pool,
            app.as_ref(),
            &task,
            &mut context,
            &mut projection,
        )
        .await
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
                .segment_optimize
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
            match maybe_auto_burn_hard_subtitle(
                pool,
                app.as_ref(),
                &task,
                &done.subtitle_segments_json,
                done.segment_total.max(0) as u32,
                &mut context,
                &mut projection,
            )
            .await
            {
                Ok(Some(burned_output_path)) => {
                    TaskLogger::main_with_media(task.id.clone(), task.media_path.clone()).event(
                        "subtitle.burn.completed",
                        Some(&json!({
                            "outputPath": burned_output_path,
                        })),
                    );
                }
                Ok(None) => {}
                Err(error) => {
                    TaskLogger::main_with_media(task.id.clone(), task.media_path.clone()).event(
                        "subtitle.burn.failed",
                        Some(&json!({
                            "error": error,
                        })),
                    );
                }
            }
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
            persist_task_context(pool, &task.id, &context, &projection).await?;
            emit_task_state_changed(
                app.as_ref(),
                &build_task_state_changed_event(&task, &projection),
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
            emit_task_state_changed(
                app.as_ref(),
                &build_task_state_changed_event(&task, &projection),
            );
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
    task_worker::spawn_worker(runtime, &db_path, &request, Some(app.clone()))?;
    match task_worker::wait_worker_finish(runtime, request.task_id.trim()).await {
        Ok(()) => Ok(()),
        Err(worker_err) => {
            if let Some(real_err) = load_task_runtime_error(pool, request.task_id.trim()).await {
                return Err(real_err);
            }
            let fallback_error = finalize_worker_exit_failure(
                pool,
                app.clone(),
                request.task_id.trim(),
                &worker_err,
            )
            .await?;
            Err(fallback_error.unwrap_or(worker_err))
        }
    }
}

async fn finalize_worker_exit_failure(
    pool: &SqlitePool,
    app: tauri::AppHandle,
    task_id: &str,
    worker_err: &str,
) -> Result<Option<String>, String> {
    let Some(task) = sqlx::query_as::<_, TaskRunExecRow>(
        "SELECT id, name, media_path, media_kind, size_bytes, intent, source_lang, target_lang,
                settings_snapshot_json, created_at, overall_status, current_stage, progress_percent,
                phase_detail, segment_current, segment_total, error_message, result_text, result_srt,
                subtitle_segments_json, translated_srt
         FROM task_runs WHERE id = ?",
    )
    .bind(task_id)
    .fetch_optional(pool)
    .await
    .map_err(|err| err.to_string())?
    else {
        return Ok(None);
    };

    if task.overall_status.trim().eq_ignore_ascii_case("failed")
        || task.overall_status.trim().eq_ignore_ascii_case("completed")
    {
        return Ok(Some(worker_err.to_string()));
    }

    let settings_snapshot = serde_json::from_str::<serde_json::Value>(&task.settings_snapshot_json)
        .unwrap_or_else(|_| json!({}));
    let mut context = hydrate_task_context(pool, &task, settings_snapshot).await?;
    let mut projection = hydrate_task_projection(&task);
    let failed_stage = context.runtime.current_stage.trim().to_string();
    let stage = if failed_stage.is_empty() {
        STAGE_INIT
    } else {
        failed_stage.as_str()
    };

    context.mark_failed(stage, "WORKER_EXIT", worker_err, true);
    set_queue_projection(
        &mut context,
        &mut projection,
        "error",
        "",
        "",
        0,
        0,
        0,
        worker_err,
    );
    persist_task_context(pool, &task.id, &context, &projection).await?;
    emit_task_state_changed(
        Some(&app),
        &build_task_state_changed_event(&task, &projection),
    );
    TaskLogger::main_with_media(task.id.clone(), task.media_path.clone()).event(
        event::TRANSCRIBE_FAILED,
        Some(&json!({
            "stage": "worker_exit",
            "error": worker_err,
        })),
    );
    Ok(Some(worker_err.to_string()))
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

async fn maybe_auto_burn_hard_subtitle(
    pool: &SqlitePool,
    app: Option<&tauri::AppHandle>,
    task: &TaskRunExecRow,
    subtitle_segments_json: &str,
    segment_total: u32,
    context: &mut TaskContext,
    projection: &mut TaskProjectionState,
) -> Result<Option<String>, String> {
    if !task.media_kind.trim().eq_ignore_ascii_case("video") {
        return Ok(None);
    }
    if subtitle_segments_json.trim().is_empty() || subtitle_segments_json.trim() == "[]" {
        return Ok(None);
    }

    let settings = load_user_preferences(pool).await?.settings;
    if !settings.auto_burn_hard_subtitle {
        return Ok(None);
    }

    context.mark_stage_running(STAGE_BURNING);
    set_queue_projection(
        context,
        projection,
        "processing",
        "burning",
        "",
        99,
        segment_total,
        segment_total,
        "",
    );
    persist_task_context(pool, &task.id, context, projection).await?;
    emit_task_state_changed(app, &build_task_state_changed_event(task, projection));

    let request = subtitle_render::BurnHardSubtitleRequest {
        task_id: task.id.clone(),
        media_path: task.media_path.clone(),
        subtitle_segments_json: subtitle_segments_json.to_string(),
        burn_mode: settings.subtitle_burn_mode,
        style: settings.subtitle_render_style,
    };
    let response = spawn_blocking(move || subtitle_render::burn_hard_subtitle(request))
        .await
        .map_err(|err| err.to_string())??;
    Ok(Some(response.output_path))
}

struct DonePayload {
    result_text: String,
    result_srt: String,
    subtitle_segments_json: String,
    segment_total: i64,
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
    let src_output_path =
        crate::services::task_path::task_src_srt_output_path(task_id, media_path_obj);
    let trans_output_path =
        crate::services::task_path::task_trans_srt_output_path(task_id, media_path_obj);
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
    asr_resume: Option<AsrResumeSnapshot>,
) -> Result<DonePayload, String> {
    let settings_before_asr = load_user_preferences(pool).await?.settings;
    let transcribe_audio_path = if asr_resume.is_some() {
        task.media_path.clone()
    } else {
        run_separate_stage(pool, app, task, context, projection, &settings_before_asr).await?
    };
    let transcribed = run_asr_stage(
        pool,
        app,
        task,
        context,
        projection,
        transcribe_audio_path,
        &settings_before_asr,
    )
    .await?;

    let settings_before_post = load_user_preferences(pool).await?.settings;
    let words = run_punctuate_stage(
        pool,
        app,
        task,
        context,
        projection,
        &transcribed.words,
        &settings_before_post,
    )
    .await?;

    let processed = run_segment_stage(
        pool,
        app,
        task,
        context,
        projection,
        &words,
        settings_before_post.subtitle_max_words_per_segment,
        with_translate,
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
    let summarize_snapshot =
        run_summarize_stage(pool, app, task, context, projection, &translate_request).await?;
    let translate_snapshot = run_translate_stage(
        pool,
        app,
        task,
        context,
        projection,
        &translate_request,
        &summarize_snapshot,
    )
    .await?;
    let word_timestamps = words
        .iter()
        .map(|w| WordTimingAnchor {
            start: w.start,
            end: w.end,
            word: w.word.clone(),
        })
        .collect::<Vec<_>>();
    let segment_optimize_snapshot = run_segment_optimize_stage(
        pool,
        app,
        task,
        context,
        projection,
        &settings_before_post,
        translate_snapshot.segments.clone(),
        &word_timestamps,
    )
    .await?;

    let final_segments = apply_subtitle_beautify_to_segments(
        &segment_optimize_snapshot.segments,
        settings_before_post.enable_subtitle_beautify,
    );
    let final_source_words = final_subtitle_words_from_word_dtos(&words);
    let merged =
        final_subtitle_segments_from_translate_segments(&final_segments, &final_source_words);
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
    let summarize_snapshot =
        run_summarize_stage(pool, app, task, context, projection, &translate_request).await?;
    let translate_snapshot = run_translate_stage(
        pool,
        app,
        task,
        context,
        projection,
        &translate_request,
        &summarize_snapshot,
    )
    .await?;
    let segment_optimize_snapshot = run_segment_optimize_stage(
        pool,
        app,
        task,
        context,
        projection,
        &settings,
        translate_snapshot.segments.clone(),
        &word_timestamps_for_opt,
    )
    .await?;
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

fn done_payload_from_projection(projection: &TaskProjectionState) -> DonePayload {
    let segment_total = count_segments_from_json(&projection.editor.subtitle_segments_json);
    DonePayload {
        result_text: projection.editor.result_text.clone(),
        result_srt: projection.editor.result_srt.clone(),
        subtitle_segments_json: projection.editor.subtitle_segments_json.clone(),
        segment_total,
    }
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
