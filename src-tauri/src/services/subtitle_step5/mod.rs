use std::collections::HashMap;
use std::sync::Arc;

use crate::services::llm::batch::run_indexed_concurrent_with_progress;
use crate::services::llm::client::OpenAiCompatLlmClient;
use crate::services::llm::port::{LlmCallContext, LlmConfig, LlmJsonTask, next_llm_request_id};
use crate::services::prompts::subtitle_step5::{
    Step5PromptLine, Step5PromptTerm, build_align_prompt, build_polish_prompt,
    build_source_split_prompt,
};

mod alignment_repair;
mod alignment_score;
mod clauses;
mod constants;
mod final_check;
mod language_units;
mod numbers;
mod polish_repair;
mod quality;
mod request_validation;
mod responses;
mod source_residue;
mod source_split;
mod source_text;
mod split_parts;
mod stage_models;
mod terminology_filter;
mod text_utils;
mod time_utils;
mod translation_candidate;
mod translation_split;
mod types;
mod watchability;

use alignment_repair::repair_aligned_lines;
use alignment_score::choose_better_alignment;
use constants::*;
pub use final_check::build_step_6_final_check;
use language_units::text_length_units;
use numbers::extract_numbers;
use polish_repair::repair_polished_translation;
#[cfg(test)]
use quality::split_line_quality_score;
use request_validation::{validate_step5_align_request, validate_step5_polish_request};
use responses::{
    validate_align_response, validate_polish_response, validate_source_split_response,
};
use source_residue::looks_like_non_cjk_translation_for_cjk_target;
#[cfg(test)]
use source_residue::looks_like_source_residue;
use source_split::{
    choose_preferred_split_ranges, desired_split_parts, enforce_source_limit_ranges,
    hard_pause_boundaries, map_source_parts_to_boundaries, merge_tiny_ranges_for_readability,
    normalize_split_boundaries, rebalance_dangling_tail_tokens, split_token_ranges,
};
use source_text::build_source_from_tokens;
use split_parts::{
    boundary_ids_to_ranges, build_single_split_part, build_split_parts_from_ranges,
    ranges_to_boundary_ids,
};
use stage_models::{Step5SplitTask, Step51LlmSplitTask, Step51SplitWorkItem};
use terminology_filter::select_terms_for_text;
#[cfg(test)]
use terminology_filter::source_contains_terminology_term;
use text_utils::normalize_inline_text;
#[cfg(test)]
use translation_candidate::has_tail_ellipsis;
#[cfg(test)]
use translation_candidate::{is_unusable_translation, trim_before_leaked_number_anchor};
use translation_split::heuristic_split_translation;
pub use types::*;
#[cfg(test)]
use watchability::is_watchability_fragment_issue;
pub use watchability::merge_watchability_subtitle_srt_segments;
#[cfg(test)]
use watchability::repair_single_watchability_line;
use watchability::{
    apply_residual_watchability_overrides, repair_watchability_fragments,
    split_watchability_overlong_segments,
};

pub async fn build_step_5_1_source_split_with_progress(
    request: BuildStep5SourceSplitRequest,
    on_progress: Option<Arc<dyn Fn(usize, usize) + Send + Sync>>,
) -> Result<BuildStep5SourceSplitResponse, String> {
    if request.segments.is_empty() {
        return Err("segments is required".to_string());
    }

    let source_limit = request.subtitle_max_words_per_segment.clamp(8, 40) as f64;
    let target_limit = request.subtitle_length_reference.clamp(8, 80) as f64;
    let mut work_items = Vec::<Step51SplitWorkItem>::new();
    let mut llm_tasks = Vec::<Step51LlmSplitTask>::new();
    for segment in request.segments {
        let source_text = if segment.tokens.is_empty() {
            normalize_inline_text(&segment.source)
        } else {
            build_source_from_tokens(&segment.tokens)
        };
        let draft_translation = normalize_inline_text(&segment.draft_translation);
        let source_units = text_length_units(&source_text, &request.source_lang);
        let target_units = text_length_units(&draft_translation, &request.target_lang);
        let mandatory_boundaries = hard_pause_boundaries(&segment.tokens);
        let over_length = source_units > source_limit || target_units > target_limit;
        let should_split = over_length || !mandatory_boundaries.is_empty();
        let fallback_ranges = if segment.tokens.is_empty() {
            Vec::new()
        } else if should_split {
            split_token_ranges(
                &segment.tokens,
                &request.source_lang,
                source_limit,
                target_limit,
                source_units,
                target_units,
            )
        } else {
            vec![(0usize, segment.tokens.len().saturating_sub(1))]
        };
        let fallback_boundaries = ranges_to_boundary_ids(&fallback_ranges);
        let min_parts = desired_split_parts(source_units, source_limit, target_units, target_limit)
            .max(mandatory_boundaries.len() + 1)
            .max(1);
        let work_index = work_items.len();
        if over_length && segment.tokens.len() > 1 {
            llm_tasks.push(Step51LlmSplitTask {
                task_id: llm_tasks.len(),
                work_index,
                source_lang: request.source_lang.clone(),
                tokens: segment.tokens.clone(),
                mandatory_boundaries: mandatory_boundaries.clone(),
                fallback_boundaries: fallback_boundaries.clone(),
                min_parts,
                prompt: build_source_split_prompt(
                    &request.source_lang,
                    &request.target_lang,
                    &source_text,
                    &draft_translation,
                    source_limit,
                    target_limit,
                    min_parts,
                ),
            });
        }
        work_items.push(Step51SplitWorkItem {
            segment,
            draft_translation,
            mandatory_boundaries,
            fallback_boundaries,
            over_length,
            min_parts,
        });
    }

    let mut split_by_work_index = HashMap::<usize, Vec<usize>>::new();
    if !llm_tasks.is_empty() {
        if request.translate_api_key.trim().is_empty() {
            return Err("translateApiKey is required".to_string());
        }
        if request.translate_base_url.trim().is_empty() {
            return Err("translateBaseUrl is required".to_string());
        }
        if request.translate_model.trim().is_empty() {
            return Err("translateModel is required".to_string());
        }
        let llm_client = OpenAiCompatLlmClient::new(LlmConfig::new(
            request.translate_base_url.clone(),
            request.translate_api_key.clone(),
            request.translate_model.clone(),
        ))
        .map_err(|err| err.message)?;

        let tasks = llm_tasks
            .iter()
            .map(|task| LlmJsonTask {
                id: task.task_id,
                request_id: next_llm_request_id(),
                user_prompt: task.prompt.clone(),
                response_validator: None,
            })
            .collect::<Vec<_>>();
        let context = LlmCallContext {
            task_id: request.task_id.clone(),
            media_path: Some(request.media_path.clone()),
            phase: "step_5_1_source_split".to_string(),
        };
        let llm_task_snapshot = llm_tasks.clone();
        let results = run_indexed_concurrent_with_progress(
            tasks,
            request.llm_concurrency.max(1) as usize,
            {
                let llm_client = llm_client.clone();
                let context = context.clone();
                move |task| {
                    let llm_client = llm_client.clone();
                    let context = context.clone();
                    let llm_tasks = llm_task_snapshot.clone();
                    async move {
                        let Some(split_task) = llm_tasks.get(task.id) else {
                            return Err(format!("missing step5_1 split task {}", task.id));
                        };
                        let llm_id = task.request_id.clone();
                        let call = llm_client
                            .call_json_validated(
                                &context,
                                &llm_id,
                                &task.user_prompt,
                                task.response_validator.as_ref(),
                                |value| {
                                    validate_source_split_response(
                                        value,
                                        split_task.min_parts.max(2),
                                    )
                                },
                            )
                            .await
                            .map_err(|err| {
                                format!(
                                    "step5_1 source split failed (llmId={}): {}",
                                    llm_id, err.message
                                )
                            })?;
                        let mapped_boundaries = map_source_parts_to_boundaries(
                            &call.value,
                            &split_task.tokens,
                            &split_task.source_lang,
                        );
                        let boundaries = normalize_split_boundaries(
                            &mapped_boundaries,
                            split_task.tokens.len(),
                            &split_task.mandatory_boundaries,
                            &split_task.fallback_boundaries,
                            split_task.min_parts,
                        );
                        Ok((split_task.work_index, boundaries))
                    }
                }
            },
            |msg| msg,
            |_done, _total| {},
        )
        .await;
        for (_, result) in results {
            let Ok((work_index, boundaries)) = result else {
                continue;
            };
            split_by_work_index.insert(work_index, boundaries);
        }
    }

    let total = work_items.len().max(1);
    if let Some(callback) = on_progress.as_ref() {
        callback(0, total);
    }
    let mut parents = Vec::<Step5SplitParent>::new();
    let mut part_total = 0usize;
    for (work_index, work) in work_items.into_iter().enumerate() {
        let ranges = if work.segment.tokens.is_empty() {
            Vec::new()
        } else if work.over_length {
            let llm_boundaries = split_by_work_index
                .get(&work_index)
                .cloned()
                .unwrap_or_else(|| {
                    normalize_split_boundaries(
                        &work.fallback_boundaries,
                        work.segment.tokens.len(),
                        &work.mandatory_boundaries,
                        &work.fallback_boundaries,
                        work.min_parts,
                    )
                });
            let llm_ranges = boundary_ids_to_ranges(&llm_boundaries, work.segment.tokens.len());
            let fallback_ranges =
                boundary_ids_to_ranges(&work.fallback_boundaries, work.segment.tokens.len());
            choose_preferred_split_ranges(
                llm_ranges,
                fallback_ranges,
                &work.segment.tokens,
                &request.source_lang,
                source_limit,
            )
        } else {
            boundary_ids_to_ranges(&work.mandatory_boundaries, work.segment.tokens.len())
        };
        let ranges = merge_tiny_ranges_for_readability(
            ranges,
            &work.segment.tokens,
            &request.source_lang,
            source_limit,
            &work.mandatory_boundaries,
        );
        let ranges = rebalance_dangling_tail_tokens(
            ranges,
            &work.segment.tokens,
            &request.source_lang,
            source_limit,
            &work.mandatory_boundaries,
        );
        let ranges = enforce_source_limit_ranges(
            ranges,
            &work.segment.tokens,
            &request.source_lang,
            source_limit,
        );
        let ranges = merge_tiny_ranges_for_readability(
            ranges,
            &work.segment.tokens,
            &request.source_lang,
            source_limit,
            &work.mandatory_boundaries,
        );
        let ranges = rebalance_dangling_tail_tokens(
            ranges,
            &work.segment.tokens,
            &request.source_lang,
            source_limit,
            &work.mandatory_boundaries,
        );
        let parts = if ranges.is_empty() {
            vec![build_single_split_part(&work.segment)]
        } else {
            build_split_parts_from_ranges(&work.segment, &ranges)
        };
        part_total += parts.len();
        parents.push(Step5SplitParent {
            parent_segment_id: work.segment.segment_id,
            draft_translation: work.draft_translation,
            parts,
        });
        if let Some(callback) = on_progress.as_ref() {
            callback(parents.len(), total);
        }
    }

    Ok(BuildStep5SourceSplitResponse {
        subtitle_max_words_per_segment: source_limit as u32,
        subtitle_length_reference: target_limit as u32,
        parent_total: parents.len(),
        part_total,
        parents,
    })
}

pub async fn build_step_5_2_translation_align_with_progress(
    request: BuildStep5TranslationAlignRequest,
    on_progress: Option<Arc<dyn Fn(usize, usize) + Send + Sync>>,
) -> Result<BuildStep5TranslationAlignResponse, String> {
    validate_step5_align_request(&request)?;

    let llm_client = OpenAiCompatLlmClient::new(LlmConfig::new(
        request.translate_base_url.clone(),
        request.translate_api_key.clone(),
        request.translate_model.clone(),
    ))
    .map_err(|err| err.message)?;

    let mut aligned_by_parent = HashMap::<usize, Vec<String>>::new();
    let mut split_tasks = Vec::<Step5SplitTask>::new();

    for parent in &request.parents {
        let part_sources = parent
            .parts
            .iter()
            .map(|part| normalize_inline_text(&part.source))
            .collect::<Vec<_>>();
        let count = part_sources.len().max(1);
        let fallback =
            heuristic_split_translation(&parent.draft_translation, count, Some(&parent.parts));
        aligned_by_parent.insert(parent.parent_segment_id, fallback);

        if count <= 1 {
            continue;
        }

        let source_joined = part_sources.join(" ");
        let prompt_terms = select_terms_for_text(
            &source_joined,
            &request.terminology_entries,
            MAX_TERMS_PER_LINE,
        );
        let prompt_lines = part_sources
            .iter()
            .enumerate()
            .map(|(index, source)| Step5PromptLine {
                id: index + 1,
                source: source.clone(),
            })
            .collect::<Vec<_>>();
        let prompt_terms = prompt_terms
            .iter()
            .map(|term| Step5PromptTerm {
                source: term.source.clone(),
                target: term.target.clone(),
                note: term.note.clone(),
            })
            .collect::<Vec<_>>();
        let prompt = build_align_prompt(
            &request.source_lang,
            &request.target_lang,
            &request.theme_summary,
            &source_joined,
            &parent.draft_translation,
            &prompt_lines,
            &prompt_terms,
        );
        split_tasks.push(Step5SplitTask {
            task_id: split_tasks.len(),
            parent_segment_id: parent.parent_segment_id,
            part_sources,
            prompt,
        });
    }

    if !split_tasks.is_empty() {
        let tasks = split_tasks
            .iter()
            .map(|task| LlmJsonTask {
                id: task.task_id,
                request_id: next_llm_request_id(),
                user_prompt: task.prompt.clone(),
                response_validator: None,
            })
            .collect::<Vec<_>>();

        let context = LlmCallContext {
            task_id: request.task_id.clone(),
            media_path: Some(request.media_path.clone()),
            phase: "step_5_2_translation_align".to_string(),
        };
        let split_tasks_for_worker = split_tasks.clone();
        let results = run_indexed_concurrent_with_progress(
            tasks,
            request.llm_concurrency.max(1) as usize,
            {
                let llm_client = llm_client.clone();
                let context = context.clone();
                move |task| {
                    let llm_client = llm_client.clone();
                    let context = context.clone();
                    let split_tasks = split_tasks_for_worker.clone();
                    async move {
                        let Some(split_task) = split_tasks.get(task.id) else {
                            return Err(format!("missing step5 split task {}", task.id));
                        };
                        let expected_ids = (1..=split_task.part_sources.len()).collect::<Vec<_>>();
                        let llm_id = task.request_id.clone();
                        let call = llm_client
                            .call_json_validated(
                                &context,
                                &llm_id,
                                &task.user_prompt,
                                task.response_validator.as_ref(),
                                |value| validate_align_response(value, &expected_ids),
                            )
                            .await
                            .map_err(|err| {
                                format!("step5 align failed (llmId={}): {}", llm_id, err.message)
                            })?;
                        let mut lines = Vec::<String>::new();
                        for expected_id in expected_ids {
                            lines.push(call.value.get(&expected_id).cloned().unwrap_or_default());
                        }
                        Ok(lines)
                    }
                }
            },
            |msg| msg,
            |_done, _total| {},
        )
        .await;

        for (index, result) in results {
            let Ok(lines) = result else {
                continue;
            };
            let Some(task) = split_tasks.get(index) else {
                continue;
            };
            if lines.len() != task.part_sources.len() {
                continue;
            }
            let line_has_text = lines.iter().any(|line| !line.trim().is_empty());
            if !line_has_text {
                continue;
            }
            aligned_by_parent.insert(task.parent_segment_id, lines);
        }
    }

    let total = request.parents.len().max(1);
    if let Some(callback) = on_progress.as_ref() {
        callback(0, total);
    }
    let mut output_parents = Vec::<Step5AlignedParent>::new();
    let mut part_total = 0usize;
    for parent in &request.parents {
        let expected_count = parent.parts.len();
        let fallback = heuristic_split_translation(
            &parent.draft_translation,
            expected_count.max(1),
            Some(&parent.parts),
        );
        let aligned_raw = aligned_by_parent
            .get(&parent.parent_segment_id)
            .cloned()
            .unwrap_or_else(|| fallback.clone());
        let aligned_candidate =
            repair_aligned_lines(parent, &aligned_raw, &fallback, &request.target_lang);
        let fallback_candidate =
            repair_aligned_lines(parent, &fallback, &fallback, &request.target_lang);
        let aligned = choose_better_alignment(
            parent,
            &aligned_candidate,
            &fallback_candidate,
            &request.target_lang,
        );
        let mut parts = Vec::<Step5AlignedPart>::new();
        for (index, part) in parent.parts.iter().enumerate() {
            let text = aligned.get(index).cloned().unwrap_or_default();
            parts.push(Step5AlignedPart {
                part_id: part.part_id,
                start: part.start,
                end: part.end,
                source: part.source.clone(),
                translation: normalize_inline_text(&text),
                tokens: part.tokens.clone(),
            });
        }
        part_total += parts.len();
        output_parents.push(Step5AlignedParent {
            parent_segment_id: parent.parent_segment_id,
            parts,
        });
        if let Some(callback) = on_progress.as_ref() {
            callback(output_parents.len(), total);
        }
    }

    Ok(BuildStep5TranslationAlignResponse {
        parent_total: output_parents.len(),
        part_total,
        parents: output_parents,
    })
}

pub async fn build_step_5_3_translation_polish_with_progress(
    request: BuildStep5TranslationPolishRequest,
    on_progress: Option<Arc<dyn Fn(usize, usize) + Send + Sync>>,
) -> Result<BuildStep5TranslationPolishResponse, String> {
    validate_step5_polish_request(&request)?;

    let llm_client = OpenAiCompatLlmClient::new(LlmConfig::new(
        request.translate_base_url.clone(),
        request.translate_api_key.clone(),
        request.translate_model.clone(),
    ))
    .map_err(|err| err.message)?;

    let subtitle_length_reference = request.subtitle_length_reference.clamp(8, 80) as f64;
    let mut segments = Vec::<Step5FinalSegment>::new();
    for parent in &request.parents {
        for part in &parent.parts {
            segments.push(Step5FinalSegment {
                segment_id: segments.len() + 1,
                start: part.start,
                end: part.end.max(part.start),
                source: normalize_inline_text(&part.source),
                translation: normalize_inline_text(&part.translation),
                tokens: part.tokens.clone(),
            });
        }
    }
    let baseline_translations = segments
        .iter()
        .map(|segment| segment.translation.clone())
        .collect::<Vec<_>>();

    let polish_length_trigger = (subtitle_length_reference as f64).min(LONG_LINE_SCORE_TRIGGER);
    let mut polish_candidates = Vec::<usize>::new();
    for (index, segment) in segments.iter().enumerate() {
        let target_len = text_length_units(&segment.translation, &request.target_lang);
        if target_len > polish_length_trigger {
            polish_candidates.push(index);
        }
    }

    if !polish_candidates.is_empty() {
        let tasks = polish_candidates
            .iter()
            .enumerate()
            .map(|(task_id, segment_index)| {
                let segment = &segments[*segment_index];
                let terms = select_terms_for_text(
                    &segment.source,
                    &request.terminology_entries,
                    MAX_TERMS_PER_LINE,
                );
                let prompt_terms = terms
                    .iter()
                    .map(|term| Step5PromptTerm {
                        source: term.source.clone(),
                        target: term.target.clone(),
                        note: term.note.clone(),
                    })
                    .collect::<Vec<_>>();
                let prompt = build_polish_prompt(
                    &request.source_lang,
                    &request.target_lang,
                    &segment.source,
                    &segment.translation,
                    subtitle_length_reference,
                    &prompt_terms,
                );
                LlmJsonTask {
                    id: task_id,
                    request_id: next_llm_request_id(),
                    user_prompt: prompt,
                    response_validator: None,
                }
            })
            .collect::<Vec<_>>();

        let context = LlmCallContext {
            task_id: request.task_id.clone(),
            media_path: Some(request.media_path.clone()),
            phase: "step_5_3_translation_polish".to_string(),
        };
        let candidate_indexes = polish_candidates.clone();
        let progress_callback = on_progress.clone();
        let results = run_indexed_concurrent_with_progress(
            tasks,
            request.llm_concurrency.max(1) as usize,
            {
                let llm_client = llm_client.clone();
                let context = context.clone();
                move |task| {
                    let llm_client = llm_client.clone();
                    let context = context.clone();
                    let indexes = candidate_indexes.clone();
                    async move {
                        let Some(segment_index) = indexes.get(task.id).copied() else {
                            return Err(format!("missing step5 polish task {}", task.id));
                        };
                        let llm_id = task.request_id.clone();
                        let call = llm_client
                            .call_json_validated(
                                &context,
                                &llm_id,
                                &task.user_prompt,
                                task.response_validator.as_ref(),
                                validate_polish_response,
                            )
                            .await
                            .map_err(|err| {
                                format!("step5 polish failed (llmId={}): {}", llm_id, err.message)
                            })?;
                        Ok((segment_index, call.value))
                    }
                }
            },
            |msg| msg,
            move |done, total| {
                if let Some(callback) = progress_callback.as_ref() {
                    callback(done, total);
                }
            },
        )
        .await;

        for (_, result) in results {
            let Ok((segment_index, polished)) = result else {
                continue;
            };
            let Some(segment) = segments.get_mut(segment_index) else {
                continue;
            };
            let polished = normalize_inline_text(&polished);
            if polished.is_empty() {
                continue;
            }
            if looks_like_non_cjk_translation_for_cjk_target(&polished, &request.target_lang) {
                continue;
            }
            let source_numbers = extract_numbers(&segment.source);
            if !source_numbers.is_empty() {
                let polished_numbers = extract_numbers(&polished);
                let dropped_anchor = source_numbers
                    .iter()
                    .any(|value| !polished_numbers.contains(value));
                if dropped_anchor {
                    continue;
                }
            }
            if text_length_units(&polished, &request.target_lang)
                <= text_length_units(&segment.translation, &request.target_lang) * 1.02
            {
                segment.translation = polished;
            }
        }
    } else if let Some(callback) = on_progress.as_ref() {
        callback(1, 1);
    }
    for segment in &mut segments {
        repair_polished_translation(segment);
    }
    repair_watchability_fragments(&mut segments, &request.target_lang);
    for segment in &mut segments {
        repair_polished_translation(segment);
    }
    apply_residual_watchability_overrides(&mut segments, &request.target_lang);
    for (index, segment) in segments.iter_mut().enumerate() {
        if !looks_like_non_cjk_translation_for_cjk_target(
            &segment.translation,
            &request.target_lang,
        ) {
            continue;
        }
        let fallback = baseline_translations
            .get(index)
            .cloned()
            .unwrap_or_default();
        if fallback.is_empty() {
            continue;
        }
        segment.translation = fallback;
        repair_polished_translation(segment);
    }
    split_watchability_overlong_segments(
        &mut segments,
        WATCHABILITY_SPLIT_TRIGGER,
        &request.target_lang,
    );
    for segment in &mut segments {
        repair_polished_translation(segment);
    }
    repair_watchability_fragments(&mut segments, &request.target_lang);
    apply_residual_watchability_overrides(&mut segments, &request.target_lang);

    let batch_size = if request.batch_size == 0 {
        DEFAULT_BATCH_SIZE
    } else {
        request
            .batch_size
            .clamp(1, MAX_BATCH_SIZE)
            .max(DEFAULT_BATCH_SIZE.min(MAX_BATCH_SIZE))
    };
    let batch_total = if segments.is_empty() {
        0
    } else {
        (segments.len() + batch_size - 1) / batch_size
    };

    Ok(BuildStep5TranslationPolishResponse {
        batch_size,
        batch_total,
        segment_total: segments.len(),
        segments,
    })
}

#[cfg(test)]
mod tests;
