use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::services::llm::batch::run_indexed_concurrent_idempotent;
use crate::services::llm::client::OpenAiCompatLlmClient;
use crate::services::llm::port::{LlmCallContext, LlmConfig, LlmJsonTask, next_llm_request_id};
use crate::services::task_log::TaskLogger;

mod batches;
mod partial_parse;
mod responses;
mod segments;
#[cfg(test)]
mod tests;
mod text;
mod types;

use batches::{batch_index_ranges, build_batch_windows};
use responses::validate_batch_translation_response;
use segments::normalize_segments;
pub use types::{
    BuildTranslationLayerRequest, BuildTranslationLayerResponse, TranslationProgress,
    TranslationSegmentInput, TranslationSegmentOutput, TranslationTerminologyEntry,
    TranslationToken,
};

const DEFAULT_BATCH_SIZE: usize = 20;
const MAX_BATCH_SIZE: usize = 40;
const CONTEXT_LINE_LIMIT: usize = 6;
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

    let llm_client = OpenAiCompatLlmClient::new(LlmConfig::new(
        request.translate_base_url.clone(),
        request.translate_api_key.clone(),
        request.translate_model.clone(),
    ))
    ?;

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

    // Vision assist: when enabled, sample video frames per batch's time range
    // and attach as base64 data URLs. Frames are cached on disk keyed by
    // timestamp+media-identity so resume is free and source replacement
    // invalidates safely. When disabled, all batches get empty frames and
    // visual_context is None — prompt is byte-equal to pre-vision.
    let (visual_context, frames_per_batch) = if request.enable_vision_assist {
        // Compute batch time ranges mirroring build_batch_windows' slicing.
        let batch_time_ranges: Vec<(f64, f64)> = batch_index_ranges(&normalized_segments, batch_size)
            .into_iter()
            .map(|(start, end)| (normalized_segments[start].start, normalized_segments[end - 1].end))
            .collect();
        // Skip batches whose translations are already persisted (resume).
        let skip_batch_indices: HashSet<usize> = precomputed.keys().copied().collect();
        let media_path = request.media_path.clone();
        let task_id = request.task_id.clone();
        let extract_result = tokio::task::spawn_blocking(move || {
            crate::services::frame_extract::extract_frames_for_batches(
                &media_path,
                &task_id,
                &batch_time_ranges,
                &skip_batch_indices,
            )
        })
        .await
        .map_err(|e| format!("frame extraction task failed: {e}"))?;
        match extract_result {
            Ok(frames) => (
                Some(
                    "Sampled video frames are attached as auxiliary evidence for this batch.",
                ),
                frames,
            ),
            Err(err) => {
                // Frame extraction is best-effort: never block translation on it.
                // Log to the task's llm.log so the failure is observable, then
                // fall back to text-only (no visual_context, empty frames).
                let logger = crate::services::task_log::TaskLogger::llm_with_media(
                    request.task_id.clone(),
                    request.media_path.clone(),
                );
                logger.event(
                    crate::services::task_log::event::VISION_BATCH_FAILED,
                    Some(&serde_json::json!({ "error": err })),
                );
                (None, Vec::new())
            }
        }
    } else {
        (None, Vec::new())
    };

    let windows = build_batch_windows(
        &normalized_segments,
        batch_size,
        &request.source_lang,
        &request.target_lang,
        &request.theme_summary,
        &request.terminology_entries,
        visual_context,
        &frames_per_batch,
    );
    if windows.is_empty() {
        return Err("failed to build translation batches".to_string());
    }

    let concurrency = request.llm_concurrency.max(1) as usize;
    let tasks = windows
        .iter()
        .map(|window| LlmJsonTask {
            id: window.batch_id,
            request_id: next_llm_request_id(),
            user_prompt: window.prompt.clone(),
            images: window.frames.clone(),
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

    let batch_total_for_stream = windows.len();
    let completed_batches = Arc::new(AtomicUsize::new(precomputed.len()));
    let results = run_indexed_concurrent_idempotent(
        tasks,
        concurrency,
        {
            let llm_client = llm_client.clone();
            let context = context.clone();
            let partial_map_for_stream = partial_map.clone();
            let progress_for_stream = progress_callback.clone();
            let segments_for_stream = normalized_for_progress.clone();
            let completed_batches = completed_batches.clone();
            let partial_snapshot_cache_for_stream = partial_snapshot_cache.clone();
            move |task| {
                let llm_client = llm_client.clone();
                let context = context.clone();
                let windows = windows_for_worker.clone();
                let partial_map = partial_map_for_stream.clone();
                let progress_callback = progress_for_stream.clone();
                let segments = segments_for_stream.clone();
                let completed_batches = completed_batches.clone();
                let partial_snapshot_cache = partial_snapshot_cache_for_stream.clone();
                async move {
                    let Some(window) = windows.get(task.id) else {
                        return Err(format!("missing batch window for index {}", task.id));
                    };
                    let llm_id = task.request_id.clone();

                    // When vision assist is on, record which frame files this
                    // batch's translation is using, so llm.log can tie a
                    // translation result back to the exact sampled frames.
                    if !window.frame_names.is_empty()
                        && let Some(media_path) = context.media_path.as_deref()
                    {
                        let logger = TaskLogger::llm_with_media(
                            context.task_id.clone(),
                            media_path.to_string(),
                        );
                        logger.event(
                            crate::services::task_log::event::VISION_BATCH_FRAMES,
                            Some(&serde_json::json!({
                                "batchId": window.batch_id + 1,
                                "llmId": llm_id,
                                "frames": window.frame_names.as_ref(),
                            })),
                        );
                    }

                    let local_to_global = window.local_to_global.clone();
                    let local_ids = window.local_ids.clone();
                    let throttle = Arc::new(Mutex::new(StreamThrottle::new()));
                    let on_partial: Arc<dyn Fn(String) + Send + Sync> = Arc::new({
                        let partial_map = partial_map.clone();
                        let progress_callback = progress_callback.clone();
                        let segments = segments.clone();
                        let throttle = throttle.clone();
                        let local_to_global = local_to_global.clone();
                        let completed_batches = completed_batches.clone();
                        let partial_snapshot_cache = partial_snapshot_cache.clone();
                        move |raw: String| {
                            // Always merge into partial_map so concurrent batch
                            // joins see the latest streamed text. Throttle only
                            // the expensive rebuild + UI progress callback.
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
                                    let idx = local_id.saturating_sub(1);
                                    let Some(global_id) = local_to_global.get(idx).copied() else {
                                        continue;
                                    };
                                    let entry = map.entry(global_id).or_default();
                                    // Keep longer (growing) text; allow replacement if model rewrote.
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
                                let partial_outputs = rebuild_partial_outputs(
                                    &segments,
                                    &partial_map,
                                    &partial_snapshot_cache,
                                );
                                let done = completed_batches.load(Ordering::Relaxed);
                                callback(TranslationProgress {
                                    done,
                                    total: batch_total_for_stream,
                                    partial_outputs,
                                });
                            }
                        }
                    });

                    let call = llm_client
                        .call_json_validated_streaming(
                            &context,
                            &llm_id,
                            &task.user_prompt,
                            Some(task.images.as_ref()),
                            task.response_validator.as_ref(),
                            on_partial,
                            |value| validate_batch_translation_response(value, &local_ids),
                        )
                        .await
                        .map_err(|err| {
                            format!(
                                "step4 translate batch {} failed (llmId={}): {}",
                                window.batch_id + 1,
                                llm_id,
                                err.message
                            )
                        })?;
                    let mut translated_map = HashMap::<usize, String>::new();
                    for (local_id, translated) in call.value {
                        let idx = local_id.saturating_sub(1);
                        let Some(global_id) = local_to_global.get(idx).copied() else {
                            continue;
                        };
                        translated_map.insert(global_id, translated);
                    }
                    Ok((window.batch_id, translated_map))
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
