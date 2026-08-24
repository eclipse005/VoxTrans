use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::json;

use crate::services::llm::batch::run_indexed_chained_idempotent;
use crate::services::llm::client::OpenAiCompatLlmClient;
use crate::services::llm::port::{LlmCallContext, LlmConfig, LlmJsonTask, next_llm_request_id};
use crate::services::task_log::TaskLogger;

mod batches;
mod guard;
mod name_memory;
mod partial_parse;
mod responses;
mod segments;
#[cfg(test)]
mod tests;
mod text;
mod types;

use batches::{build_batch_windows, matched_term_targets, split_chunk_size, split_window};
use name_memory::derive_name_memory;
use responses::{
    TranslationValidationContext, validate_batch_translation_response_with_context,
};
use types::BatchWindow;
use segments::normalize_segments;
pub use types::{
    BuildTranslationLayerRequest, BuildTranslationLayerResponse, TranslationProgress,
    TranslationSegmentInput, TranslationSegmentOutput, TranslationTerminologyEntry,
    TranslationToken,
};

const DEFAULT_BATCH_SIZE: usize = 20;
const MAX_BATCH_SIZE: usize = 40;
const PREV_CONTEXT_LINES: usize = 3;
const NEXT_CONTEXT_LINES: usize = 2;
const MAX_TERMS_PER_BATCH: usize = 16;
/// Min interval between mid-stream subtitle preview patches (per batch worker).
const STREAM_PREVIEW_MIN_INTERVAL: Duration = Duration::from_millis(100);
/// Also emit when this many new chars accumulate since last preview.
const STREAM_PREVIEW_MIN_CHARS: usize = 24;

pub async fn build_translation_layer_with_progress(
    request: BuildTranslationLayerRequest,
    on_progress: Option<Arc<dyn Fn(TranslationProgress) + Send + Sync>>,
) -> Result<BuildTranslationLayerResponse, String> {
    validate_request(&request)?;

    let normalized_segments = normalize_segments(&request.segments);
    if normalized_segments.is_empty() {
        return Err("segments contain no translatable text".to_string());
    }

    // HTTP retries live on the client. JSON/semantic failures return here and
    // split currentLines (20→10→5→1) with the same translate prompt.
    let llm_client = OpenAiCompatLlmClient::new(LlmConfig::new(
        request.translate_base_url.clone(),
        request.translate_api_key.clone(),
        request.translate_model.clone(),
    ))?;

    let batch_size = if request.batch_size == 0 {
        DEFAULT_BATCH_SIZE
    } else {
        request.batch_size.clamp(1, MAX_BATCH_SIZE)
    };

    // Build precomputed map from domain table if unit store available.
    // Computed early so frame extraction can skip already-translated batches
    // (resume case) and avoid redundant ffmpeg work.
    let (precomputed, persist_store) = if let Some(ref us) = request.unit_store {
        // Propagate DB errors instead of treating them as "no rows". A
        // transient failure (locked DB, disk full) would otherwise cause a
        // silent full re-translation that clobbers prior persisted work.
        let rows = us.load_translation_batches().await?;
        let mut map = HashMap::<usize, (usize, HashMap<usize, String>)>::new();
        for row in rows {
            map.insert(row.batch_index, (row.batch_index, row.segment_translations));
        }
        (map, Some(us.clone()))
    } else {
        (HashMap::new(), None)
    };
    // Translations already known before this run (resumed/checkpointed
    // batches). Each batch's prompt is built after the predecessor commits
    // so previousLines render as "source → translation".
    let known_translations: Arc<Mutex<HashMap<usize, String>>> =
        Arc::new(Mutex::new(precomputed_translations(&precomputed)));

    let windows = build_batch_windows(
        &normalized_segments,
        batch_size,
        &request.source_lang,
        &request.target_lang,
        &request.style_guide,
        &request.terminology_entries,
    );
    if windows.is_empty() {
        return Err("failed to build translation batches".to_string());
    }

    let tasks = windows
        .iter()
        .map(|window| LlmJsonTask {
            id: window.batch_id,
            request_id: next_llm_request_id(),
            user_prompt: Arc::from(""), // built lazily in the worker against live known translations
            response_validator: None,
        })
        .collect::<Vec<_>>();

    let context = LlmCallContext {
        task_id: request.task_id.clone(),
        media_path: Some(request.media_path.clone()),
        phase: "step4_translate_batch".to_string(),
        store: request.unit_store.as_ref().map(|us| us.store().clone()),
    };

    // Share the batch windows across worker tasks via Arc instead of
    // deep-cloning the whole Vec (prompts + frame payloads) per task.
    let windows = Arc::new(windows);
    let windows_for_worker = windows.clone();
    let known_translations_for_worker = known_translations.clone();
    let progress_callback = on_progress.clone();

    // Cumulative segment_id -> translation map, seeded from precomputed
    // (cached/resumed) batches so partial previews reflect prior work.
    // Written only from the serial join-loop progress closure, so contention
    // is nil; Mutex is required only to satisfy the `Fn + Send + Sync` bound
    // on the shared closure.
    let partial_map: Arc<Mutex<HashMap<usize, String>>> =
        Arc::new(Mutex::new(precomputed_translations(&precomputed)));
    // Incremental snapshot cache for progress emits: entries are rebuilt
    // only when a segment's translation actually changed; emits clone Arc
    // pointers instead of every source/tokens payload each time.
    let partial_snapshot_cache: Arc<Mutex<Vec<Option<Arc<TranslationSegmentOutput>>>>> =
        Arc::new(Mutex::new(Vec::new()));
    let normalized_for_progress = Arc::new(normalized_segments.clone());

    let on_item_done = {
        let store = persist_store.clone();
        move |idx: usize, val: (usize, HashMap<usize, String>)| {
            let store = store.clone();
            async move {
                if let Some(ref us) = store {
                    us.save_translation_batch(
                        &crate::services::pipeline::TranslationBatchRow {
                            batch_index: idx,
                            segment_translations: val.1,
                        },
                    )
                    .await?;
                }
                Ok(())
            }
        }
    };

    let completed_batches = Arc::new(AtomicUsize::new(precomputed.len()));
    let translate_call = TranslateCall {
        llm_client,
        context: context.clone(),
        segments: normalized_for_progress.clone(),
        known_translations: known_translations_for_worker.clone(),
        partial_map: partial_map.clone(),
        progress_callback: progress_callback.clone(),
        completed_batches: completed_batches.clone(),
        partial_snapshot_cache: partial_snapshot_cache.clone(),
        batch_total: windows.len(),
    };
    // Batches run in index order: batch N's prompt is built after batch N-1
    // has published translations, so previousLines are actually bilingual.
    let results = run_indexed_chained_idempotent(
        tasks,
        {
            move |task| {
                let call = translate_call.clone();
                let windows = windows_for_worker.clone();
                async move {
                    let Some(window) = windows.get(task.id).cloned() else {
                        return Err(format!("missing batch window for index {}", task.id));
                    };
                    let batch_id = window.batch_id;
                    let translated_map =
                        translate_window_with_split(&call, window, task.request_id).await?;
                    Ok((batch_id, translated_map))
                }
            }
        },
        |msg| msg,
        {
            let partial_map = partial_map.clone();
            let segments = normalized_for_progress.clone();
            let progress_callback = progress_callback.clone();
            let partial_snapshot_cache = partial_snapshot_cache.clone();
            {
                let completed_batches = completed_batches.clone();
                move |done: usize, total: usize, result: Option<&(usize, HashMap<usize, String>)>| {
                    completed_batches.store(done, Ordering::Relaxed);
                    if let Some((_, translations)) = result {
                        match partial_map.lock() {
                            Ok(mut map) => {
                                for (id, text) in translations {
                                    map.insert(*id, text.clone());
                                }
                            }
                            Err(err) => {
                                // Lock poisoned = another worker panicked mid-update.
                                // Surfacing this as a warning rather than silently
                                // dropping progress updates. The poisoned lock will
                                // also fail the main-thread read below, which will
                                // propagate the error properly.
                                eprintln!(
                                    "[warn] translation partial_map lock poisoned: {err}"
                                );
                            }
                        }
                    }
                    if let Some(callback) = progress_callback.as_ref() {
                        let partial_outputs =
                            rebuild_partial_outputs(&segments, &partial_map, &partial_snapshot_cache);
                        callback(TranslationProgress {
                            done,
                            total,
                            partial_outputs,
                        });
                    }
                }
            }
        },
        precomputed,
        on_item_done,
    )
    .await;

    let mut translated_by_id = HashMap::<usize, String>::new();
    for (_, item) in results {
        let (_, translated_map) = item?;
        for (id, translated) in translated_map {
            translated_by_id.insert(id, translated);
        }
    }

    let mut outputs = Vec::<TranslationSegmentOutput>::new();
    for segment in &normalized_segments {
        let translated = translated_by_id
            .remove(&segment.segment_id)
            .unwrap_or_default();
        outputs.push(TranslationSegmentOutput {
            segment_id: segment.segment_id,
            start: segment.start,
            end: segment.end,
            source: segment.source.clone(),
            translation: translated,
            tokens: segment.tokens.clone(),
        });
    }

    let incomplete_ids = outputs
        .iter()
        .filter(|segment| segment.translation.trim().is_empty())
        .map(|segment| segment.segment_id)
        .collect::<Vec<_>>();
    if !incomplete_ids.is_empty() {
        return Err(format!(
            "translation incomplete: missing non-empty translations for segment ids {:?}",
            incomplete_ids
        ));
    }

    Ok(BuildTranslationLayerResponse {
        batch_size,
        batch_total: windows.len(),
        segment_total: outputs.len(),
        segments: outputs,
    })
}

#[derive(Clone)]
struct TranslateCall {
    llm_client: OpenAiCompatLlmClient,
    context: LlmCallContext,
    segments: Arc<Vec<types::NormalizedSegment>>,
    known_translations: Arc<Mutex<HashMap<usize, String>>>,
    partial_map: Arc<Mutex<HashMap<usize, String>>>,
    progress_callback: Option<Arc<dyn Fn(TranslationProgress) + Send + Sync>>,
    completed_batches: Arc<AtomicUsize>,
    partial_snapshot_cache: Arc<Mutex<Vec<Option<Arc<TranslationSegmentOutput>>>>>,
    batch_total: usize,
}

async fn translate_window_with_split(
    call: &TranslateCall,
    window: BatchWindow,
    request_id: String,
) -> Result<HashMap<usize, String>, String> {
    match translate_window_once(call, &window, &request_id).await {
        Ok(map) => return Ok(map),
        Err(err) => {
            let Some(chunk) = split_chunk_size(window.local_to_global.len()) else {
                return Err(err);
            };
            log_translate_split(&call.context, &window, chunk, &err);
            let mut combined = HashMap::new();
            let mut queue: VecDeque<BatchWindow> = split_window(&window, chunk).into();
            while let Some(part) = queue.pop_front() {
                let part_id = next_llm_request_id();
                match translate_window_once(call, &part, &part_id).await {
                    Ok(map) => combined.extend(map),
                    Err(part_err) => {
                        let Some(next_chunk) = split_chunk_size(part.local_to_global.len()) else {
                            return Err(part_err);
                        };
                        log_translate_split(&call.context, &part, next_chunk, &part_err);
                        let splits = split_window(&part, next_chunk);
                        for (index, split) in splits.into_iter().enumerate() {
                            queue.insert(index, split);
                        }
                    }
                }
            }
            Ok(combined)
        }
    }
}

async fn translate_window_once(
    call: &TranslateCall,
    window: &BatchWindow,
    request_id: &str,
) -> Result<HashMap<usize, String>, String> {
    let current_text = window
        .current_lines
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let prompt = {
        let guard = call
            .known_translations
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let memory = derive_name_memory(
            call.segments.as_slice(),
            &guard,
            &current_text,
            &window.source_lang,
            &window.target_lang,
        );
        window.build_prompt(&guard, &memory.terms, &memory.examples)
    };
    let local_to_global = window.local_to_global.clone();
    let local_ids = window.local_ids.clone();
    let source_lang = window.source_lang.clone();
    let target_lang = window.target_lang.clone();
    // Only terms whose source actually appears in this window are enforced
    // verbatim in the output; backfilled terms are context for the LLM and
    // must not blanket-exempt lines from the source-script leak guard.
    let enforced_targets: Vec<String> = matched_term_targets(&current_text, window.terms.as_ref());
    let on_partial = make_stream_partial(call, local_to_global.clone());

    let result = call
        .llm_client
        .call_json_validated_streaming(
            &call.context,
            request_id,
            &prompt,
            None,
            on_partial,
            |value| {
                validate_batch_translation_response_with_context(
                    value,
                    &TranslationValidationContext {
                        expected_ids: &local_ids,
                        source_lang: &source_lang,
                        target_lang: &target_lang,
                        enforced_targets: &enforced_targets,
                    },
                )
            },
        )
        .await
        .map_err(|err| {
            format!(
                "step4 translate batch {} ({} lines) failed (llmId={}): {}",
                window.batch_id + 1,
                window.local_to_global.len(),
                request_id,
                err.message
            )
        })?;

    let mut skipped = |id: usize| {
        eprintln!(
            "[warn] translate: model returned out-of-range id {id} (expected 1..={}); ignoring",
            local_to_global.len()
        );
    };
    let translated_map = merge_local_to_global(&local_to_global, result.value, &mut skipped);

    if let Ok(mut known) = call.known_translations.lock() {
        for (global_id, text) in &translated_map {
            if !text.trim().is_empty() {
                known.insert(*global_id, text.clone());
            }
        }
    }
    if let Ok(mut map) = call.partial_map.lock() {
        for (global_id, text) in &translated_map {
            map.insert(*global_id, text.clone());
        }
    }
    if let Some(callback) = call.progress_callback.as_ref() {
        let partial_outputs = rebuild_partial_outputs(
            call.segments.as_slice(),
            &call.partial_map,
            &call.partial_snapshot_cache,
        );
        callback(TranslationProgress {
            done: call.completed_batches.load(Ordering::Relaxed),
            total: call.batch_total,
            partial_outputs,
        });
    }
    Ok(translated_map)
}

/// Map model-local ids (1-based, per currentLines) to global segment ids.
/// Ids outside `1..=local_to_global.len()` — a model emitting `0`, or an id
/// past the window — previously aliased the first/last line via saturating
/// arithmetic; now they are reported through `skipped` and dropped.
fn merge_local_to_global(
    local_to_global: &[usize],
    pairs: impl IntoIterator<Item = (usize, String)>,
    skipped: &mut dyn FnMut(usize),
) -> HashMap<usize, String> {
    let mut out = HashMap::<usize, String>::new();
    for (local_id, translated) in pairs {
        let Some(idx) = local_id.checked_sub(1) else {
            skipped(local_id);
            continue;
        };
        let Some(global_id) = local_to_global.get(idx).copied() else {
            skipped(local_id);
            continue;
        };
        out.insert(global_id, translated);
    }
    out
}

fn make_stream_partial(
    call: &TranslateCall,
    local_to_global: Vec<usize>,
) -> Arc<dyn Fn(String) + Send + Sync> {
    let partial_map = call.partial_map.clone();
    let progress_callback = call.progress_callback.clone();
    let segments = call.segments.clone();
    let completed_batches = call.completed_batches.clone();
    let partial_snapshot_cache = call.partial_snapshot_cache.clone();
    let batch_total = call.batch_total;
    let throttle = Arc::new(Mutex::new(StreamThrottle::new()));
    Arc::new(move |raw: String| {
        let extracted = partial_parse::extract_partial_translations(&raw);
        if extracted.is_empty() {
            return;
        }
        let mut changed = false;
        if let Ok(mut map) = partial_map.lock() {
            for (local_id, text) in extracted {
                if text.is_empty() {
                    continue;
                }
                // Per-token-block callback: silently drop out-of-range ids
                // (the next block re-sends the full line, so previews
                // self-correct).
                let Some(idx) = local_id.checked_sub(1) else {
                    continue;
                };
                let Some(global_id) = local_to_global.get(idx).copied() else {
                    continue;
                };
                let entry = map.entry(global_id).or_default();
                if text.len() >= entry.len() || entry.is_empty() {
                    if text != *entry {
                        *entry = text;
                        changed = true;
                    }
                }
            }
        } else {
            return;
        }
        if !changed {
            return;
        }
        let should_emit = match throttle.lock() {
            Ok(mut t) => t.should_emit(raw.len()),
            Err(_) => true,
        };
        if !should_emit {
            return;
        }
        if let Some(callback) = progress_callback.as_ref() {
            let partial_outputs =
                rebuild_partial_outputs(&segments, &partial_map, &partial_snapshot_cache);
            let done = completed_batches.load(Ordering::Relaxed);
            callback(TranslationProgress {
                done,
                total: batch_total,
                partial_outputs,
            });
        }
    })
}

fn log_translate_split(
    context: &LlmCallContext,
    window: &BatchWindow,
    chunk: usize,
    reason: &str,
) {
    let n = window.local_to_global.len();
    let parts = if chunk == 0 { 0 } else { (n + chunk - 1) / chunk };
    let logger = match context.media_path.as_deref() {
        Some(path) if !path.trim().is_empty() => {
            TaskLogger::llm_with_media(context.task_id.clone(), path)
        }
        _ => TaskLogger::llm(context.task_id.clone()),
    };
    logger.event(
        "translate.split",
        Some(&json!({
            "batchId": window.batch_id + 1,
            "fromLines": n,
            "chunk": chunk,
            "parts": parts,
            "reason": reason,
        })),
    );
}

struct StreamThrottle {
    last_emit: Instant,
    last_len: usize,
}

impl StreamThrottle {
    fn new() -> Self {
        Self {
            last_emit: Instant::now()
                .checked_sub(STREAM_PREVIEW_MIN_INTERVAL)
                .unwrap_or_else(Instant::now),
            last_len: 0,
        }
    }

    /// Shared gate for `should_emit` only. Never use this to skip merging into
    /// `partial_map` — concurrent batches need map freshness even when UI emit
    /// is throttled.
    fn would_emit(&self, acc_len: usize) -> bool {
        self.last_emit.elapsed() >= STREAM_PREVIEW_MIN_INTERVAL
            || acc_len.saturating_sub(self.last_len) >= STREAM_PREVIEW_MIN_CHARS
    }

    fn should_emit(&mut self, acc_len: usize) -> bool {
        if self.would_emit(acc_len) {
            self.last_emit = Instant::now();
            self.last_len = acc_len;
            true
        } else {
            false
        }
    }
}

/// Flatten precomputed (cached/resumed) batch results into a single
/// segment_id -> translation map so the partial preview starts from the
/// already-translated segments instead of empty.
fn precomputed_translations(
    precomputed: &HashMap<usize, (usize, HashMap<usize, String>)>,
) -> HashMap<usize, String> {
    let mut out = HashMap::new();
    for (_, translations) in precomputed.values() {
        for (id, text) in translations {
            out.insert(*id, text.clone());
        }
    }
    out
}

/// Rebuild a full segment snapshot from the normalized inputs plus the
/// cumulative translations collected so far. Translated segments carry
/// their text; the rest carry only the source (translation empty).
///
/// Entries are cached in `snapshot_cache`: only segments whose translation
/// changed since the previous emit are rebuilt, so an emit clones Arc
/// pointers for untouched segments instead of their full payloads. The
/// serialized snapshot content is unchanged.
fn rebuild_partial_outputs(
    segments: &[types::NormalizedSegment],
    partial_map: &Arc<Mutex<HashMap<usize, String>>>,
    snapshot_cache: &Arc<Mutex<Vec<Option<Arc<TranslationSegmentOutput>>>>>,
) -> Vec<Arc<TranslationSegmentOutput>> {
    let map = match partial_map.lock() {
        Ok(map) => map,
        Err(_) => return Vec::new(),
    };
    let mut cache = match snapshot_cache.lock() {
        Ok(cache) => cache,
        Err(_) => return Vec::new(),
    };
    if cache.len() != segments.len() {
        cache.clear();
        cache.resize_with(segments.len(), || None);
    }
    let mut out = Vec::with_capacity(segments.len());
    for (index, segment) in segments.iter().enumerate() {
        let translation = map
            .get(&segment.segment_id)
            .cloned()
            .unwrap_or_default();
        let stale = match cache[index].as_ref() {
            Some(existing) => existing.translation != translation,
            None => true,
        };
        if stale {
            cache[index] = Some(Arc::new(TranslationSegmentOutput {
                segment_id: segment.segment_id,
                start: segment.start,
                end: segment.end,
                source: segment.source.clone(),
                translation,
                tokens: segment.tokens.clone(),
            }));
        }
        let Some(entry) = cache[index].as_ref() else {
            continue;
        };
        out.push(entry.clone());
    }
    out
}

fn validate_request(request: &BuildTranslationLayerRequest) -> Result<(), String> {
    if request.task_id.trim().is_empty() {
        return Err("taskId is required".to_string());
    }
    if request.media_path.trim().is_empty() {
        return Err("mediaPath is required".to_string());
    }
    if request.source_lang.trim().is_empty() {
        return Err("sourceLang is required".to_string());
    }
    if request.target_lang.trim().is_empty() {
        return Err("targetLang is required".to_string());
    }
    if request.segments.is_empty() {
        return Err("segments is required".to_string());
    }
    if request.translate_api_key.trim().is_empty() {
        return Err("translateApiKey is required".to_string());
    }
    if request.translate_base_url.trim().is_empty() {
        return Err("translateBaseUrl is required".to_string());
    }
    if request.translate_model.trim().is_empty() {
        return Err("translateModel is required".to_string());
    }
    Ok(())
}
