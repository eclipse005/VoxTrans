use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::SqlitePool;
use std::sync::{Arc, Mutex};
use tauri::Emitter;
use tauri::async_runtime::spawn_blocking;
use voxtrans_core::subtitle::beautify::beautify_words_for_subtitle;
use voxtrans_core::subtitle::alignment::align_text_to_timestamps;
use voxtrans_core::subtitle::segmenter::WordToken;
use voxtrans_core::subtitle::srt::{SrtCue, to_srt_from_cues};

use crate::app_state::TaskWorkerRuntime;
use crate::services::file::save_srt;
use crate::services::preferences::load_user_preferences;
use crate::services::task_context::{
    STAGE_ASR, STAGE_COMPOSE, STAGE_CORRECT, STAGE_INIT, STAGE_PUNCTUATE, STAGE_SEGMENT, STAGE_SEPARATE,
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
    CorrectionConfig, CorrectionTerminologyEntry, PunctuationConfig, correct_words_with_rig_node,
    optimize_words_with_rig_node,
};
use crate::services::translate::types::{
    TranslatePipelineRequest, TranslateTerminologyEntry, TranslateToken,
};
use crate::services::translate::pipeline::beautify_translated_text;
use crate::services::translate::{run_translate_summarize, run_translate_with_style};
use crate::services::translate::qa_simple::{QaAgentRequest, run_qa_simple};

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
    context.mark_stage_running(STAGE_INIT);
    context.set_queue_projection("processing", "initializing", "", 0, 0, 0, "");
    persist_task_context(pool, &task.id, &context).await?;

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
        && stage_is_done(context.stage_status(STAGE_QA_LAYOUT))
        && has_translated_segments_available(&context.projections.editor.subtitle_segments_json)
    {
        Ok(done_payload_from_context(&context))
    } else if !with_translate
        && stage_is_done(context.stage_status(STAGE_SEGMENT))
        && has_source_segments_available(&context.projections.editor.subtitle_segments_json)
    {
        Ok(done_payload_from_context(&context))
    } else if with_translate
        && has_source_segments_available(&context.projections.editor.subtitle_segments_json)
        && !has_translated_segments_available(&context.projections.editor.subtitle_segments_json)
    {
        run_translate_from_existing_segments(pool, app.as_ref(), &task, &mut context).await
    } else if let Some(asr_snapshot) = load_asr_resume_snapshot(&context) {
        run_transcribe_and_maybe_translate(
            pool,
            app.as_ref(),
            &task,
            with_translate,
            &mut context,
            Some(asr_snapshot),
        )
        .await
    } else {
        run_transcribe_and_maybe_translate(pool, app.as_ref(), &task, with_translate, &mut context, None).await
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

            let qa_quality_changes = context
                .stages
                .qa_quality
                .output
                .get("appliedChangeTotal")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let qa_layout_changes = context
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
                    "qaQualityAppliedChangeTotal": qa_quality_changes,
                    "qaLayoutAppliedChangeTotal": qa_layout_changes,
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
            context.set_queue_projection("error", "", "", 0, 0, 0, &err);
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
    topic_summary: String,
    tone_strategy: String,
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
struct QaSnapshot {
    segments: Vec<crate::services::translate::types::TranslateSegment>,
    report: Value,
    applied_change_total: usize,
    source_srt: String,
    target_srt: String,
    src_trans_srt: String,
    trans_src_srt: String,
}

#[derive(Debug, Clone)]
struct QaWordTiming {
    start: f64,
    end: f64,
    word: String,
}

fn realign_segments_with_words(
    segments: &mut [crate::services::translate::types::TranslateSegment],
    word_timestamps: &[QaWordTiming],
) -> Value {
    if segments.is_empty() {
        return json!({ "applied": false, "reason": "empty_segments" });
    }
    let words = word_timestamps
        .iter()
        .filter_map(|w| {
            let word = w.word.trim().to_string();
            if word.is_empty() {
                return None;
            }
            Some(WordToken {
                start: w.start,
                end: w.end.max(w.start),
                word,
            })
        })
        .collect::<Vec<_>>();
    if words.is_empty() {
        return json!({ "applied": false, "reason": "empty_words" });
    }
    if words.len() < segments.len() {
        return json!({
            "applied": false,
            "reason": "insufficient_words",
            "wordTotal": words.len(),
            "segmentTotal": segments.len()
        });
    }

    let source_full_text = segments
        .iter()
        .map(|s| s.source_text.trim())
        .filter(|v| !v.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if source_full_text.is_empty() {
        return json!({ "applied": false, "reason": "empty_source_text" });
    }

    let aligned_words = align_text_to_timestamps(&source_full_text, &words);
    if aligned_words.is_empty() {
        return json!({ "applied": false, "reason": "alignment_empty" });
    }
    if aligned_words.len() < segments.len() {
        return json!({
            "applied": false,
            "reason": "alignment_insufficient_words",
            "alignedWordTotal": aligned_words.len(),
            "segmentTotal": segments.len()
        });
    }

    let mut segment_token_owners: Vec<usize> = Vec::new();
    let mut segment_token_stream: Vec<String> = Vec::new();
    for (seg_idx, segment) in segments.iter().enumerate() {
        for token in segment.source_text.split_whitespace() {
            let norm = normalize_alignment_token(token);
            if norm.is_empty() {
                continue;
            }
            segment_token_owners.push(seg_idx);
            segment_token_stream.push(norm);
        }
    }
    let aligned_word_stream = aligned_words
        .iter()
        .map(|w| normalize_alignment_token(&w.word))
        .filter(|t| !t.is_empty())
        .collect::<Vec<_>>();
    if segment_token_stream.is_empty() || aligned_word_stream.is_empty() {
        return json!({
            "applied": false,
            "reason": "alignment_token_stream_empty",
            "segmentTokenTotal": segment_token_stream.len(),
            "alignedWordTokenTotal": aligned_word_stream.len()
        });
    }

    let matched_pairs = lcs_match_pairs(&segment_token_stream, &aligned_word_stream);
    let mut segment_boundaries: Vec<Option<(usize, usize)>> = vec![None; segments.len()];
    for (segment_token_idx, aligned_word_idx) in matched_pairs {
        let seg_idx = segment_token_owners[segment_token_idx];
        match &mut segment_boundaries[seg_idx] {
            Some((start, end)) => {
                if aligned_word_idx < *start {
                    *start = aligned_word_idx;
                }
                if aligned_word_idx > *end {
                    *end = aligned_word_idx;
                }
            }
            None => {
                segment_boundaries[seg_idx] = Some((aligned_word_idx, aligned_word_idx));
            }
        }
    }

    let total_segments = segments.len();
    let mut cursor_word = 0usize;
    let mut changed = 0usize;
    let mut fallback_segments = 0usize;
    for (idx, segment) in segments.iter_mut().enumerate() {
        if cursor_word >= aligned_words.len() {
            break;
        }
        let (mut start_idx, mut end_idx) = match segment_boundaries[idx] {
            Some(boundary) => boundary,
            None => {
                fallback_segments += 1;
                let remaining = total_segments.saturating_sub(idx).max(1);
                let remaining_words = aligned_words.len().saturating_sub(cursor_word).max(1);
                let allocation = if idx + 1 == total_segments {
                    remaining_words
                } else {
                    (remaining_words / remaining).max(1)
                };
                let start = cursor_word.min(aligned_words.len().saturating_sub(1));
                let end = (start + allocation.saturating_sub(1)).min(aligned_words.len().saturating_sub(1));
                (start, end)
            }
        };
        if start_idx < cursor_word {
            start_idx = cursor_word;
        }
        if end_idx < start_idx {
            end_idx = start_idx;
        }
        end_idx = end_idx.min(aligned_words.len().saturating_sub(1));

        let start_word = &aligned_words[start_idx];
        let end_word = &aligned_words[end_idx];
        let new_start_ms = (start_word.start.max(0.0) * 1000.0).round() as u64;
        let new_end_ms = (end_word.end.max(start_word.start) * 1000.0).round() as u64;
        if segment.start_ms != new_start_ms || segment.end_ms != new_end_ms {
            changed += 1;
        }
        segment.start_ms = new_start_ms;
        segment.end_ms = new_end_ms.max(new_start_ms);
        cursor_word = end_idx.saturating_add(1);
    }

    json!({
        "applied": true,
        "segmentTotal": total_segments,
        "wordTotal": words.len(),
        "alignedWordTotal": aligned_words.len(),
        "changedSegmentTotal": changed,
        "fallbackSegmentTotal": fallback_segments
    })
}

fn normalize_alignment_token(token: &str) -> String {
    token
        .chars()
        .filter(|ch| ch.is_alphanumeric())
        .flat_map(|ch| ch.to_lowercase())
        .collect::<String>()
}

fn lcs_match_pairs(left: &[String], right: &[String]) -> Vec<(usize, usize)> {
    if left.is_empty() || right.is_empty() {
        return Vec::new();
    }
    let n = left.len();
    let m = right.len();
    let mut dp = vec![vec![0usize; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[i][j] = if left[i] == right[j] {
                dp[i + 1][j + 1] + 1
            } else {
                dp[i + 1][j].max(dp[i][j + 1])
            };
        }
    }
    let mut pairs = Vec::new();
    let (mut i, mut j) = (0usize, 0usize);
    while i < n && j < m {
        if left[i] == right[j] {
            pairs.push((i, j));
            i += 1;
            j += 1;
        } else if dp[i + 1][j] >= dp[i][j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    pairs
}

fn build_srt_from_translate_segments(
    segments: &[crate::services::translate::types::TranslateSegment],
    translated: bool,
) -> String {
    let cues = segments
        .iter()
        .enumerate()
        .map(|(idx, segment)| SrtCue {
            index: idx + 1,
            start_ms: segment.start_ms,
            end_ms: segment.end_ms.max(segment.start_ms),
            text: if translated {
                segment.translated_text.trim().to_string()
            } else {
                segment.source_text.trim().to_string()
            },
        })
        .collect::<Vec<_>>();
    to_srt_from_cues(&cues)
}

fn build_bilingual_srt_from_translate_segments(
    segments: &[crate::services::translate::types::TranslateSegment],
    source_first: bool,
) -> String {
    let cues = segments
        .iter()
        .enumerate()
        .map(|(idx, segment)| {
            let source = segment.source_text.trim();
            let translated = segment.translated_text.trim();
            let text = if source_first {
                format!("{source}\n{translated}")
            } else {
                format!("{translated}\n{source}")
            };
            SrtCue {
                index: idx + 1,
                start_ms: segment.start_ms,
                end_ms: segment.end_ms.max(segment.start_ms),
                text,
            }
        })
        .collect::<Vec<_>>();
    to_srt_from_cues(&cues)
}

fn apply_subtitle_beautify_to_segments(
    segments: &[crate::services::translate::types::TranslateSegment],
    enabled: bool,
) -> Vec<crate::services::translate::types::TranslateSegment> {
    if !enabled {
        return segments.to_vec();
    }
    segments
        .iter()
        .cloned()
        .map(|mut seg| {
            seg.translated_text = beautify_translated_text(&seg.translated_text);
            seg
        })
        .collect()
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
    asr_resume: Option<AsrResumeSnapshot>,
) -> Result<DonePayload, String> {
    let resumed_from_asr = asr_resume.is_some();
    let settings_before_asr = load_user_preferences(pool).await?.settings;
    let transcribed = run_stage(
        pool,
        &task.id,
        context,
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
    )
    .await?;

    let settings_before_post = load_user_preferences(pool).await?.settings;
    let mut words = transcribed.words.clone();

    words = run_stage(
        pool,
        &task.id,
        context,
        STAGE_PUNCTUATE,
        |ctx| load_stage_words(ctx, STAGE_PUNCTUATE),
        |value| !value.is_empty(),
        || {
            if resumed_from_asr {
                emit_bridge_event(
                    app,
                    "transcribe-phase",
                    &TranscribePhaseEvent {
                        task_id: task.id.clone(),
                        phase: "punctuate".to_string(),
                        phase_detail: None,
                    },
                );
            }
            let words_for_exec = words.clone();
            let media_path = task.media_path.clone();
            let task_id = task.id.clone();
            let settings = settings_before_post.clone();
            async move {
                let optimized_words = optimize_words_with_rig_node(
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
    .await?;

    words = run_stage(
        pool,
        &task.id,
        context,
        STAGE_CORRECT,
        |ctx| load_stage_words(ctx, STAGE_CORRECT),
        |value| !value.is_empty(),
        || {
            let words_for_exec = words.clone();
            let media_path = task.media_path.clone();
            let task_id = task.id.clone();
            let source_lang = task.source_lang.clone();
            let settings = settings_before_post.clone();
            async move {
                let corrected_words = correct_words_with_rig_node(
                    &task_id,
                    &media_path,
                    to_core_words(words_for_exec),
                    &CorrectionConfig {
                        source_lang,
                        base_url: settings.translate_base_url.clone(),
                        api_key: settings.translate_api_key.clone(),
                        model: settings.translate_model.clone(),
                        terminology_entries: if settings.enable_terminology {
                            map_correction_terminology_entries(&settings.terminology_groups)
                        } else {
                            Vec::new()
                        },
                    },
                )
                .await?;
                Ok(from_core_words(corrected_words))
            }
        },
        |value| json!({ "wordTotal": value.len(), "words": value }),
        |_| Value::Null,
    )
    .await?;

    let processed = run_stage(
        pool,
        &task.id,
        context,
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
        "",
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
        STAGE_SUMMARIZE,
        load_summarize_snapshot,
        |value| {
            !value.topic_summary.trim().is_empty() && !value.tone_strategy.trim().is_empty()
        },
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
                let (topic_summary, tone_strategy) = run_translate_summarize(&request).await?;
                Ok(SummarizeSnapshot {
                    topic_summary,
                    tone_strategy,
                })
            }
        },
        |value| {
            json!({
                "topicSummary": value.topic_summary,
                "toneStrategy": value.tone_strategy,
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
            "topicSummary": summarize_snapshot.topic_summary,
            "toneStrategy": summarize_snapshot.tone_strategy,
        }),
    );

    log_pipeline_stage(task, "translate", "started", Value::Null);
    let translate_snapshot = run_stage(
        pool,
        &task.id,
        context,
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
                let translated = run_translate_with_style(
                    request,
                    summarize.topic_summary,
                    summarize.tone_strategy,
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

    log_pipeline_stage(task, "qa_quality", "started", Value::Null);
    let qa_quality_snapshot = run_stage(
        pool,
        &task.id,
        context,
        STAGE_QA_QUALITY,
        load_qa_quality_snapshot,
        |value| !value.segments.is_empty(),
        || {
            let task_id = task.id.clone();
            let media_path = task.media_path.clone();
            let source_lang = task.source_lang.clone();
            let target_lang = task.target_lang.clone();
            let settings = settings_before_post.clone();
            let final_segments = translate_snapshot.segments.clone();
            let style_guidance = summarize_snapshot.tone_strategy.clone();
            let app_handle = app.cloned();
            async move {
                emit_bridge_event(
                    app_handle.as_ref(),
                    "transcribe-phase",
                    &TranscribePhaseEvent {
                        task_id: task_id.clone(),
                        phase: "qa_quality".to_string(),
                        phase_detail: None,
                    },
                );
                let qa_pass2 = run_qa_simple(QaAgentRequest {
                    task_id,
                    media_path,
                    source_lang,
                    target_lang,
                    api_key: settings.translate_api_key.clone(),
                    base_url: settings.translate_base_url.clone(),
                    model: settings.translate_model.clone(),
                    llm_concurrency: settings.llm_concurrency,
                    source_max_words_per_segment: settings.subtitle_max_words_per_segment,
                    target_reference_len: settings.subtitle_length_reference,
                    terminology_entries: if settings.enable_terminology {
                        map_terminology_entries(&settings.terminology_groups)
                    } else {
                        Vec::new()
                    },
                    segments: final_segments,
                    style_guidance: style_guidance.clone(),
                    pass: "pass2_quality".to_string(),
                    prior_report: None,
                })
                .await
                .map_err(|err| format!("qa quality failed: {err}"))?;
                Ok(QaSnapshot {
                    segments: qa_pass2.segments,
                    report: qa_pass2.report,
                    applied_change_total: qa_pass2.applied_changes.len(),
                    source_srt: qa_pass2.source_srt,
                    target_srt: qa_pass2.target_srt,
                    src_trans_srt: qa_pass2.bilingual_srt_source_first,
                    trans_src_srt: qa_pass2.bilingual_srt_target_first,
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
    log_pipeline_stage(
        task,
        "qa_quality",
        "completed",
        json!({
            "appliedChangeTotal": qa_quality_snapshot.applied_change_total,
            "segmentTotal": qa_quality_snapshot.segments.len(),
        }),
    );

    log_pipeline_stage(task, "qa_layout", "started", Value::Null);
    let qa_snapshot = run_stage(
        pool,
        &task.id,
        context,
        STAGE_QA_LAYOUT,
        load_qa_layout_snapshot,
        |value| !value.segments.is_empty(),
        || {
            let task_id = task.id.clone();
            let media_path = task.media_path.clone();
            let source_lang = task.source_lang.clone();
            let target_lang = task.target_lang.clone();
            let settings = settings_before_post.clone();
            let mut final_segments = qa_quality_snapshot.segments.clone();
            let qa_words = words.clone();
            let pass_quality_report = qa_quality_snapshot.report.clone();
            let pass_quality_applied_total = qa_quality_snapshot.applied_change_total;
            let style_guidance = summarize_snapshot.tone_strategy.clone();
            let app_handle = app.cloned();
            async move {
                emit_bridge_event(
                    app_handle.as_ref(),
                    "transcribe-phase",
                    &TranscribePhaseEvent {
                        task_id: task_id.clone(),
                        phase: "qa_layout".to_string(),
                        phase_detail: None,
                    },
                );
                let qa_pass1 = run_qa_simple(QaAgentRequest {
                    task_id: task_id.clone(),
                    media_path: media_path.clone(),
                    source_lang,
                    target_lang,
                    api_key: settings.translate_api_key.clone(),
                    base_url: settings.translate_base_url.clone(),
                    model: settings.translate_model.clone(),
                    llm_concurrency: settings.llm_concurrency,
                    source_max_words_per_segment: settings.subtitle_max_words_per_segment,
                    target_reference_len: settings.subtitle_length_reference,
                    terminology_entries: if settings.enable_terminology {
                        map_terminology_entries(&settings.terminology_groups)
                    } else {
                        Vec::new()
                    },
                    segments: final_segments,
                    style_guidance: style_guidance.clone(),
                    pass: "pass1_segment".to_string(),
                    prior_report: Some(pass_quality_report.clone()),
                })
                .await
                .map_err(|err| format!("qa layout failed: {err}"))?;
                final_segments = qa_pass1.segments;
                let pass_layout_report = qa_pass1.report;
                let pass_layout_applied_total = qa_pass1.applied_changes.len();
                let qa_report = json!({
                    "mode": "two_pass",
                    "passQuality": pass_quality_report,
                    "passLayout": pass_layout_report,
                });
                let qa_applied_total = pass_quality_applied_total + pass_layout_applied_total;
                let _qa_align = realign_segments_with_words(
                    &mut final_segments,
                    &qa_words
                        .iter()
                        .map(|w| QaWordTiming {
                            start: w.start,
                            end: w.end,
                            word: w.word.clone(),
                        })
                        .collect::<Vec<_>>(),
                );
                let source_srt = build_srt_from_translate_segments(&final_segments, false);
                let target_srt = build_srt_from_translate_segments(&final_segments, true);
                let src_trans_srt = build_bilingual_srt_from_translate_segments(&final_segments, true);
                let trans_src_srt = build_bilingual_srt_from_translate_segments(&final_segments, false);
                Ok(QaSnapshot {
                    segments: final_segments,
                    report: qa_report,
                    applied_change_total: qa_applied_total,
                    source_srt,
                    target_srt,
                    src_trans_srt,
                    trans_src_srt,
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
    log_pipeline_stage(
        task,
        "qa_layout",
        "completed",
        json!({
            "appliedChangeTotal": qa_snapshot.applied_change_total,
            "segmentTotal": qa_snapshot.segments.len(),
        }),
    );

    let final_segments = apply_subtitle_beautify_to_segments(
        &qa_snapshot.segments,
        settings_before_post.enable_subtitle_beautify,
    );
    let final_source_srt = build_srt_from_translate_segments(&final_segments, false);
    let final_target_srt = build_srt_from_translate_segments(&final_segments, true);
    let final_src_trans_srt = build_bilingual_srt_from_translate_segments(&final_segments, true);
    let final_trans_src_srt = build_bilingual_srt_from_translate_segments(&final_segments, false);

    save_translation_srt_set(
        &task.id,
        &task.media_path,
        &final_source_srt,
        &final_target_srt,
        &final_src_trans_srt,
        &final_trans_src_srt,
    )?;

    let merged = final_segments
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
) -> Result<DonePayload, String> {
    let tokens = parse_tokens_from_segments(&context.projections.editor.subtitle_segments_json);
    if tokens.is_empty() {
        return Err("当前任务没有可翻译内容，请先执行转录".to_string());
    }
    let qa_word_timestamps = tokens
        .iter()
        .map(|w| QaWordTiming {
            start: w.start,
            end: w.end,
            word: w.word.clone(),
        })
        .collect::<Vec<_>>();
    let settings = load_user_preferences(pool).await?.settings;
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
        STAGE_SUMMARIZE,
        load_summarize_snapshot,
        |value| {
            !value.topic_summary.trim().is_empty() && !value.tone_strategy.trim().is_empty()
        },
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
                let (topic_summary, tone_strategy) = run_translate_summarize(&request).await?;
                Ok(SummarizeSnapshot {
                    topic_summary,
                    tone_strategy,
                })
            }
        },
        |value| {
            json!({
                "topicSummary": value.topic_summary,
                "toneStrategy": value.tone_strategy,
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
            "topicSummary": summarize_snapshot.topic_summary,
            "toneStrategy": summarize_snapshot.tone_strategy,
        }),
    );
    log_pipeline_stage(task, "translate", "started", Value::Null);
    let translate_snapshot = run_stage(
        pool,
        &task.id,
        context,
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
                let translated = run_translate_with_style(
                    request,
                    summarize.topic_summary,
                    summarize.tone_strategy,
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
    log_pipeline_stage(task, "qa_quality", "started", Value::Null);
    let qa_quality_snapshot = run_stage(
        pool,
        &task.id,
        context,
        STAGE_QA_QUALITY,
        load_qa_quality_snapshot,
        |value| !value.segments.is_empty(),
        || {
            let task_id = task.id.clone();
            let media_path = task.media_path.clone();
            let source_lang = task.source_lang.clone();
            let target_lang = task.target_lang.clone();
            let settings = settings.clone();
            let final_segments = translate_snapshot.segments.clone();
            let style_guidance = summarize_snapshot.tone_strategy.clone();
            let app_handle = app.cloned();
            async move {
                emit_bridge_event(
                    app_handle.as_ref(),
                    "transcribe-phase",
                    &TranscribePhaseEvent {
                        task_id: task_id.clone(),
                        phase: "qa_quality".to_string(),
                        phase_detail: None,
                    },
                );
                let qa_pass2 = run_qa_simple(QaAgentRequest {
                    task_id,
                    media_path,
                    source_lang,
                    target_lang,
                    api_key: settings.translate_api_key.clone(),
                    base_url: settings.translate_base_url.clone(),
                    model: settings.translate_model.clone(),
                    llm_concurrency: settings.llm_concurrency,
                    source_max_words_per_segment: settings.subtitle_max_words_per_segment,
                    target_reference_len: settings.subtitle_length_reference,
                    terminology_entries: if settings.enable_terminology {
                        map_terminology_entries(&settings.terminology_groups)
                    } else {
                        Vec::new()
                    },
                    segments: final_segments,
                    style_guidance: style_guidance.clone(),
                    pass: "pass2_quality".to_string(),
                    prior_report: None,
                })
                .await
                .map_err(|err| format!("qa quality failed: {err}"))?;
                Ok(QaSnapshot {
                    segments: qa_pass2.segments,
                    report: qa_pass2.report,
                    applied_change_total: qa_pass2.applied_changes.len(),
                    source_srt: qa_pass2.source_srt,
                    target_srt: qa_pass2.target_srt,
                    src_trans_srt: qa_pass2.bilingual_srt_source_first,
                    trans_src_srt: qa_pass2.bilingual_srt_target_first,
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
    log_pipeline_stage(
        task,
        "qa_quality",
        "completed",
        json!({
            "appliedChangeTotal": qa_quality_snapshot.applied_change_total,
            "segmentTotal": qa_quality_snapshot.segments.len(),
        }),
    );
    log_pipeline_stage(task, "qa_layout", "started", Value::Null);
    let qa_snapshot = run_stage(
        pool,
        &task.id,
        context,
        STAGE_QA_LAYOUT,
        load_qa_layout_snapshot,
        |value| !value.segments.is_empty(),
        || {
            let task_id = task.id.clone();
            let media_path = task.media_path.clone();
            let source_lang = task.source_lang.clone();
            let target_lang = task.target_lang.clone();
            let settings = settings.clone();
            let qa_words = qa_word_timestamps.clone();
            let mut final_segments = qa_quality_snapshot.segments.clone();
            let pass_quality_report = qa_quality_snapshot.report.clone();
            let pass_quality_applied_total = qa_quality_snapshot.applied_change_total;
            let style_guidance = summarize_snapshot.tone_strategy.clone();
            let app_handle = app.cloned();
            async move {
                emit_bridge_event(
                    app_handle.as_ref(),
                    "transcribe-phase",
                    &TranscribePhaseEvent {
                        task_id: task_id.clone(),
                        phase: "qa_layout".to_string(),
                        phase_detail: None,
                    },
                );
                let qa_pass1 = run_qa_simple(QaAgentRequest {
                    task_id: task_id.clone(),
                    media_path: media_path.clone(),
                    source_lang,
                    target_lang,
                    api_key: settings.translate_api_key.clone(),
                    base_url: settings.translate_base_url.clone(),
                    model: settings.translate_model.clone(),
                    llm_concurrency: settings.llm_concurrency,
                    source_max_words_per_segment: settings.subtitle_max_words_per_segment,
                    target_reference_len: settings.subtitle_length_reference,
                    terminology_entries: if settings.enable_terminology {
                        map_terminology_entries(&settings.terminology_groups)
                    } else {
                        Vec::new()
                    },
                    segments: final_segments,
                    style_guidance: style_guidance.clone(),
                    pass: "pass1_segment".to_string(),
                    prior_report: Some(pass_quality_report.clone()),
                })
                .await
                .map_err(|err| format!("qa layout failed: {err}"))?;
                final_segments = qa_pass1.segments;
                let pass_layout_report = qa_pass1.report;
                let pass_layout_applied_total = qa_pass1.applied_changes.len();
                let qa_report = json!({
                    "mode": "two_pass",
                    "passQuality": pass_quality_report,
                    "passLayout": pass_layout_report,
                });
                let qa_applied_total = pass_quality_applied_total + pass_layout_applied_total;
                let _qa_align = realign_segments_with_words(&mut final_segments, &qa_words);
                let source_srt = build_srt_from_translate_segments(&final_segments, false);
                let target_srt = build_srt_from_translate_segments(&final_segments, true);
                let src_trans_srt = build_bilingual_srt_from_translate_segments(&final_segments, true);
                let trans_src_srt = build_bilingual_srt_from_translate_segments(&final_segments, false);
                Ok(QaSnapshot {
                    segments: final_segments,
                    report: qa_report,
                    applied_change_total: qa_applied_total,
                    source_srt,
                    target_srt,
                    src_trans_srt,
                    trans_src_srt,
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
    log_pipeline_stage(
        task,
        "qa_layout",
        "completed",
        json!({
            "appliedChangeTotal": qa_snapshot.applied_change_total,
            "segmentTotal": qa_snapshot.segments.len(),
        }),
    );
    let final_segments = apply_subtitle_beautify_to_segments(
        &qa_snapshot.segments,
        settings.enable_subtitle_beautify,
    );
    let final_source_srt = build_srt_from_translate_segments(&final_segments, false);
    let final_target_srt = build_srt_from_translate_segments(&final_segments, true);
    let final_src_trans_srt = build_bilingual_srt_from_translate_segments(&final_segments, true);
    let final_trans_src_srt = build_bilingual_srt_from_translate_segments(&final_segments, false);
    save_translation_srt_set(
        &task.id,
        &task.media_path,
        &final_source_srt,
        &final_target_srt,
        &final_src_trans_srt,
        &final_trans_src_srt,
    )?;

    let merged = final_segments
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
    context.set_queue_projection("processing", "translate", "", 99, merged.len() as u32, merged.len() as u32, "");
    persist_task_context(pool, &task.id, context).await?;

    Ok(DonePayload {
        result_text: if context.projections.editor.result_text.trim().is_empty() {
            format!("translated with {}", settings.translate_model)
        } else {
            context.projections.editor.result_text.clone()
        },
        result_srt: if context.projections.editor.result_srt.trim().is_empty() {
            final_source_srt
        } else {
            context.projections.editor.result_srt.clone()
        },
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
        STAGE_CORRECT => &context.stages.correct.output,
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
    let topic_summary = output.get("topicSummary")?.as_str()?.to_string();
    let tone_strategy = output.get("toneStrategy")?.as_str()?.to_string();
    if topic_summary.trim().is_empty() || tone_strategy.trim().is_empty() {
        return None;
    }
    Some(SummarizeSnapshot {
        topic_summary,
        tone_strategy,
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

fn load_qa_layout_snapshot(context: &TaskContext) -> Option<QaSnapshot> {
    load_qa_stage_snapshot(context, STAGE_QA_LAYOUT)
}

fn load_qa_quality_snapshot(context: &TaskContext) -> Option<QaSnapshot> {
    load_qa_stage_snapshot(context, STAGE_QA_QUALITY)
}

fn load_qa_stage_snapshot(context: &TaskContext, stage: &str) -> Option<QaSnapshot> {
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
    Some(QaSnapshot {
        segments,
        report: output.get("report").cloned().unwrap_or(Value::Null),
        applied_change_total,
        source_srt: output.get("sourceSrt")?.as_str()?.to_string(),
        target_srt: output.get("targetSrt")?.as_str()?.to_string(),
        src_trans_srt: output.get("srcTransSrt")?.as_str()?.to_string(),
        trans_src_srt: output.get("transSrcSrt")?.as_str()?.to_string(),
    })
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

async fn run_stage<T, FLoad, FValid, FExec, Fut, FOutput, FMetrics>(
    pool: &SqlitePool,
    task_id: &str,
    context: &mut TaskContext,
    stage: &str,
    load_existing: FLoad,
    validate: FValid,
    exec: FExec,
    output_of: FOutput,
    metrics_of: FMetrics,
) -> Result<T, String>
where
    FLoad: Fn(&TaskContext) -> Option<T>,
    FValid: Fn(&T) -> bool,
    FExec: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<T, String>>,
    FOutput: Fn(&T) -> Value,
    FMetrics: Fn(&T) -> Value,
{
    if stage_is_done(context.stage_status(stage)) {
        if let Some(existing) = load_existing(context) {
            if validate(&existing) {
                return Ok(existing);
            }
        }
    }

    context.mark_stage_running(stage);
    persist_task_context(pool, task_id, context).await?;

    let value = match exec().await {
        Ok(v) => v,
        Err(err) => {
            context.mark_failed(stage, "STAGE_FAILED", &err, true);
            persist_task_context(pool, task_id, context).await?;
            return Err(err);
        }
    };
    if !validate(&value) {
        let err = format!("{stage} failed: invalid output");
        context.mark_failed(stage, "INVALID_OUTPUT", &err, false);
        persist_task_context(pool, task_id, context).await?;
        return Err(err);
    }

    context.mark_stage_done(stage, output_of(&value), metrics_of(&value));
    persist_task_context(pool, task_id, context).await?;
    Ok(value)
}

fn done_payload_from_context(context: &TaskContext) -> DonePayload {
    let segment_total = count_segments_from_json(&context.projections.editor.subtitle_segments_json);
    DonePayload {
        result_text: context.projections.editor.result_text.clone(),
        result_srt: context.projections.editor.result_srt.clone(),
        subtitle_segments_json: context.projections.editor.subtitle_segments_json.clone(),
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

fn map_correction_terminology_entries(
    groups: &[crate::services::preferences::TerminologyGroup],
) -> Vec<CorrectionTerminologyEntry> {
    groups
        .iter()
        .flat_map(|group| {
            group.terms.iter().map(|term| CorrectionTerminologyEntry {
                source: term.origin.trim().to_string(),
                target: term.target.trim().to_string(),
                note: term.note.trim().to_string(),
            })
        })
        .filter(|entry| !entry.source.is_empty() && !entry.target.is_empty())
        .collect()
}

#[derive(Debug, sqlx::FromRow)]
struct TaskStageSnapshotRow {
    stage: String,
    status: String,
    started_at: Option<i64>,
    finished_at: Option<i64>,
    output_json: String,
    metrics_json: String,
    error_code: String,
    error_message: String,
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
    context.set_queue_projection(
        map_workspace_status(&task.overall_status),
        map_workspace_phase(&task.current_stage).as_str(),
        &task.phase_detail,
        task.progress_percent.clamp(0, 100) as u32,
        task.segment_current.max(0) as u32,
        task.segment_total.max(0) as u32,
        &task.error_message,
    );
    context.set_editor_projection(
        task.subtitle_segments_json.clone(),
        task.result_text.clone(),
        task.result_srt.clone(),
        task.translated_srt.clone(),
    );

    let rows = sqlx::query_as::<_, TaskStageSnapshotRow>(
        "SELECT stage, status, started_at, finished_at, output_json, metrics_json, error_code, error_message
         FROM task_stage_runs
         WHERE task_id = ?",
    )
    .bind(&task.id)
    .fetch_all(pool)
    .await
    .map_err(|err| err.to_string())?;

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

async fn persist_task_context(
    pool: &SqlitePool,
    task_id: &str,
    context: &TaskContext,
) -> Result<(), String> {
    let now = unix_now();
    let is_final = matches!(
        context.runtime.status.as_str(),
        "failed" | "completed" | "cancelled"
    );
    sqlx::query(
        "UPDATE task_runs
         SET overall_status = ?,
             current_stage = ?,
             progress_percent = ?,
             phase_detail = ?,
             segment_current = ?,
             segment_total = ?,
             error_message = ?,
             result_text = ?,
             result_srt = ?,
             subtitle_segments_json = ?,
             translated_srt = ?,
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
    .bind(normalize_overall_status(&context.runtime.status))
    .bind(&context.runtime.current_stage)
    .bind(context.runtime.progress_percent as i64)
    .bind(&context.projections.queue.phase_detail)
    .bind(context.projections.queue.transcribe_segment_current as i64)
    .bind(context.projections.queue.transcribe_segment_total as i64)
    .bind(&context.projections.queue.transcribe_error)
    .bind(&context.projections.editor.result_text)
    .bind(&context.projections.editor.result_srt)
    .bind(&context.projections.editor.subtitle_segments_json)
    .bind(&context.projections.editor.translated_srt)
    .bind(&context.runtime.status)
    .bind(now)
    .bind(if is_final { 1 } else { 0 })
    .bind(now)
    .bind(now)
    .bind(task_id)
    .execute(pool)
    .await
    .map_err(|err| err.to_string())?;

    for (stage, envelope) in [
        (STAGE_INIT, &context.stages.init),
        (STAGE_SEPARATE, &context.stages.separate),
        (STAGE_ASR, &context.stages.asr),
        (STAGE_PUNCTUATE, &context.stages.punctuate),
        (STAGE_CORRECT, &context.stages.correct),
        (STAGE_SEGMENT, &context.stages.segment),
        (STAGE_SUMMARIZE, &context.stages.summarize),
        (STAGE_TRANSLATE, &context.stages.translate),
        (STAGE_QA, &context.stages.qa),
        (STAGE_QA_LAYOUT, &context.stages.qa_layout),
        (STAGE_QA_QUALITY, &context.stages.qa_quality),
        (STAGE_COMPOSE, &context.stages.compose),
    ] {
        let output_json = serde_json::to_string(&envelope.output).unwrap_or_else(|_| "{}".to_string());
        let metrics_json = serde_json::to_string(&envelope.metrics).unwrap_or_else(|_| "{}".to_string());
        let error_code = envelope.error.as_ref().map(|e| e.code.clone()).unwrap_or_default();
        let error_message = envelope.error.as_ref().map(|e| e.message.clone()).unwrap_or_default();
        let duration_ms = match (envelope.started_at, envelope.finished_at) {
            (Some(start), Some(end)) if end >= start => (end - start) * 1000,
            _ => 0,
        };
        sqlx::query(
            "INSERT INTO task_stage_runs (
                task_id, stage, status, attempt, input_hash, output_json, metrics_json, error_code, error_message,
                started_at, finished_at, duration_ms, updated_at
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(task_id, stage) DO UPDATE SET
                status = excluded.status,
                attempt = CASE
                    WHEN excluded.status = 'running' THEN task_stage_runs.attempt + 1
                    ELSE task_stage_runs.attempt
                END,
                output_json = excluded.output_json,
                metrics_json = excluded.metrics_json,
                error_code = excluded.error_code,
                error_message = excluded.error_message,
                started_at = excluded.started_at,
                finished_at = excluded.finished_at,
                duration_ms = excluded.duration_ms,
                updated_at = excluded.updated_at",
        )
        .bind(task_id)
        .bind(stage)
        .bind(&envelope.status)
        .bind(if envelope.status == "running" { 1_i64 } else { 0_i64 })
        .bind("")
        .bind(output_json)
        .bind(metrics_json)
        .bind(error_code)
        .bind(error_message)
        .bind(envelope.started_at)
        .bind(envelope.finished_at)
        .bind(duration_ms)
        .bind(now)
        .execute(pool)
        .await
        .map_err(|err| err.to_string())?;
    }

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

fn normalize_overall_status(runtime_status: &str) -> &str {
    match runtime_status {
        "queued" => "queued",
        "running" => "running",
        "failed" => "failed",
        "completed" => "completed",
        _ => "pending",
    }
}

fn normalize_runtime_status(overall_status: &str) -> String {
    match overall_status {
        "queued" => "queued".to_string(),
        "running" => "running".to_string(),
        "failed" => "failed".to_string(),
        "completed" => "completed".to_string(),
        _ => "queued".to_string(),
    }
}

fn map_workspace_status(overall_status: &str) -> &str {
    match overall_status {
        "queued" => "queued",
        "running" => "processing",
        "failed" => "error",
        "completed" => "done",
        _ => "pending",
    }
}

fn map_workspace_phase(current_stage: &str) -> String {
    match current_stage {
        "separate" => "separating".to_string(),
        "asr" => "recognizing".to_string(),
        other => other.to_string(),
    }
}
