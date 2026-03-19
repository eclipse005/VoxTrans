use serde::Deserialize;
use serde_json::json;
use sqlx::SqlitePool;
use tauri::Emitter;
use tauri::async_runtime::spawn_blocking;

use crate::services::file::save_srt;
use crate::services::preferences::load_user_preferences;
use crate::services::task_engine::{EnqueueTaskRequest, enqueue_task};
use crate::services::task_log::{TaskLogger, event};
use crate::services::transcribe::{TranscribeRequest, transcribe_blocking};
use crate::services::transcription::{RunPostAsrPipelineRequest, run_post_asr_pipeline};
use crate::services::translate::types::{TranslatePipelineRequest, TranslateToken};
use crate::services::translate::run_translate_pipeline;

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

#[derive(Debug, sqlx::FromRow)]
struct TaskRunExecRow {
    id: String,
    media_path: String,
    intent: String,
    transcribe_status: String,
    subtitle_segments_json: String,
    result_text: String,
}

pub async fn execute_task_run(
    pool: &SqlitePool,
    app: tauri::AppHandle,
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
        "SELECT id, media_path, intent, transcribe_status, subtitle_segments_json, result_text
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

    set_running(pool, &task.id).await?;

    let intent = request
        .intent
        .as_deref()
        .map(|v| v.trim().to_uppercase())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| task.intent.trim().to_uppercase());
    let run_result = if intent == "TRANSLATE_ONLY" {
        run_translate_only(pool, &task).await
    } else if intent == "TRANSCRIBE_TRANSLATE" {
        run_transcribe_and_maybe_translate(pool, &app, &task, true).await
    } else {
        run_transcribe_and_maybe_translate(pool, &app, &task, false).await
    };

    match run_result {
        Ok(done) => {
            set_done(pool, &task.id, &done).await?;
            TaskLogger::main_with_media(task.id.clone(), task.media_path.clone()).event(
                event::TRANSCRIBE_COMPLETED,
                Some(&json!({
                    "stage": "execute_entry"
                })),
            );
            Ok(())
        }
        Err(err) => {
            set_failed(pool, &task.id, &err).await?;
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

pub async fn execute_task_batch(
    pool: &SqlitePool,
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
        match execute_task_run(
            pool,
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

pub async fn enqueue_and_execute_task_batch(
    pool: &SqlitePool,
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

    let mut response = execute_task_batch(
        pool,
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
    app: &tauri::AppHandle,
    task: &TaskRunExecRow,
    with_translate: bool,
) -> Result<DonePayload, String> {
    let mut transcribe_audio_path = task.media_path.clone();
    let settings_before_asr = load_user_preferences(pool).await?.settings;
    if settings_before_asr.enable_vocal_separation {
        let _ = app.emit(
            "transcribe-phase",
            TranscribePhaseEvent {
                task_id: task.id.clone(),
                phase: "separating".to_string(),
            },
        );
        let app_handle = app.clone();
        let req = crate::services::demucs::SeparateVocalsRequest {
            task_id: task.id.clone(),
            audio_path: task.media_path.clone(),
            model: settings_before_asr.demucs_model.clone(),
        };
        let task_id = task.id.clone();
        let separated = spawn_blocking(move || {
            crate::services::demucs::separate_vocals_blocking(req, |percent| {
                let _ = app_handle.emit(
                    "separate-progress",
                    SeparateProgressEvent {
                        task_id: task_id.clone(),
                        percent,
                    },
                );
            })
        })
        .await
        .map_err(|err| err.to_string())??;
        transcribe_audio_path = separated.vocals_path;
    }

    let app_handle = app.clone();
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
            let _ = app_handle.emit(
                "transcribe-progress",
                TranscribeProgressEvent {
                    task_id: task_id.clone(),
                    current_segment: current,
                    total_segments: total,
                },
            );
        })
    })
    .await
    .map_err(|err| err.to_string())??;

    let settings_before_post = load_user_preferences(pool).await?.settings;
    let app_handle = app.clone();
    let phase_task_id = task.id.clone();
    let processed = run_post_asr_pipeline(
        RunPostAsrPipelineRequest {
            task_id: task.id.clone(),
            audio_path: task.media_path.clone(),
            words: transcribed.words.clone(),
            subtitle_max_words_per_segment: settings_before_post.subtitle_max_words_per_segment,
            enable_punctuation_optimization: settings_before_post.enable_punctuation_optimization,
            translate_api_key: settings_before_post.translate_api_key,
            translate_base_url: settings_before_post.translate_base_url,
            translate_model: settings_before_post.translate_model,
            llm_concurrency: settings_before_post.llm_concurrency,
        },
        move |phase| {
            let _ = app_handle.emit(
                "transcribe-phase",
                TranscribePhaseEvent {
                    task_id: phase_task_id.clone(),
                    phase: phase.to_string(),
                },
            );
        },
    )
    .await?;

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

    let translated = run_translate_pipeline(TranslatePipelineRequest {
        task_id: task.id.clone(),
        media_path: task.media_path.clone(),
        source_lang: "en".to_string(),
        target_lang: "zh-CN".to_string(),
        tokens: transcribed
            .words
            .iter()
            .map(|word| TranslateToken {
                start: word.start,
                end: word.end,
                word: word.word.clone(),
            })
            .collect(),
    })?;

    save_srt(crate::services::file::SaveSrtRequest {
        task_id: Some(task.id.clone()),
        media_path: Some(task.media_path.clone()),
        output_path: processed.srt_output_path.clone(),
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

    let result_text = if task.result_text.trim().is_empty() {
        processed.text
    } else {
        task.result_text.clone()
    };

    Ok(DonePayload {
        result_text,
        result_srt: translated.target_srt,
        subtitle_segments_json: serde_json::to_string(&merged).map_err(|err| err.to_string())?,
        segment_total: transcribed.segment_total as i64,
    })
}

async fn run_translate_only(pool: &SqlitePool, task: &TaskRunExecRow) -> Result<DonePayload, String> {
    let tokens = parse_tokens_from_segments(&task.subtitle_segments_json);
    if tokens.is_empty() {
        return Err("当前任务没有可翻译内容，请先执行转录".to_string());
    }
    let translated = run_translate_pipeline(TranslatePipelineRequest {
        task_id: task.id.clone(),
        media_path: task.media_path.clone(),
        source_lang: "en".to_string(),
        target_lang: "zh-CN".to_string(),
        tokens,
    })?;
    let settings = load_user_preferences(pool).await?.settings;
    let output_path = crate::services::task_path::task_srt_output_path(
        &task.id,
        std::path::Path::new(&task.media_path),
    );
    save_srt(crate::services::file::SaveSrtRequest {
        task_id: Some(task.id.clone()),
        media_path: Some(task.media_path.clone()),
        output_path: output_path.display().to_string(),
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

    Ok(DonePayload {
        result_text: if task.result_text.trim().is_empty() {
            format!("translated with {}", settings.translate_model)
        } else {
            task.result_text.clone()
        },
        result_srt: translated.target_srt,
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

async fn set_running(pool: &SqlitePool, task_id: &str) -> Result<(), String> {
    sqlx::query(
        "UPDATE task_runs
         SET state = 'RUNNING', transcribe_status = 'processing', transcribe_phase = 'initializing',
             transcribe_error = '', error_code = '', error_message = '', started_at = strftime('%s','now'),
             updated_at = strftime('%s','now')
         WHERE id = ?",
    )
    .bind(task_id)
    .execute(pool)
    .await
    .map_err(|err| err.to_string())?;
    Ok(())
}

async fn set_done(pool: &SqlitePool, task_id: &str, done: &DonePayload) -> Result<(), String> {
    sqlx::query(
        "UPDATE task_runs
         SET state = 'COMPLETED', transcribe_status = 'done',
             transcribe_progress = 100, transcribe_segment_current = ?, transcribe_segment_total = ?,
             transcribe_phase = '', transcribe_error = '',
             result_text = ?, result_srt = ?, subtitle_segments_json = ?,
             finished_at = strftime('%s','now'), updated_at = strftime('%s','now')
         WHERE id = ?",
    )
    .bind(done.segment_total)
    .bind(done.segment_total)
    .bind(&done.result_text)
    .bind(&done.result_srt)
    .bind(&done.subtitle_segments_json)
    .bind(task_id)
    .execute(pool)
    .await
    .map_err(|err| err.to_string())?;
    Ok(())
}

async fn set_failed(pool: &SqlitePool, task_id: &str, error: &str) -> Result<(), String> {
    sqlx::query(
        "UPDATE task_runs
         SET state = 'FAILED', transcribe_status = 'error', transcribe_phase = '',
             transcribe_error = ?, error_code = 'TASK_FAILED', error_message = ?,
             finished_at = strftime('%s','now'), updated_at = strftime('%s','now')
         WHERE id = ?",
    )
    .bind(error)
    .bind(error)
    .bind(task_id)
    .execute(pool)
    .await
    .map_err(|err| err.to_string())?;
    Ok(())
}
