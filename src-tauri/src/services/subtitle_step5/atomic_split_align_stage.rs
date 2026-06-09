use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::Semaphore;
use tokio::task::JoinSet;

use crate::services::llm::client::OpenAiCompatLlmClient;
use crate::services::llm::port::{LlmCallContext, LlmConfig, next_llm_request_id};
use crate::services::prompts::subtitle_step5::{
    Step5PromptLine, Step5PromptTerm, build_align_prompt, build_source_split_prompt,
};

use super::alignment_repair::repair_aligned_lines;
use super::alignment_score::choose_better_alignment;
use super::constants::{MAX_TERMS_PER_LINE, OBVIOUS_OVERLONG_RATIO};
use super::language_units::text_length_units;
use super::responses::{validate_align_response, validate_source_split_response};
use super::source_split_boundaries::{map_source_parts_to_boundaries, normalize_split_boundaries};
use super::source_text::build_source_from_tokens;
use super::split_parts::boundary_ids_to_ranges;
use super::terminology_filter::select_terms_for_text;
use super::text_utils::normalize_inline_text;
use super::translation_split::heuristic_split_translation;
use super::types::{
    BuildStep5SplitAlignRequest, BuildStep5SplitAlignResponse, Step5AlignedParent,
    Step5AlignedPart, Step5DraftSegment, Step5SplitParent, Step5SplitPart,
};

const MAX_REFINE_ROUNDS: usize = 2;

struct SegmentWork {
    orig_idx: usize,
    segment: Step5DraftSegment,
    split_parts: Vec<Step5SplitPart>,
    result: Option<Step5AlignedParent>,
}

pub async fn build_step_5_split_align_with_progress(
    request: BuildStep5SplitAlignRequest,
    on_progress: Option<Arc<dyn Fn(usize, usize) + Send + Sync>>,
) -> Result<BuildStep5SplitAlignResponse, String> {
    let llm_client = OpenAiCompatLlmClient::new(LlmConfig::new(
        request.translate_base_url.clone(),
        request.translate_api_key.clone(),
        request.translate_model.clone(),
    ))?;

    let effective_limits = crate::services::subtitle_length::effective_subtitle_limits_from_preset(
        &request.source_lang,
        &request.target_lang,
        &request.subtitle_length_preset,
    );
    let source_limit = effective_limits.source_limit as f64;
    let target_limit = effective_limits.target_limit as f64;

    // ── Pre-scan: separate overlong vs non-overlong ──
    let overlong: Vec<(usize, Step5DraftSegment)> = request
        .segments
        .iter()
        .enumerate()
        .filter(|(_, s)| {
            text_length_units(&s.source, &request.source_lang) > source_limit
                || text_length_units(&s.draft_translation, &request.target_lang) > target_limit
        })
        .map(|(i, s)| (i, s.clone()))
        .collect();

    // ── Load precomputed checkpoint ──
    let mut cached: HashMap<usize, Step5AlignedParent> = HashMap::new();
    if let Some(us) = request.unit_store.as_ref() {
        let rows = us.load_step5_split_aligns().await.unwrap_or_default();
        for row in rows {
            if let Ok(parent) = serde_json::from_str(&row.parent_json) {
                cached.insert(row.segment_index, parent);
            }
        }
    }

    // ── Build work items: cached ones get result immediately ──
    let mut works = Vec::<SegmentWork>::new();
    for (orig_idx, segment) in overlong {
        if let Some(parent) = cached.remove(&orig_idx) {
            works.push(SegmentWork {
                orig_idx,
                segment,
                split_parts: Vec::new(),
                result: Some(parent),
            });
        } else {
            works.push(SegmentWork {
                orig_idx,
                segment: segment.clone(),
                split_parts: vec![Step5SplitPart {
                    part_id: 1,
                    start: segment.start,
                    end: segment.end.max(segment.start),
                    source: segment.source.clone(),
                    tokens: segment.tokens.clone(),
                }],
                result: None,
            });
        }
    }

    let llm_client = Arc::new(llm_client);
    let concurrency = request.llm_concurrency.max(1) as usize;

    // ── Round-barrier loop with cumulative progress ──
    // Progress is monotonically increasing: done never resets, total grows
    // each round.  This ensures the frontend's monotonicity filter works
    // correctly without any special-casing.
    let mut cumulative_done = 0usize;
    let mut cumulative_total = 0usize;

    for round in 1..=MAX_REFINE_ROUNDS {
        let pending_indices: Vec<usize> = works
            .iter()
            .enumerate()
            .filter(|(_, w)| w.result.is_none())
            .map(|(i, _)| i)
            .collect();

        if pending_indices.is_empty() {
            break;
        }

        let round_count = pending_indices.len();
        cumulative_total += round_count;
        if let Some(cb) = on_progress.as_ref() {
            cb(cumulative_done, cumulative_total);
        }

        let semaphore = Arc::new(Semaphore::new(concurrency));
        let mut join_set = JoinSet::new();

        for &work_idx in &pending_indices {
            let sem = Arc::clone(&semaphore);
            let llm_client = Arc::clone(&llm_client);
            let source_lang = request.source_lang.clone();
            let target_lang = request.target_lang.clone();
            let theme_summary = request.theme_summary.clone();
            let terminology_entries = request.terminology_entries.clone();
            let task_id = request.task_id.clone();
            let media_path = request.media_path.clone();
            let unit_store = request.unit_store.clone();

            // Snapshot the work state needed for this round
            let segment = works[work_idx].segment.clone();
            let split_parts = works[work_idx].split_parts.clone();

            join_set.spawn(async move {
                let _permit = sem.acquire_owned().await;
                let context = LlmCallContext {
                    task_id: task_id.to_string(),
                    media_path: Some(media_path.to_string()),
                    phase: "step_5_split_align".to_string(),
                    store: unit_store.map(|us| us.store().clone()),
                };
                let result = process_single_round(
                    &segment,
                    &split_parts,
                    &source_lang,
                    &target_lang,
                    source_limit,
                    target_limit,
                    &theme_summary,
                    &terminology_entries,
                    &llm_client,
                    &context,
                    round,
                )
                .await;
                (work_idx, result)
            });
        }

        while let Some(joined) = join_set.join_next().await {
            let Ok((work_idx, result)) = joined else {
                cumulative_done += 1;
                continue;
            };
            if let Ok(aligned) = result {
                // Persist immediately on completion so resume after a crash
                // / restart picks up exactly what was done. Saving only
                // after the whole round loop (the old behavior) lost ALL
                // in-flight work whenever the process exited mid-round.
                if let Some(us) = request.unit_store.as_ref() {
                    if let Ok(json) = serde_json::to_string(&aligned) {
                        let _ = us
                            .save_step5_split_align(
                                &crate::services::pipeline::Step5SplitAlignRow {
                                    segment_index: works[work_idx].orig_idx,
                                    parent_json: json,
                                },
                            )
                            .await;
                    }
                }
                works[work_idx].result = Some(aligned);
            }
            cumulative_done += 1;
            if let Some(cb) = on_progress.as_ref() {
                cb(cumulative_done, cumulative_total);
            }
        }

        // ── Post-round: check which completed works still have overlong parts ──
        // Only check if there are more rounds ahead
        if round < MAX_REFINE_ROUNDS {
            for work in works.iter_mut() {
                let Some(ref aligned) = work.result else {
                    continue;
                };
                let mut has_overlong = false;
                for part in &aligned.parts {
                    let su = text_length_units(&part.source, &request.source_lang);
                    let tu = text_length_units(&part.translation, &request.target_lang);
                    if (su > source_limit || tu > target_limit) && part.tokens.len() > 1 {
                        has_overlong = true;
                        break;
                    }
                }
                if !has_overlong {
                    continue;
                }
                // If the LLM refused to split this round (part count did not grow),
                // it will almost certainly refuse next round too — skip to avoid
                // wasting tokens.  Only segments that actually split-but-still-overlong
                // benefit from another pass.
                let old_part_count = work.split_parts.len();
                let new_part_count = aligned.parts.len();
                if new_part_count <= old_part_count {
                    continue;
                }
                // Prepare for next round: keep result=None, update split_parts
                work.split_parts = aligned
                    .parts
                    .iter()
                    .map(|p| Step5SplitPart {
                        part_id: 0,
                        start: p.start,
                        end: p.end,
                        source: p.source.clone(),
                        tokens: p.tokens.clone(),
                    })
                    .collect();
                work.result = None;
            }
        }
    }

    // ── Build final output ──
    // NB: per-work persistence already happens inside the round loop, so we
    // do NOT save again here -- doing so would just rewrite the same rows.

    // Build a map from orig_idx → result for fast lookup
    let mut result_map: HashMap<usize, Step5AlignedParent> = HashMap::new();
    for work in works {
        if let Some(parent) = work.result {
            result_map.insert(work.orig_idx, parent);
        }
    }

    let mut parents = Vec::with_capacity(request.segments.len());
    let mut part_total = 0usize;
    for (i, segment) in request.segments.iter().enumerate() {
        if let Some(parent) = result_map.remove(&i) {
            part_total += parent.parts.len();
            parents.push(parent);
        } else {
            let part = Step5AlignedPart {
                part_id: 1,
                start: segment.start,
                end: segment.end.max(segment.start),
                source: segment.source.clone(),
                translation: segment.draft_translation.clone(),
                tokens: segment.tokens.clone(),
            };
            part_total += 1;
            parents.push(Step5AlignedParent {
                parent_segment_id: segment.segment_id,
                parts: vec![part],
                rounds_used: 0,
            });
        }
    }

    Ok(BuildStep5SplitAlignResponse {
        parent_total: parents.len(),
        part_total,
        parents,
    })
}

async fn process_single_round(
    segment: &Step5DraftSegment,
    split_parts: &[Step5SplitPart],
    source_lang: &str,
    target_lang: &str,
    source_limit: f64,
    target_limit: f64,
    theme_summary: &str,
    terminology_entries: &[super::types::Step5TerminologyEntry],
    llm_client: &OpenAiCompatLlmClient,
    context: &LlmCallContext,
    round: usize,
) -> Result<Step5AlignedParent, String> {
    // ── Phase 1: Split overlong source parts ──
    let mut new_parts = Vec::new();
    for part in split_parts {
        let su = text_length_units(&part.source, source_lang);
        if su > source_limit && part.tokens.len() > 1 {
            let must = su > source_limit * OBVIOUS_OVERLONG_RATIO;
            let split = binary_split_source_part(
                part,
                segment,
                source_lang,
                target_lang,
                source_limit,
                target_limit,
                round,
                must,
                llm_client,
                context,
            )
            .await;
            if split.len() > 1 {
                new_parts.extend(split);
                continue;
            }
        }
        new_parts.push(part.clone());
    }

    for (i, p) in new_parts.iter_mut().enumerate() {
        p.part_id = i + 1;
    }

    // ── Phase 2: Align ──
    let parent = Step5SplitParent {
        parent_segment_id: segment.segment_id,
        draft_translation: segment.draft_translation.clone(),
        parts: new_parts,
    };
    let mut aligned = align_single_parent(
        &parent,
        source_lang,
        target_lang,
        theme_summary,
        terminology_entries,
        llm_client,
        context,
    )
    .await?;

    aligned.rounds_used = round;
    Ok(aligned)
}

async fn binary_split_source_part(
    part: &Step5SplitPart,
    segment: &Step5DraftSegment,
    source_lang: &str,
    target_lang: &str,
    source_limit: f64,
    target_limit: f64,
    round: usize,
    must_split: bool,
    llm_client: &OpenAiCompatLlmClient,
    context: &LlmCallContext,
) -> Vec<Step5SplitPart> {
    let prompt = build_source_split_prompt(
        source_lang,
        target_lang,
        &segment.source,
        &segment.draft_translation,
        &part.source,
        source_limit,
        target_limit,
        round,
        must_split,
    );

    let llm_id = next_llm_request_id();
    let call = llm_client
        .call_json_validated(&context, &llm_id, &prompt, None, |value| {
            validate_source_split_response(value, &part.source, must_split)
        })
        .await;

    let Ok(call) = call else {
        return vec![part.clone()];
    };

    let boundaries =
        map_source_parts_to_boundaries(&call.value, &part.tokens, source_lang);
    let boundaries = normalize_split_boundaries(&boundaries, part.tokens.len(), &[], &[], 1);
    let ranges = boundary_ids_to_ranges(&boundaries, part.tokens.len());
    if ranges.len() <= 1 {
        return vec![part.clone()];
    }

    ranges
        .into_iter()
        .enumerate()
        .map(|(index, (s, e))| {
            let tokens = part.tokens[s..=e].to_vec();
            Step5SplitPart {
                part_id: index + 1,
                start: tokens.first().map(|t| t.start).unwrap_or(part.start),
                end: tokens.last().map(|t| t.end).unwrap_or(part.end),
                source: build_source_from_tokens(&tokens),
                tokens,
            }
        })
        .collect()
}

async fn align_single_parent(
    parent: &Step5SplitParent,
    source_lang: &str,
    target_lang: &str,
    theme_summary: &str,
    terminology_entries: &[super::types::Step5TerminologyEntry],
    llm_client: &OpenAiCompatLlmClient,
    context: &LlmCallContext,
) -> Result<Step5AlignedParent, String> {
    let part_sources: Vec<String> = parent
        .parts
        .iter()
        .map(|part| normalize_inline_text(&part.source))
        .collect();
    let count = part_sources.len().max(1);

    let fallback = heuristic_split_translation(&parent.draft_translation, count, Some(&parent.parts));
    let mut aligned_lines = fallback.clone();

    if count > 1 {
        let source_joined = part_sources.join(" ");
        let prompt_terms = select_terms_for_text(&source_joined, terminology_entries, MAX_TERMS_PER_LINE);
        let prompt_lines: Vec<Step5PromptLine> = part_sources
            .iter()
            .enumerate()
            .map(|(i, s)| Step5PromptLine { id: i + 1, source: s.clone() })
            .collect();
        let prompt_terms: Vec<Step5PromptTerm> = prompt_terms
            .iter()
            .map(|t| Step5PromptTerm {
                source: t.source.clone(),
                target: t.target.clone(),
                note: t.note.clone(),
            })
            .collect();
        let prompt = build_align_prompt(
            source_lang,
            target_lang,
            theme_summary,
            &source_joined,
            &parent.draft_translation,
            &prompt_lines,
            &prompt_terms,
        );

        let expected_ids: Vec<usize> = (1..=count).collect();
        let llm_id = next_llm_request_id();
        let call = llm_client
            .call_json_validated(context, &llm_id, &prompt, None, |value| {
                validate_align_response(value, &expected_ids)
            })
            .await;

        if let Ok(call) = call {
            let mut lines = Vec::with_capacity(expected_ids.len());
            for id in &expected_ids {
                lines.push(call.value.get(id).cloned().unwrap_or_default());
            }
            if lines.iter().any(|l| !l.trim().is_empty()) {
                aligned_lines = lines;
            }
        }
    }

    let aligned_candidate = repair_aligned_lines(parent, &aligned_lines, &fallback, target_lang);
    let fallback_candidate = repair_aligned_lines(parent, &fallback, &fallback, target_lang);
    let best = choose_better_alignment(parent, &aligned_candidate, &fallback_candidate, target_lang);

    let mut parts = Vec::with_capacity(parent.parts.len());
    for (i, part) in parent.parts.iter().enumerate() {
        let text = best.get(i).cloned().unwrap_or_default();
        parts.push(Step5AlignedPart {
            part_id: part.part_id,
            start: part.start,
            end: part.end,
            source: part.source.clone(),
            translation: normalize_inline_text(&text),
            tokens: part.tokens.clone(),
        });
    }

    Ok(Step5AlignedParent {
        parent_segment_id: parent.parent_segment_id,
        parts,
        rounds_used: 0, // caller overwrites
    })
}
