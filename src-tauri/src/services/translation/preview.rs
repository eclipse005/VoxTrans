//! Incremental mid-stream subtitle preview: turn the growing assistant text
//! into best-effort per-segment updates, throttle the UI emit, and rebuild the
//! Arc-cached snapshot. Separate concern from the translation window/ladder
//! orchestration in `mod.rs`.

use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::partial_parse;
use super::slots::{proportional_split, slot_texts_in_order};
use super::types::{BatchWindow, TranslationProgress, TranslationSegmentOutput};
use super::TranslateCall;

/// Min interval between mid-stream subtitle preview patches (per batch worker).
const STREAM_PREVIEW_MIN_INTERVAL: Duration = Duration::from_millis(100);
/// Also emit when this many new chars accumulate since last preview.
const STREAM_PREVIEW_MIN_CHARS: usize = 24;

pub(super) fn make_stream_partial<P>(
    call: &TranslateCall<P>,
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

/// Build the preview snapshot: segments already translated carry
/// their text; the rest carry only the source (translation empty).
///
/// Entries are cached in `snapshot_cache`: only segments whose translation
/// changed since the previous emit are rebuilt, so an emit clones Arc
/// pointers for untouched segments instead of their full payloads. The
/// serialized snapshot content is unchanged.
pub(super) fn rebuild_partial_outputs(
    segments: &[super::types::NormalizedSegment],
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
