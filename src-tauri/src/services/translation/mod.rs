//! Two-stage translation: whole-sentence translation + display-slot backfill.
//!
//! The model translates complete sentences (units); a deterministic layer
//! redistributes that translation onto the display cues (slots) under a
//! content-preserving accept gate. A bad answer is retried, split (8→4→2→1)
//! or salvaged at the ladder floor — never shown.
//!
//! **Contract:** see `README.md` in this directory for the unit/slot
//! definitions, the accept-gate table, the four repair rules, the ladder and
//! the invariants this module must keep. Read it before changing anything
//! here; change it first if the contract changes.
//!
//! End-to-end sample from a real 49-minute task lives in
//! `testdata/ep6_slot_units.json` and is exercised by `tests.rs`.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::json;

use crate::services::llm::batch::run_indexed_chained_idempotent;
use crate::services::llm::client::LlmClient;
use crate::services::llm::port::{LlmCallContext, LlmConfig, LlmJsonTask, next_llm_request_id};
use crate::services::task_log::TaskLogger;

/// Structured-output schema for translation batches (two-stage protocol):
/// the model must answer with sentence units, each carrying the full unit
/// translation plus the display-slot backfill `{"units":[{"id":<int>,
/// "translation":<str>,"slots":[{"id":<int>,"text":<str>}]}]}`. Endpoints
/// that reject `output_config` fall back to plain JSON + the repair chain.
fn translation_output_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "units": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "id": {"type": "integer"},
                        "translation": {"type": "string"},
                        "slots": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "id": {"type": "integer"},
                                    "text": {"type": "string"}
                                },
                                "required": ["id", "text"],
                                "additionalProperties": false
                            }
                        }
                    },
                    "required": ["id", "translation", "slots"],
                    "additionalProperties": false
                }
            }
        },
        "required": ["units"],
        "additionalProperties": false
    })
}

mod batches;
mod guard;
mod name_memory;
mod partial_parse;
mod responses;
mod segments;
mod slots;
#[cfg(test)]
mod tests;
mod text;
mod types;

use batches::{build_batch_windows, matched_term_targets, split_chunk_size, split_window};
use crate::services::transcription::join_words;
use name_memory::derive_name_memory;
use responses::{
    SlotUnitExpectation, SlotValidationContext, UnitFill,
    validate_sentence_slot_response_with_context,
};
use slots::{proportional_split, repair_slot_boundaries, slot_texts_in_order};
use types::BatchWindow;
use segments::normalize_segments;
pub use types::{
    BuildTranslationLayerRequest, TranslationProgress, TranslationSegmentInput,
    TranslationSegmentOutput, TranslationTerminologyEntry, TranslationToken,
};
pub(crate) use types::BuildTranslationLayerResponse;

/// Units per batch: whole semantic sentences (two-stage pipeline).
///
/// The pilot (ep6, 392 sentences) showed 8 keeps prompts compact while the
/// chained context carries cross-sentence continuity. This is the single
/// definition of "how many units per call": the command DTO default, the
/// workspace pipeline step and the replay examples all reference it, so no
/// caller hard-codes its own number again. A request that passes 0 also
/// resolves to this value.
pub const DEFAULT_BATCH_SIZE: usize = 8;
const MAX_BATCH_SIZE: usize = 40;
const PREV_CONTEXT_LINES: usize = 3;
const NEXT_CONTEXT_LINES: usize = 2;
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
    // re-split the failed window with the same translate prompt: windows over
    // 4 units become 4-unit windows, then 2, then 1 (see README.md §5). A
    // single-unit window is the lenient floor.
    let llm_client = LlmClient::new(LlmConfig::new(
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
        let mut map = HashMap::<usize, (usize, WindowOutcome)>::new();
        for row in rows {
            map.insert(
                row.batch_index,
                (
                    row.batch_index,
                    WindowOutcome {
                        segment_translations: row.segment_translations,
                        unit_translations: HashMap::new(),
                    },
                ),
            );
        }
        (map, Some(us.clone()))
    } else {
        (HashMap::new(), None)
    };
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

    // Unit-level translations already known before this run (resumed /
    // checkpointed batches). Each batch's prompt is built after the
    // predecessor commits so previousLines render as "source → translation"
    // at UNIT granularity. A unit counts as known only when its persisted
    // batch carries every display slot's text.
    let precomputed_segments = precomputed_translations(&precomputed);
    let known_translations: Arc<Mutex<HashMap<usize, String>>> = Arc::new(Mutex::new(
        seed_unit_translations(&windows, &precomputed_segments),
    ));

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
        output_schema: Some(translation_output_schema()),
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
        move |idx: usize, val: (usize, WindowOutcome)| {
            let store = store.clone();
            async move {
                if let Some(ref us) = store {
                    us.save_translation_batch(
                        &crate::services::pipeline::TranslationBatchRow {
                            batch_index: idx,
                            segment_translations: val.1.segment_translations,
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
                move |done: usize, total: usize, result: Option<&(usize, WindowOutcome)>| {
                    completed_batches.store(done, Ordering::Relaxed);
                    if let Some((_, outcome)) = result {
                        match partial_map.lock() {
                            Ok(mut map) => {
                                for (id, text) in &outcome.segment_translations {
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
        let (_, outcome) = item?;
        translated_by_id.extend(outcome.segment_translations);
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
    llm_client: LlmClient,
    context: LlmCallContext,
    segments: Arc<Vec<types::NormalizedSegment>>,
    known_translations: Arc<Mutex<HashMap<usize, String>>>,
    partial_map: Arc<Mutex<HashMap<usize, String>>>,
    progress_callback: Option<Arc<dyn Fn(TranslationProgress) + Send + Sync>>,
    completed_batches: Arc<AtomicUsize>,
    partial_snapshot_cache: Arc<Mutex<Vec<Option<Arc<TranslationSegmentOutput>>>>>,
    batch_total: usize,
}

/// Result of translating one window: display-slot texts keyed by global
/// segment id (persisted + previewed) and full unit translations keyed by
/// global unit id (chained context for later batches).
#[derive(Clone, Default)]
struct WindowOutcome {
    segment_translations: HashMap<usize, String>,
    unit_translations: HashMap<usize, String>,
}

impl WindowOutcome {
    fn extend(&mut self, other: WindowOutcome) {
        self.segment_translations.extend(other.segment_translations);
        self.unit_translations.extend(other.unit_translations);
    }
}

async fn translate_window_with_split(
    call: &TranslateCall,
    window: BatchWindow,
    request_id: String,
) -> Result<WindowOutcome, String> {
    match translate_window_once(call, &window, &request_id).await {
        Ok(outcome) => Ok(outcome),
        Err(err) => {
            let Some(chunk) = split_chunk_size(window.local_to_global.len()) else {
                return Err(err);
            };
            log_translate_split(&call.context, &window, chunk, &err);
            let mut combined = WindowOutcome::default();
            let mut queue: VecDeque<BatchWindow> = split_window(&window, chunk).into();
            while let Some(part) = queue.pop_front() {
                let part_id = next_llm_request_id();
                match translate_window_once(call, &part, &part_id).await {
                    Ok(outcome) => combined.extend(outcome),
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
) -> Result<WindowOutcome, String> {
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
        // Name memory works at unit granularity: one pseudo-segment per unit
        // (global unit id + full unit source sentence).
        let unit_pseudo_segments: Vec<types::NormalizedSegment> = window
            .local_to_global
            .iter()
            .zip(window.current_lines.iter())
            .map(|(global_unit, line)| types::NormalizedSegment {
                segment_id: *global_unit,
                sentence_id: 0,
                start: 0.0,
                end: 0.0,
                source: line.text.clone(),
                tokens: Vec::new(),
            })
            .collect();
        let memory = derive_name_memory(
            &unit_pseudo_segments,
            &guard,
            &current_text,
            &window.source_lang,
            &window.target_lang,
        );
        window.build_prompt(&guard, &memory.terms, &memory.examples)
    };
    // Lenient mode at the ladder floor: a single-unit window that keeps
    // failing strict validation salvages its content instead of failing the
    // whole task (slots kept as-is, or the bare translation for proportional
    // backfill).
    let lenient = window.current_lines.len() == 1;
    let unit_expectations: Vec<SlotUnitExpectation<'_>> = window
        .unit_slots
        .iter()
        .zip(window.current_lines.iter())
        .map(|(slots, line)| SlotUnitExpectation {
            slots,
            source: line.text.as_str(),
        })
        .collect();
    let source_lang = window.source_lang.clone();
    let target_lang = window.target_lang.clone();
    // Only terms whose source actually appears in this window are enforced
    // verbatim in the output; their targets are exempt from the source-script
    // leak guard.
    let enforced_targets: Vec<String> = matched_term_targets(&current_text, window.terms.as_ref());
    let local_to_global = window.local_to_global.clone();
    let on_partial = make_stream_partial(call, window);

    let result = call
        .llm_client
        .call_json_validated_streaming(
            &call.context,
            request_id,
            &prompt,
            None,
            on_partial,
            |value| {
                validate_sentence_slot_response_with_context(
                    value,
                    &SlotValidationContext {
                        units: &unit_expectations,
                        source_lang: &source_lang,
                        target_lang: &target_lang,
                        enforced_targets: &enforced_targets,
                        lenient,
                    },
                )
            },
        )
        .await
        .map_err(|err| {
            format!(
                "step4 translate batch {} ({} units) failed (llmId={}): {}",
                window.batch_id + 1,
                window.local_to_global.len(),
                request_id,
                err.message
            )
        })?;

    let mut skipped = |id: usize| {
        eprintln!(
            "[warn] translate: model returned out-of-range unit id {id} (expected 1..={}); ignoring",
            local_to_global.len()
        );
    };
    let outcome = merge_units_to_slots(window, result.value, &mut skipped);

    if let Ok(mut known) = call.known_translations.lock() {
        for (global_unit, translation) in &outcome.unit_translations {
            if !translation.trim().is_empty() {
                known.insert(*global_unit, translation.clone());
            }
        }
    }
    if let Ok(mut map) = call.partial_map.lock() {
        for (global_segment, text) in &outcome.segment_translations {
            map.insert(*global_segment, text.clone());
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
    Ok(outcome)
}

/// Map validated units onto the window's display slots.
///
/// `UnitFill::Slots` texts are used as-is after the deterministic boundary
/// repair pass; `UnitFill::TranslationOnly` (lenient floor) is distributed
/// proportionally onto the unit's slots by cue source weight. Both are
/// content-preserving: the slot texts always concatenate back to the unit's
/// translation (whitespace and punctuation aside).
fn merge_units_to_slots(
    window: &BatchWindow,
    units: Vec<responses::ValidatedUnit>,
    skipped: &mut dyn FnMut(usize),
) -> WindowOutcome {
    let mut outcome = WindowOutcome::default();
    for (index, unit) in units.into_iter().enumerate() {
        let Some(global_unit) = window.local_to_global.get(index).copied() else {
            skipped(index + 1);
            continue;
        };
        let Some(slots) = window.unit_slots.get(index) else {
            skipped(index + 1);
            continue;
        };
        let slot_texts = match unit.fill {
            UnitFill::Slots(texts) => texts,
            UnitFill::TranslationOnly(translation) => {
                let weights: Vec<usize> = slots
                    .iter()
                    .map(|(_, source)| source.chars().count())
                    .collect();
                proportional_split(&translation, &weights)
            }
        };
        // Deterministic boundary repair (leading punctuation, mid-word snaps).
        let mut repaired = slot_texts;
        repair_slot_boundaries(&mut repaired, &window.target_lang);
        if repaired.len() != slots.len() {
            // Defensive: never publish a slot-count mismatch.
            skipped(global_unit);
            continue;
        }
        for ((segment_id, _), text) in slots.iter().zip(repaired) {
            if !text.trim().is_empty() {
                outcome
                    .segment_translations
                    .insert(*segment_id, text);
            }
        }
        outcome
            .unit_translations
            .insert(global_unit, unit.translation);
    }
    outcome
}

fn make_stream_partial(
    call: &TranslateCall,
    window: &BatchWindow,
) -> Arc<dyn Fn(String) + Send + Sync> {
    let partial_map = call.partial_map.clone();
    let progress_callback = call.progress_callback.clone();
    let segments = call.segments.clone();
    let completed_batches = call.completed_batches.clone();
    let partial_snapshot_cache = call.partial_snapshot_cache.clone();
    let batch_total = call.batch_total;
    let unit_slots = window.unit_slots.clone();
    let local_to_global = window.local_to_global.clone();
    let throttle = Arc::new(Mutex::new(StreamThrottle::new()));
    Arc::new(move |raw: String| {
        // Best-effort (segment_id, text) updates from the growing stream.
        let mut updates: Vec<(usize, String)> = Vec::new();
        let units = partial_parse::extract_partial_units(&raw);
        if !units.is_empty() {
            for unit in units {
                let Some(idx) = unit.id.checked_sub(1) else {
                    continue;
                };
                if idx >= local_to_global.len() {
                    continue;
                }
                let Some(slots) = unit_slots.get(idx) else {
                    continue;
                };
                if slots.is_empty() {
                    continue;
                }
                if let Some(texts) = slot_texts_in_order(slots, &unit.slots) {
                    for ((id, _), text) in slots.iter().zip(texts) {
                        if !text.trim().is_empty() {
                            updates.push((*id, text));
                        }
                    }
                    continue;
                }
                // Slots not complete yet: preview the unit translation
                // distributed proportionally onto the display slots.
                if let Some(translation) = unit.translation.as_ref() {
                    if translation.trim().is_empty() {
                        continue;
                    }
                    let weights: Vec<usize> =
                        slots.iter().map(|(_, source)| source.chars().count()).collect();
                    for ((id, _), text) in
                        slots.iter().zip(proportional_split(translation, &weights))
                    {
                        if !text.trim().is_empty() {
                            updates.push((*id, text));
                        }
                    }
                }
            }
        } else {
            // Fallback: the model ignored the units schema and streamed the
            // flat "translations" form — ids are unit ids; distribute each
            // proportionally so the preview still shows plausible slot texts.
            for (unit_id, text) in partial_parse::extract_partial_translations(&raw) {
                let Some(idx) = unit_id.checked_sub(1) else {
                    continue;
                };
                if idx >= local_to_global.len() {
                    continue;
                }
                let Some(slots) = unit_slots.get(idx) else {
                    continue;
                };
                if slots.is_empty() || text.trim().is_empty() {
                    continue;
                }
                let weights: Vec<usize> =
                    slots.iter().map(|(_, source)| source.chars().count()).collect();
                for ((id, _), slot_text) in
                    slots.iter().zip(proportional_split(&text, &weights))
                {
                    if !slot_text.trim().is_empty() {
                        updates.push((*id, slot_text));
                    }
                }
            }
        }
        if updates.is_empty() {
            return;
        }
        let mut changed = false;
        if let Ok(mut map) = partial_map.lock() {
            for (global_id, text) in updates {
                // Per-token-block callback: previews self-correct as the
                // stream grows; keep the longest seen text per segment.
                let entry = map.entry(global_id).or_default();
                if (text.len() >= entry.len() || entry.is_empty()) && text != *entry {
                    *entry = text;
                    changed = true;
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
    let parts = if chunk == 0 { 0 } else { n.div_ceil(chunk) };
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
    precomputed: &HashMap<usize, (usize, WindowOutcome)>,
) -> HashMap<usize, String> {
    let mut out = HashMap::new();
    for (_, outcome) in precomputed.values() {
        for (id, text) in &outcome.segment_translations {
            out.insert(*id, text.clone());
        }
    }
    out
}

/// Seed the unit-keyed known-translations map (chained context) from the
/// persisted segment-level slot texts: a unit counts as known only when its
/// persisted batch carries every display slot's text; the unit translation
/// is the slot texts re-joined with language-aware spacing.
fn seed_unit_translations(
    windows: &[BatchWindow],
    precomputed_segments: &HashMap<usize, String>,
) -> HashMap<usize, String> {
    let mut out = HashMap::new();
    for window in windows {
        for (local_index, slots) in window.unit_slots.iter().enumerate() {
            if slots.is_empty() {
                continue;
            }
            let all_known = slots
                .iter()
                .all(|(id, _)| precomputed_segments.contains_key(id));
            if !all_known {
                continue;
            }
            let joined = join_words(
                slots
                    .iter()
                    .filter_map(|(id, _)| precomputed_segments.get(id).map(String::as_str)),
            );
            if joined.trim().is_empty() {
                continue;
            }
            out.insert(window.local_to_global[local_index], joined);
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
