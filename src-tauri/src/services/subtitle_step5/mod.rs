use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::services::llm::batch::run_indexed_concurrent_with_progress;
use crate::services::llm::client::OpenAiCompatLlmClient;
use crate::services::llm::port::{LlmCallContext, LlmConfig, LlmJsonTask, next_llm_request_id};
use crate::services::prompts::subtitle_step5::{
    Step5PromptLine, Step5PromptTerm, build_align_prompt, build_polish_prompt,
    build_source_split_prompt,
};

mod language_units;
mod responses;
mod types;

use language_units::{
    count_word_units, is_cjk_char, is_hangul_char, text_length_units, use_char_units,
};
use responses::{
    validate_align_response, validate_polish_response, validate_source_split_response,
};
pub use types::*;

const HARD_SPLIT_GAP_SECONDS: f64 = 2.0;
const SOFT_SPLIT_GAP_SECONDS: f64 = 0.35;
const MIN_TOKENS_FOR_SOFT_SPLIT: usize = 6;
const FORCE_SPLIT_MARGIN: f64 = 1.05;
const HARD_MIN_SEGMENT_DURATION_SECONDS: f64 = 0.5;
const MIN_READABLE_UNITS: f64 = 3.0;
const MIN_READABLE_DURATION_SECONDS: f64 = 0.9;
const DEFAULT_BATCH_SIZE: usize = 20;
const MAX_BATCH_SIZE: usize = 40;
const MAX_TERMS_PER_LINE: usize = 10;
const LONG_LINE_SCORE_TRIGGER: f64 = 25.0;
const WATCHABILITY_SPLIT_TRIGGER: f64 = 25.0;
const WATCHABILITY_MERGE_TIME_GAP_SECONDS: f64 = 0.5;
const WATCHABILITY_MERGE_TIME_BUDGET_SECONDS: f64 = 6.0;
const WATCHABILITY_MERGE_LEN_RATIO: f64 = 1.55;

#[derive(Debug, Clone)]
struct Step5SplitTask {
    task_id: usize,
    parent_segment_id: usize,
    part_sources: Vec<String>,
    prompt: String,
}

#[derive(Debug, Clone)]
struct Step51SplitWorkItem {
    segment: Step5DraftSegment,
    draft_translation: String,
    mandatory_boundaries: Vec<usize>,
    fallback_boundaries: Vec<usize>,
    over_length: bool,
    min_parts: usize,
}

#[derive(Debug, Clone)]
struct Step51LlmSplitTask {
    task_id: usize,
    work_index: usize,
    source_lang: String,
    tokens: Vec<Step5Token>,
    mandatory_boundaries: Vec<usize>,
    fallback_boundaries: Vec<usize>,
    min_parts: usize,
    prompt: String,
}

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

pub fn build_step_6_final_check(
    request: BuildStep6FinalCheckRequest,
) -> Result<BuildStep6FinalCheckResponse, String> {
    if request.segments.is_empty() {
        return Err("segments is required".to_string());
    }

    let mut issues = Vec::<Step5QualityIssue>::new();
    let mut issue_keys = HashSet::<String>::new();
    let mut empty_count = 0usize;
    let mut ellipsis_tail_count = 0usize;
    let mut numeric_drift_count = 0usize;
    let mut cross_line_leak_count = 0usize;
    let mut gt25_count = 0usize;
    let mut gt32_count = 0usize;

    for (index, segment) in request.segments.iter().enumerate() {
        let source = normalize_inline_text(&segment.source);
        let translation = normalize_inline_text(&segment.translation);
        let target_units = text_length_units(&translation, &request.target_lang);
        if target_units > 25.0 {
            gt25_count += 1;
        }
        if target_units > 32.0 {
            gt32_count += 1;
        }
        if translation.is_empty() {
            empty_count += 1;
            push_quality_issue(
                &mut issues,
                &mut issue_keys,
                Step5QualityIssue {
                    rule_id: "empty_translation".to_string(),
                    severity: "hard".to_string(),
                    segment_id: segment.segment_id,
                    part_id: index + 1,
                    message: "字幕译文为空".to_string(),
                },
            );
            continue;
        }
        if is_punctuation_only(&translation) {
            push_quality_issue(
                &mut issues,
                &mut issue_keys,
                Step5QualityIssue {
                    rule_id: "non_lexical_translation".to_string(),
                    severity: "hard".to_string(),
                    segment_id: segment.segment_id,
                    part_id: index + 1,
                    message: "字幕仅包含标点或无有效文本".to_string(),
                },
            );
            continue;
        }
        if has_tail_ellipsis(&translation) {
            ellipsis_tail_count += 1;
            push_quality_issue(
                &mut issues,
                &mut issue_keys,
                Step5QualityIssue {
                    rule_id: "tail_ellipsis".to_string(),
                    severity: "hard".to_string(),
                    segment_id: segment.segment_id,
                    part_id: index + 1,
                    message: "字幕以省略号结尾，疑似截断".to_string(),
                },
            );
        }
        if looks_like_source_residue(&source, &translation, &request.target_lang) {
            push_quality_issue(
                &mut issues,
                &mut issue_keys,
                Step5QualityIssue {
                    rule_id: "source_residue".to_string(),
                    severity: "hard".to_string(),
                    segment_id: segment.segment_id,
                    part_id: index + 1,
                    message: "译文含大段源语言残留，疑似未翻译".to_string(),
                },
            );
        }

        let source_numbers = extract_numbers(&source);
        if !source_numbers.is_empty() {
            let translation_numbers = extract_numbers(&translation);
            let missing = source_numbers
                .iter()
                .any(|value| !translation_numbers.contains(value));
            if missing {
                numeric_drift_count += 1;
                push_quality_issue(
                    &mut issues,
                    &mut issue_keys,
                    Step5QualityIssue {
                        rule_id: "numeric_drift".to_string(),
                        severity: "hard".to_string(),
                        segment_id: segment.segment_id,
                        part_id: index + 1,
                        message: "数字锚点未保持一致".to_string(),
                    },
                );
            }
        }
        if is_watchability_fragment_issue(&source, &translation, &request.target_lang) {
            push_quality_issue(
                &mut issues,
                &mut issue_keys,
                Step5QualityIssue {
                    rule_id: "watchability_fragment".to_string(),
                    severity: "soft".to_string(),
                    segment_id: segment.segment_id,
                    part_id: index + 1,
                    message: "译文疑似碎片化，影响观看流畅度".to_string(),
                },
            );
        }
    }

    for window in request.segments.windows(2) {
        let current = &window[0];
        let next = &window[1];
        let current_source_numbers = extract_numbers(&current.source);
        let current_translation_numbers = extract_numbers(&current.translation);
        let next_source_numbers = extract_numbers(&next.source);
        if next_source_numbers.is_empty() || current_translation_numbers.is_empty() {
            continue;
        }
        let mut next_only = HashSet::<String>::new();
        for value in next_source_numbers {
            if !current_source_numbers.contains(&value) {
                next_only.insert(value);
            }
        }
        if next_only.is_empty() {
            continue;
        }
        let leaked = next_only
            .iter()
            .any(|value| current_translation_numbers.contains(value));
        if leaked {
            cross_line_leak_count += 1;
            push_quality_issue(
                &mut issues,
                &mut issue_keys,
                Step5QualityIssue {
                    rule_id: "cross_line_leak".to_string(),
                    severity: "hard".to_string(),
                    segment_id: current.segment_id,
                    part_id: 0,
                    message: "当前句疑似提前翻译下一句信息".to_string(),
                },
            );
        }
    }

    let hard_fail_count = issues
        .iter()
        .filter(|issue| issue.severity == "hard")
        .count();
    let segment_total = request.segments.len();
    let long_line_penalty = gt25_count as f64 * 1.2 + gt32_count as f64 * 2.5;
    let hard_penalty = hard_fail_count as f64 * 20.0;
    let soft_penalty = issues
        .iter()
        .filter(|issue| issue.severity == "soft")
        .count() as f64
        * 1.5;
    let mut soft_score = 100.0 - hard_penalty - long_line_penalty - soft_penalty;
    if soft_score < 0.0 {
        soft_score = 0.0;
    }

    Ok(BuildStep6FinalCheckResponse {
        passed: hard_fail_count == 0,
        hard_fail_count,
        soft_score: (soft_score * 10.0).round() / 10.0,
        issue_count: issues.len(),
        issues,
        metrics: Step6FinalCheckMetrics {
            segment_total,
            empty_count,
            ellipsis_tail_count,
            numeric_drift_count,
            cross_line_leak_count,
            gt25_count,
            gt32_count,
        },
    })
}

pub fn merge_watchability_subtitle_srt_segments(
    segments: &mut Vec<crate::services::subtitle_srt::SubtitleSrtSegment>,
    subtitle_length_reference: u32,
    target_lang: &str,
) {
    let original_segments = segments.clone();
    let mut step_segments = segments
        .iter()
        .enumerate()
        .map(|(index, segment)| Step5FinalSegment {
            segment_id: index + 1,
            start: segment.start_ms as f64 / 1000.0,
            end: segment.end_ms.max(segment.start_ms) as f64 / 1000.0,
            source: normalize_inline_text(&segment.source_text),
            translation: normalize_inline_text(&segment.translated_text),
            tokens: Vec::new(),
        })
        .collect::<Vec<_>>();

    merge_watchability_fragments(&mut step_segments, subtitle_length_reference, target_lang);

    *segments = step_segments
        .into_iter()
        .map(
            |segment| crate::services::subtitle_srt::SubtitleSrtSegment {
                start_ms: seconds_to_millis(segment.start),
                end_ms: seconds_to_millis(segment.end.max(segment.start)),
                source_text: original_segments
                    .get(segment.segment_id.saturating_sub(1))
                    .filter(|original| {
                        seconds_to_millis(segment.start) == original.start_ms
                            && seconds_to_millis(segment.end.max(segment.start)) == original.end_ms
                    })
                    .map(|original| original.source_text.clone())
                    .unwrap_or(segment.source),
                translated_text: segment.translation,
            },
        )
        .collect();
}

fn seconds_to_millis(value: f64) -> u64 {
    if !value.is_finite() || value <= 0.0 {
        return 0;
    }
    (value * 1000.0).round() as u64
}

fn push_quality_issue(
    issues: &mut Vec<Step5QualityIssue>,
    issue_keys: &mut HashSet<String>,
    issue: Step5QualityIssue,
) {
    let key = format!(
        "{}|{}|{}|{}|{}",
        issue.rule_id, issue.severity, issue.segment_id, issue.part_id, issue.message
    );
    if issue_keys.insert(key) {
        issues.push(issue);
    }
}

fn validate_step5_align_request(request: &BuildStep5TranslationAlignRequest) -> Result<(), String> {
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
    if request.parents.is_empty() {
        return Err("parents is required".to_string());
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

fn validate_step5_polish_request(
    request: &BuildStep5TranslationPolishRequest,
) -> Result<(), String> {
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
    if request.parents.is_empty() {
        return Err("parents is required".to_string());
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

fn build_single_split_part(segment: &Step5DraftSegment) -> Step5SplitPart {
    let source = if !segment.tokens.is_empty() {
        build_source_from_tokens(&segment.tokens)
    } else {
        normalize_inline_text(&segment.source)
    };
    Step5SplitPart {
        part_id: 1,
        start: segment.start,
        end: segment.end.max(segment.start),
        source,
        tokens: segment.tokens.clone(),
    }
}

fn build_split_parts_from_ranges(
    segment: &Step5DraftSegment,
    ranges: &[(usize, usize)],
) -> Vec<Step5SplitPart> {
    ranges
        .iter()
        .enumerate()
        .map(|(index, (start_idx, end_idx))| {
            let tokens = segment.tokens[*start_idx..=*end_idx].to_vec();
            let part_start = tokens
                .first()
                .map(|token| token.start)
                .unwrap_or(segment.start);
            let part_end = tokens
                .last()
                .map(|token| token.end)
                .unwrap_or(segment.end.max(segment.start));
            let source = build_source_from_tokens(&tokens);
            Step5SplitPart {
                part_id: index + 1,
                start: part_start,
                end: part_end.max(part_start),
                source: if source.is_empty() {
                    normalize_inline_text(&segment.source)
                } else {
                    source
                },
                tokens,
            }
        })
        .collect::<Vec<_>>()
}

fn hard_pause_boundaries(tokens: &[Step5Token]) -> Vec<usize> {
    if tokens.len() < 2 {
        return Vec::new();
    }
    let mut out = Vec::<usize>::new();
    for index in 0..tokens.len() - 1 {
        let current = &tokens[index];
        let next = &tokens[index + 1];
        let gap = (next.start - current.end).max(0.0);
        if gap >= HARD_SPLIT_GAP_SECONDS {
            out.push(index + 1);
        }
    }
    out
}

fn ranges_to_boundary_ids(ranges: &[(usize, usize)]) -> Vec<usize> {
    if ranges.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::<usize>::new();
    for (index, (_start, end)) in ranges.iter().enumerate() {
        if index + 1 >= ranges.len() {
            continue;
        }
        out.push(end + 1);
    }
    out
}

fn boundary_ids_to_ranges(boundaries: &[usize], token_len: usize) -> Vec<(usize, usize)> {
    if token_len == 0 {
        return Vec::new();
    }
    let mut sorted = boundaries
        .iter()
        .copied()
        .filter(|id| *id >= 1 && *id < token_len)
        .collect::<Vec<_>>();
    sorted.sort_unstable();
    sorted.dedup();
    let mut ranges = Vec::<(usize, usize)>::new();
    let mut start = 0usize;
    for boundary in sorted {
        let end = boundary.saturating_sub(1);
        if end < start {
            continue;
        }
        ranges.push((start, end));
        start = boundary;
    }
    if start < token_len {
        ranges.push((start, token_len - 1));
    }
    ranges
}

fn choose_preferred_split_ranges(
    llm_ranges: Vec<(usize, usize)>,
    fallback_ranges: Vec<(usize, usize)>,
    tokens: &[Step5Token],
    source_lang: &str,
    source_limit: f64,
) -> Vec<(usize, usize)> {
    if llm_ranges.is_empty() {
        return fallback_ranges;
    }
    if fallback_ranges.is_empty() {
        return llm_ranges;
    }
    let llm_score = score_split_ranges(&llm_ranges, tokens, source_lang, source_limit);
    let fallback_score = score_split_ranges(&fallback_ranges, tokens, source_lang, source_limit);
    if llm_score <= fallback_score * 1.05 {
        llm_ranges
    } else {
        fallback_ranges
    }
}

fn score_split_ranges(
    ranges: &[(usize, usize)],
    tokens: &[Step5Token],
    source_lang: &str,
    source_limit: f64,
) -> f64 {
    if ranges.is_empty() {
        return 1_000_000.0;
    }
    let mut score = 0.0f64;
    let mut lengths = Vec::<f64>::new();
    for (start, end) in ranges {
        if *start >= tokens.len() || *end >= tokens.len() || end < start {
            score += 1000.0;
            continue;
        }
        let text = build_source_from_tokens(&tokens[*start..=*end]);
        let units = text_length_units(&text, source_lang);
        lengths.push(units);
        if units > source_limit {
            score += 80.0 + (units - source_limit) * 20.0;
        }
        if units < 4.0 {
            score += (4.0 - units) * 25.0 + 20.0;
        }
        if units > source_limit * 1.6 {
            score += 120.0;
        }
    }
    for window in ranges.windows(2) {
        let left = window[0];
        let right = window[1];
        let Some(left_token) = tokens.get(left.1) else {
            continue;
        };
        let Some(right_token) = tokens.get(right.0) else {
            continue;
        };
        let gap = (right_token.start - left_token.end).max(0.0);
        if gap >= SOFT_SPLIT_GAP_SECONDS || ends_with_sentence_punctuation(&left_token.text) {
            score -= 2.0;
        } else {
            score += 4.0;
        }
    }
    if lengths.len() >= 2 {
        let avg = lengths.iter().sum::<f64>() / lengths.len() as f64;
        if avg > 0.0 {
            for len in &lengths {
                let ratio = len / avg;
                if ratio > 2.4 || ratio < 0.35 {
                    score += 16.0;
                }
            }
        }
    }
    score
}

fn merge_tiny_ranges_for_readability(
    mut ranges: Vec<(usize, usize)>,
    tokens: &[Step5Token],
    source_lang: &str,
    source_limit: f64,
    mandatory_boundaries: &[usize],
) -> Vec<(usize, usize)> {
    if ranges.len() <= 1 || tokens.is_empty() {
        return ranges;
    }

    let mandatory_set = mandatory_boundaries
        .iter()
        .copied()
        .collect::<std::collections::HashSet<_>>();

    let mut changed = true;
    while changed && ranges.len() > 1 {
        changed = false;
        let mut idx = 0usize;
        while idx < ranges.len() {
            let (start, end) = ranges[idx];
            if start >= tokens.len() || end >= tokens.len() || end < start {
                idx += 1;
                continue;
            }

            let units = tokens[start..=end]
                .iter()
                .map(|token| text_length_units(&token.text, source_lang))
                .sum::<f64>();
            let duration = segment_duration_seconds(tokens, start, end);
            let too_short = duration < HARD_MIN_SEGMENT_DURATION_SECONDS
                || (units < MIN_READABLE_UNITS && duration < MIN_READABLE_DURATION_SECONDS);
            if !too_short {
                idx += 1;
                continue;
            }

            let left_boundary = if idx > 0 { Some(start) } else { None };
            let right_boundary = if idx + 1 < ranges.len() {
                Some(end + 1)
            } else {
                None
            };
            let can_merge_left = left_boundary
                .map(|b| !mandatory_set.contains(&b))
                .unwrap_or(false);
            let can_merge_right = right_boundary
                .map(|b| !mandatory_set.contains(&b))
                .unwrap_or(false);

            let mut merged = false;
            if can_merge_left && can_merge_right {
                let left_score = merge_penalty_with_neighbor(
                    ranges[idx - 1],
                    ranges[idx],
                    tokens,
                    source_lang,
                    source_limit,
                );
                let right_score = merge_penalty_with_neighbor(
                    ranges[idx],
                    ranges[idx + 1],
                    tokens,
                    source_lang,
                    source_limit,
                );
                if left_score <= right_score {
                    ranges[idx - 1] = (ranges[idx - 1].0, ranges[idx].1);
                    ranges.remove(idx);
                } else {
                    ranges[idx + 1] = (ranges[idx].0, ranges[idx + 1].1);
                    ranges.remove(idx);
                }
                merged = true;
            } else if can_merge_left {
                ranges[idx - 1] = (ranges[idx - 1].0, ranges[idx].1);
                ranges.remove(idx);
                merged = true;
            } else if can_merge_right {
                ranges[idx + 1] = (ranges[idx].0, ranges[idx + 1].1);
                ranges.remove(idx);
                merged = true;
            }

            if merged {
                changed = true;
                break;
            }
            idx += 1;
        }
    }

    ranges
}

fn rebalance_dangling_tail_tokens(
    mut ranges: Vec<(usize, usize)>,
    tokens: &[Step5Token],
    source_lang: &str,
    source_limit: f64,
    mandatory_boundaries: &[usize],
) -> Vec<(usize, usize)> {
    if ranges.len() <= 1 || tokens.is_empty() {
        return ranges;
    }

    let probe_text = tokens
        .iter()
        .take(24)
        .map(|token| token.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    if use_char_units(source_lang, &probe_text) {
        return ranges;
    }

    let mandatory_set = mandatory_boundaries.iter().copied().collect::<HashSet<_>>();
    let mut changed = true;
    while changed {
        changed = false;
        for index in 0..ranges.len().saturating_sub(1) {
            let (left_start, left_end) = ranges[index];
            let (right_start, right_end) = ranges[index + 1];
            if left_end + 1 != right_start {
                continue;
            }
            if mandatory_set.contains(&right_start) {
                continue;
            }
            let move_count = trailing_dangling_token_count(tokens, left_start, left_end);
            if move_count == 0 || left_end < left_start + move_count {
                continue;
            }
            let new_left_end = left_end - move_count;
            let new_right_start = right_start - move_count;
            if new_right_start > right_end || new_left_end < left_start {
                continue;
            }

            let left_units = tokens[left_start..=new_left_end]
                .iter()
                .map(|token| text_length_units(&token.text, source_lang))
                .sum::<f64>();
            if left_units < 2.0 {
                continue;
            }
            let right_units = tokens[new_right_start..=right_end]
                .iter()
                .map(|token| text_length_units(&token.text, source_lang))
                .sum::<f64>();
            if right_units > source_limit * 1.45 {
                continue;
            }

            ranges[index] = (left_start, new_left_end);
            ranges[index + 1] = (new_right_start, right_end);
            changed = true;
        }
    }

    ranges
}

fn trailing_dangling_token_count(tokens: &[Step5Token], start: usize, end: usize) -> usize {
    if end <= start {
        return 0;
    }
    let Some(last_token) = tokens.get(end) else {
        return 0;
    };
    if ends_with_sentence_punctuation(&last_token.text) {
        return 0;
    }
    let last_word = normalize_ascii_token_word(&last_token.text);
    if last_word.is_empty() {
        return 0;
    }

    let prev_word = end
        .checked_sub(1)
        .and_then(|idx| tokens.get(idx))
        .map(|token| normalize_ascii_token_word(&token.text))
        .unwrap_or_default();

    if is_dangling_tail_word(&last_word) {
        if !prev_word.is_empty() && is_dangling_tail_word(&prev_word) {
            return 2;
        }
        return 1;
    }
    if prev_word == "to" && looks_like_content_word(&last_word) {
        return 2;
    }
    0
}

fn normalize_ascii_token_word(raw: &str) -> String {
    raw.chars()
        .filter(|ch| ch.is_ascii_alphabetic())
        .map(|ch| ch.to_ascii_lowercase())
        .collect::<String>()
}

fn looks_like_content_word(word: &str) -> bool {
    word.len() >= 2 && !is_dangling_tail_word(word)
}

fn is_dangling_tail_word(word: &str) -> bool {
    matches!(
        word,
        "a" | "an"
            | "and"
            | "as"
            | "at"
            | "because"
            | "before"
            | "but"
            | "by"
            | "for"
            | "from"
            | "he"
            | "her"
            | "his"
            | "i"
            | "if"
            | "in"
            | "into"
            | "it"
            | "its"
            | "my"
            | "of"
            | "on"
            | "or"
            | "our"
            | "she"
            | "so"
            | "that"
            | "the"
            | "their"
            | "them"
            | "then"
            | "these"
            | "they"
            | "this"
            | "those"
            | "to"
            | "we"
            | "when"
            | "where"
            | "which"
            | "while"
            | "who"
            | "with"
            | "you"
            | "your"
    )
}

fn segment_duration_seconds(tokens: &[Step5Token], start: usize, end: usize) -> f64 {
    let start_time = tokens.get(start).map(|t| t.start).unwrap_or(0.0);
    let end_time = tokens.get(end).map(|t| t.end).unwrap_or(start_time);
    (end_time - start_time).max(0.0)
}

fn merge_penalty_with_neighbor(
    left: (usize, usize),
    right: (usize, usize),
    tokens: &[Step5Token],
    source_lang: &str,
    source_limit: f64,
) -> f64 {
    let start = left.0.min(right.0);
    let end = left.1.max(right.1);
    if start >= tokens.len() || end >= tokens.len() || end < start {
        return 1_000_000.0;
    }
    let units = tokens[start..=end]
        .iter()
        .map(|token| text_length_units(&token.text, source_lang))
        .sum::<f64>();
    if units <= source_limit {
        return units;
    }
    units + (units - source_limit) * 20.0
}

fn enforce_source_limit_ranges(
    mut ranges: Vec<(usize, usize)>,
    tokens: &[Step5Token],
    source_lang: &str,
    source_limit: f64,
) -> Vec<(usize, usize)> {
    if ranges.is_empty() || tokens.is_empty() || source_limit <= 0.0 {
        return ranges;
    }
    let hard_limit = (source_limit * FORCE_SPLIT_MARGIN).max(1.0);
    let max_rounds = tokens.len().max(1);

    for _ in 0..max_rounds {
        let mut changed = false;
        let mut next_ranges = Vec::<(usize, usize)>::new();
        for (start, end) in ranges.iter().copied() {
            if start >= tokens.len() || end >= tokens.len() || end < start {
                continue;
            }
            let units = tokens[start..=end]
                .iter()
                .map(|token| text_length_units(&token.text, source_lang))
                .sum::<f64>();
            if units <= hard_limit || end == start {
                next_ranges.push((start, end));
                continue;
            }
            if let Some(split_after) =
                pick_force_split_after(start, end, tokens, source_lang, source_limit)
            {
                next_ranges.push((start, split_after));
                next_ranges.push((split_after + 1, end));
                changed = true;
            } else {
                next_ranges.push((start, end));
            }
        }
        ranges = next_ranges;
        if !changed {
            break;
        }
    }

    ranges
}

fn pick_force_split_after(
    start: usize,
    end: usize,
    tokens: &[Step5Token],
    source_lang: &str,
    source_limit: f64,
) -> Option<usize> {
    if end <= start {
        return None;
    }
    let mut total_units = 0.0f64;
    for token in &tokens[start..=end] {
        total_units += text_length_units(&token.text, source_lang);
    }
    if total_units <= source_limit {
        return None;
    }

    let target = (total_units / 2.0).min(source_limit);
    let mut best = None::<(usize, f64)>;
    let mut left_units = 0.0f64;
    for idx in start..end {
        left_units += text_length_units(&tokens[idx].text, source_lang);
        let right_units = (total_units - left_units).max(0.0);
        if left_units <= 0.0 || right_units <= 0.0 {
            continue;
        }
        let mut penalty = (left_units - target).abs() + (right_units - target).abs();
        if left_units > source_limit {
            penalty += (left_units - source_limit) * 8.0;
        }
        if right_units > source_limit {
            penalty += (right_units - source_limit) * 8.0;
        }
        let gap = (tokens[idx + 1].start - tokens[idx].end).max(0.0);
        if gap >= SOFT_SPLIT_GAP_SECONDS || ends_with_sentence_punctuation(&tokens[idx].text) {
            penalty -= 0.8;
        }
        match best {
            Some((_best_idx, best_penalty)) if best_penalty <= penalty => {}
            _ => {
                best = Some((idx, penalty));
            }
        }
    }
    best.map(|(idx, _)| idx)
}

fn normalize_split_boundaries(
    candidate_boundaries: &[usize],
    token_count: usize,
    mandatory_boundaries: &[usize],
    fallback_boundaries: &[usize],
    min_parts: usize,
) -> Vec<usize> {
    if token_count <= 1 {
        return Vec::new();
    }
    let mut boundaries = candidate_boundaries
        .iter()
        .copied()
        .filter(|id| *id >= 1 && *id < token_count)
        .collect::<Vec<_>>();
    boundaries.extend(
        mandatory_boundaries
            .iter()
            .copied()
            .filter(|id| *id >= 1 && *id < token_count),
    );
    boundaries.sort_unstable();
    boundaries.dedup();

    let mut mandatory_sorted = mandatory_boundaries
        .iter()
        .copied()
        .filter(|id| *id >= 1 && *id < token_count)
        .collect::<Vec<_>>();
    mandatory_sorted.sort_unstable();
    mandatory_sorted.dedup();
    let mandatory_set = mandatory_sorted
        .iter()
        .copied()
        .collect::<std::collections::HashSet<_>>();

    let required_boundaries = min_parts.saturating_sub(1);
    if boundaries.len() < required_boundaries {
        for id in fallback_boundaries
            .iter()
            .copied()
            .filter(|id| *id >= 1 && *id < token_count)
        {
            if boundaries.len() >= required_boundaries {
                break;
            }
            if !boundaries.contains(&id) {
                boundaries.push(id);
            }
        }
    }
    boundaries.sort_unstable();
    boundaries.dedup();

    let max_parts = (required_boundaries + 3).max(min_parts).min(token_count);
    let max_boundaries = max_parts.saturating_sub(1);
    if boundaries.len() > max_boundaries {
        let mut pruned = mandatory_sorted.clone();
        for id in fallback_boundaries
            .iter()
            .copied()
            .filter(|id| *id >= 1 && *id < token_count)
        {
            if pruned.len() >= max_boundaries {
                break;
            }
            if !pruned.contains(&id) {
                pruned.push(id);
            }
        }
        for id in boundaries.iter().copied() {
            if pruned.len() >= max_boundaries {
                break;
            }
            if !mandatory_set.contains(&id) && !pruned.contains(&id) {
                pruned.push(id);
            }
        }
        boundaries = pruned;
        boundaries.sort_unstable();
        boundaries.dedup();
    }

    if boundaries.len() < required_boundaries {
        let mut ranges = boundary_ids_to_ranges(&boundaries, token_count);
        let fake_tokens = (0..token_count)
            .map(|idx| Step5Token {
                text: idx.to_string(),
                start: idx as f64,
                end: idx as f64,
            })
            .collect::<Vec<_>>();
        ensure_min_split_ranges(&mut ranges, min_parts, &fake_tokens, "en");
        boundaries = ranges_to_boundary_ids(&ranges);
        for id in mandatory_sorted {
            if !boundaries.contains(&id) {
                boundaries.push(id);
            }
        }
        boundaries.sort_unstable();
        boundaries.dedup();
    }

    boundaries
}

fn map_source_parts_to_boundaries(
    source_parts: &[String],
    tokens: &[Step5Token],
    source_lang: &str,
) -> Vec<usize> {
    if source_parts.len() <= 1 || tokens.len() <= 1 {
        return Vec::new();
    }
    let token_units = tokens
        .iter()
        .map(|token| text_length_units(&token.text, source_lang).max(0.5))
        .collect::<Vec<_>>();
    let mut prefix_units = Vec::<f64>::with_capacity(token_units.len() + 1);
    prefix_units.push(0.0);
    for unit in &token_units {
        let prev = *prefix_units.last().unwrap_or(&0.0);
        prefix_units.push(prev + *unit);
    }

    let mut part_units = source_parts
        .iter()
        .map(|part| text_length_units(part, source_lang).max(1.0))
        .collect::<Vec<_>>();
    let total_part_units = part_units.iter().sum::<f64>();
    let total_token_units = *prefix_units.last().unwrap_or(&0.0);
    if total_part_units > 0.0 && total_token_units > 0.0 {
        let scale = total_token_units / total_part_units;
        for unit in &mut part_units {
            *unit *= scale;
        }
    }

    let mut boundaries = Vec::<usize>::new();
    let mut start = 0usize;
    let mut consumed_target = 0.0f64;
    let boundary_count = source_parts.len().saturating_sub(1);
    for boundary_idx in 0..boundary_count {
        consumed_target += part_units
            .get(boundary_idx)
            .copied()
            .unwrap_or(1.0)
            .max(0.5);
        let remaining_boundaries = boundary_count.saturating_sub(boundary_idx + 1);
        let min_boundary = start.saturating_add(1);
        let max_boundary = tokens.len().saturating_sub(remaining_boundaries + 1);
        if min_boundary > max_boundary {
            break;
        }

        let mut best_boundary = min_boundary;
        let mut best_score = f64::MAX;
        for boundary in min_boundary..=max_boundary {
            let consumed_units = prefix_units.get(boundary).copied().unwrap_or(0.0);
            let mut score = (consumed_units - consumed_target).abs();

            if let (Some(left), Some(right)) =
                (tokens.get(boundary.saturating_sub(1)), tokens.get(boundary))
            {
                let gap = (right.start - left.end).max(0.0);
                if gap >= SOFT_SPLIT_GAP_SECONDS || ends_with_sentence_punctuation(&left.text) {
                    score -= 0.8;
                } else {
                    score += 0.5;
                }
            }

            let left_units = consumed_units - prefix_units.get(start).copied().unwrap_or(0.0);
            let right_units = total_token_units - consumed_units;
            if left_units < 1.5 {
                score += 3.0;
            }
            if right_units < 1.5 {
                score += 3.0;
            }

            if score < best_score {
                best_score = score;
                best_boundary = boundary;
            }
        }
        boundaries.push(best_boundary);
        start = best_boundary;
    }
    boundaries.sort_unstable();
    boundaries.dedup();
    boundaries
}

fn split_token_ranges(
    tokens: &[Step5Token],
    source_lang: &str,
    source_limit: f64,
    target_limit: f64,
    source_units: f64,
    target_units: f64,
) -> Vec<(usize, usize)> {
    if tokens.is_empty() {
        return Vec::new();
    }
    if tokens.len() == 1 {
        return vec![(0, 0)];
    }

    let desired_parts = desired_split_parts(source_units, source_limit, target_units, target_limit);
    let dynamic_soft_limit = if desired_parts <= 1 {
        source_limit
    } else {
        (source_units / desired_parts as f64)
            .max(1.0)
            .min(source_limit.max(1.0))
    };

    let mut out = Vec::<(usize, usize)>::new();
    let mut chunk_start = 0usize;
    let mut chunk_units = 0.0f64;

    for index in 0..tokens.len() - 1 {
        let current = &tokens[index];
        let next = &tokens[index + 1];
        chunk_units += text_length_units(&current.text, source_lang);
        let current_len = index + 1 - chunk_start;
        let gap = (next.start - current.end).max(0.0);
        let hard_split = gap >= HARD_SPLIT_GAP_SECONDS;
        let can_soft_split = current_len >= MIN_TOKENS_FOR_SOFT_SPLIT
            && chunk_units >= dynamic_soft_limit
            && (gap >= SOFT_SPLIT_GAP_SECONDS || ends_with_sentence_punctuation(&current.text));
        let force_split = chunk_units >= source_limit * FORCE_SPLIT_MARGIN
            && current_len >= (MIN_TOKENS_FOR_SOFT_SPLIT / 2).max(2);

        if hard_split || can_soft_split || force_split {
            out.push((chunk_start, index));
            chunk_start = index + 1;
            chunk_units = 0.0;
        }
    }
    out.push((chunk_start, tokens.len() - 1));

    let mut out = out
        .into_iter()
        .filter(|(start, end)| end >= start)
        .collect::<Vec<_>>();
    ensure_min_split_ranges(&mut out, desired_parts, tokens, source_lang);
    out
}

fn desired_split_parts(
    source_units: f64,
    source_limit: f64,
    target_units: f64,
    target_limit: f64,
) -> usize {
    let source_parts = if source_limit <= 0.0 {
        1usize
    } else {
        (source_units / source_limit).ceil().max(1.0) as usize
    };
    let target_parts = if target_limit <= 0.0 {
        1usize
    } else {
        (target_units / target_limit).ceil().max(1.0) as usize
    };
    source_parts.max(target_parts).max(1)
}

fn ensure_min_split_ranges(
    ranges: &mut Vec<(usize, usize)>,
    desired_parts: usize,
    tokens: &[Step5Token],
    source_lang: &str,
) {
    if desired_parts <= 1 || ranges.is_empty() {
        return;
    }
    while ranges.len() < desired_parts {
        let mut best_index = None::<usize>;
        let mut best_score = 0.0f64;
        for (idx, (start, end)) in ranges.iter().enumerate() {
            if end <= start {
                continue;
            }
            let unit_len = tokens[*start..=*end]
                .iter()
                .map(|token| text_length_units(&token.text, source_lang))
                .sum::<f64>();
            if unit_len > best_score {
                best_score = unit_len;
                best_index = Some(idx);
            }
        }
        let Some(idx) = best_index else {
            break;
        };
        let (start, end) = ranges[idx];
        if end <= start {
            break;
        }
        let mid = start + (end - start) / 2;
        if mid < start || mid >= end {
            break;
        }
        ranges[idx] = (start, mid);
        ranges.insert(idx + 1, (mid + 1, end));
    }
}

fn build_source_from_tokens(tokens: &[Step5Token]) -> String {
    let mut out = String::new();
    let mut prev_has_spacing_word = false;
    let mut prev_allows_space_after = false;

    for token in tokens {
        let text = token.text.trim();
        if text.is_empty() {
            continue;
        }
        let next_has_spacing_word = source_token_has_spacing_word(text);
        if !out.is_empty()
            && next_has_spacing_word
            && !source_token_starts_attached(text)
            && (prev_has_spacing_word || prev_allows_space_after)
        {
            out.push(' ');
        }
        out.push_str(text);
        prev_has_spacing_word = next_has_spacing_word;
        prev_allows_space_after = source_token_allows_space_after(text);
    }
    normalize_inline_text(&out)
}

fn source_token_has_spacing_word(token: &str) -> bool {
    token
        .chars()
        .any(|ch| ch.is_ascii_alphanumeric() || is_hangul_char(ch))
}

fn source_token_starts_attached(token: &str) -> bool {
    token
        .chars()
        .next()
        .map(|ch| ch == '\'' || ch == '’' || ch.is_ascii_punctuation())
        .unwrap_or(false)
}

fn source_token_allows_space_after(token: &str) -> bool {
    token
        .chars()
        .last()
        .map(|ch| {
            matches!(
                ch,
                ',' | ';' | ':' | '?' | '!' | '.' | '，' | '；' | '：' | '？' | '！' | '。'
            )
        })
        .unwrap_or(false)
}

fn heuristic_split_translation(
    text: &str,
    expected_count: usize,
    part_sources: Option<&[Step5SplitPart]>,
) -> Vec<String> {
    let normalized = normalize_inline_text(text);
    if expected_count <= 1 {
        return vec![normalized];
    }
    if normalized.is_empty() {
        return vec![String::new(); expected_count];
    }

    let clauses = split_clauses(&normalized);
    if clauses.is_empty() {
        return vec![normalized];
    }
    let mut candidates = Vec::<Vec<String>>::new();
    let clause_bucketed = bucket_split_clauses(&clauses, expected_count);
    if !clause_bucketed.is_empty() {
        candidates.push(clause_bucketed);
    }

    let weights = part_sources
        .filter(|parts| parts.len() == expected_count)
        .map(weighted_source_units_from_parts)
        .unwrap_or_else(|| vec![1.0; expected_count]);

    if let Some(clause_weighted) =
        split_translation_by_clause_weights(&clauses, expected_count, &weights)
    {
        candidates.push(clause_weighted);
    }
    candidates.push(split_translation_evenly_by_weights(
        &normalized,
        expected_count,
        &weights,
    ));

    let mut best = candidates
        .first()
        .cloned()
        .unwrap_or_else(|| vec![normalized.clone(); expected_count]);
    let mut best_score = split_line_quality_score(&best);
    for candidate in candidates.into_iter().skip(1) {
        if candidate.len() != expected_count {
            continue;
        }
        let score = split_line_quality_score(&candidate);
        if score > best_score {
            best = candidate;
            best_score = score;
        }
    }
    if has_empty_or_duplicated_long_line(&best) {
        return split_translation_evenly_by_weights(&normalized, expected_count, &weights);
    }
    best
}

fn bucket_split_clauses(clauses: &[String], expected_count: usize) -> Vec<String> {
    if expected_count == 0 {
        return Vec::new();
    }
    if clauses.is_empty() {
        return vec![String::new(); expected_count];
    }
    let clauses_total = clauses.len().max(1);
    let mut out = vec![String::new(); expected_count];
    for (index, clause) in clauses.iter().enumerate() {
        let bucket = index * expected_count / clauses_total;
        let target = bucket.min(expected_count - 1);
        if out[target].is_empty() {
            out[target] = clause.clone();
        } else {
            out[target].push(' ');
            out[target].push_str(clause);
        }
    }
    out.into_iter()
        .map(|line| normalize_inline_text(&line))
        .collect()
}

fn split_translation_by_clause_weights(
    clauses: &[String],
    expected_count: usize,
    weights: &[f64],
) -> Option<Vec<String>> {
    if expected_count == 0 || clauses.is_empty() || clauses.len() < expected_count {
        return None;
    }
    let mut normalized_weights = if weights.len() == expected_count {
        weights.to_vec()
    } else {
        vec![1.0; expected_count]
    };
    for value in &mut normalized_weights {
        if !value.is_finite() || *value <= 0.0 {
            *value = 1.0;
        }
    }
    let weight_total = normalized_weights.iter().sum::<f64>().max(1.0);
    let clause_units = clauses
        .iter()
        .map(|clause| count_word_units(clause).max(1) as f64)
        .collect::<Vec<_>>();
    let units_total = clause_units.iter().sum::<f64>().max(expected_count as f64);
    let target_units = normalized_weights
        .iter()
        .map(|weight| units_total * (*weight / weight_total))
        .collect::<Vec<_>>();

    let n = clauses.len();
    let m = expected_count;
    let mut prefix = vec![0.0f64; n + 1];
    for index in 0..n {
        prefix[index + 1] = prefix[index] + clause_units[index];
    }
    let neg_inf = -1.0e18f64;
    let mut dp = vec![vec![neg_inf; n + 1]; m + 1];
    let mut prev = vec![vec![usize::MAX; n + 1]; m + 1];
    dp[0][0] = 0.0;

    for part in 1..=m {
        for end in part..=n {
            let min_start = part - 1;
            let max_start = end - 1;
            for start in min_start..=max_start {
                if dp[part - 1][start] <= neg_inf / 2.0 {
                    continue;
                }
                let segment_units = prefix[end] - prefix[start];
                let segment_text = clauses[start..end].join(" ");
                let segment_text = normalize_inline_text(&segment_text);
                let mut segment_score = -((segment_units - target_units[part - 1]).abs() * 4.0);
                segment_score -= line_fragment_penalty(&segment_text) as f64 * 1.5;
                if segment_text.is_empty() {
                    segment_score -= 40.0;
                }
                let candidate = dp[part - 1][start] + segment_score;
                if candidate > dp[part][end] {
                    dp[part][end] = candidate;
                    prev[part][end] = start;
                }
            }
        }
    }
    if dp[m][n] <= neg_inf / 2.0 {
        return None;
    }
    let mut boundaries = Vec::<(usize, usize)>::with_capacity(m);
    let mut part = m;
    let mut end = n;
    while part > 0 {
        let start = prev[part][end];
        if start == usize::MAX || start >= end {
            return None;
        }
        boundaries.push((start, end));
        end = start;
        part -= 1;
    }
    boundaries.reverse();
    if boundaries.len() != expected_count {
        return None;
    }
    let out = boundaries
        .into_iter()
        .map(|(start, end)| normalize_inline_text(&clauses[start..end].join(" ")))
        .collect::<Vec<_>>();
    Some(out)
}

fn weighted_source_units_from_parts(parts: &[Step5SplitPart]) -> Vec<f64> {
    let mut weights = parts
        .iter()
        .map(|part| count_word_units(&part.source).max(1) as f64)
        .collect::<Vec<_>>();
    if weights.is_empty() {
        return weights;
    }
    if weights
        .iter()
        .all(|value| *value <= 0.0 || !value.is_finite())
    {
        weights.fill(1.0);
    }
    weights
}

fn split_translation_evenly_by_weights(
    text: &str,
    expected_count: usize,
    weights: &[f64],
) -> Vec<String> {
    if expected_count == 0 {
        return Vec::new();
    }
    let normalized = normalize_inline_text(text);
    if normalized.is_empty() {
        return vec![String::new(); expected_count];
    }

    let words = normalized
        .split_whitespace()
        .map(|value| value.to_string())
        .collect::<Vec<_>>();
    let (tokens, join_with_space) = if words.len() >= expected_count {
        (words, true)
    } else {
        let chars = normalized
            .chars()
            .filter(|ch| !ch.is_whitespace())
            .map(|ch| ch.to_string())
            .collect::<Vec<_>>();
        if chars.is_empty() {
            (words, true)
        } else {
            (chars, false)
        }
    };

    let token_total = tokens.len();
    if token_total == 0 {
        return vec![String::new(); expected_count];
    }

    if token_total < expected_count {
        let mut out = vec![String::new(); expected_count];
        for index in 0..token_total {
            out[index] = tokens[index].clone();
        }
        return out
            .into_iter()
            .map(|line| normalize_inline_text(&line))
            .collect();
    }

    let mut normalized_weights = if weights.len() == expected_count {
        weights.to_vec()
    } else {
        vec![1.0; expected_count]
    };
    for value in &mut normalized_weights {
        if !value.is_finite() || *value <= 0.0 {
            *value = 1.0;
        }
    }

    let mut remaining_tokens = token_total;
    let mut remaining_weight = normalized_weights.iter().sum::<f64>().max(1.0);
    let mut start = 0usize;
    let mut out = vec![String::new(); expected_count];
    for index in 0..expected_count {
        let remaining_slots = expected_count - index;
        let take = if remaining_slots <= 1 {
            remaining_tokens
        } else {
            let weight = normalized_weights[index];
            let ideal = ((remaining_tokens as f64) * (weight / remaining_weight)).round() as usize;
            let min_take = 1usize;
            let max_take = remaining_tokens.saturating_sub(remaining_slots - 1);
            ideal.clamp(min_take, max_take.max(min_take))
        };
        let end = start + take.min(token_total.saturating_sub(start));
        let text = if join_with_space {
            tokens[start..end].join(" ")
        } else {
            tokens[start..end].join("")
        };
        out[index] = normalize_inline_text(&text);
        start = end;
        remaining_tokens = remaining_tokens.saturating_sub(take);
        remaining_weight = (remaining_weight - normalized_weights[index]).max(1.0);
    }
    out
}

fn split_watchability_overlong_segments(
    segments: &mut Vec<Step5FinalSegment>,
    split_trigger: f64,
    target_lang: &str,
) {
    if segments.is_empty() || split_trigger <= 0.0 {
        return;
    }

    let mut split_segments = Vec::<Step5FinalSegment>::new();
    for segment in segments.drain(..) {
        split_segments.push(segment);
    }

    let mut output = Vec::<Step5FinalSegment>::new();
    let mut work_queue = split_segments;
    while let Some(segment) = work_queue.pop() {
        let target_len = text_length_units(&segment.translation, target_lang);
        if target_len <= split_trigger {
            output.push(segment);
            continue;
        }

        let Some((left, right)) = split_long_final_segment_for_watchability(&segment, target_lang)
        else {
            output.push(segment);
            continue;
        };

        if !is_safe_watchability_split_part(&left, target_lang)
            || !is_safe_watchability_split_part(&right, target_lang)
        {
            output.push(segment);
            continue;
        }

        let left_len = text_length_units(&left.translation, target_lang);
        let right_len = text_length_units(&right.translation, target_lang);
        if left_len <= 0.0 || right_len <= 0.0 {
            output.push(left);
            output.push(right);
            continue;
        }

        if left_len > split_trigger || right_len > split_trigger {
            work_queue.push(left);
            work_queue.push(right);
        } else {
            work_queue.push(right);
            work_queue.push(left);
        }
    }

    output.sort_by(|a, b| a.start.total_cmp(&b.start));
    for (index, segment) in output.iter_mut().enumerate() {
        segment.segment_id = index + 1;
    }
    *segments = output;
}

fn is_safe_watchability_split_part(segment: &Step5FinalSegment, target_lang: &str) -> bool {
    let translation = normalize_inline_text(&segment.translation);
    if translation.is_empty() || is_unusable_translation(&translation) {
        return false;
    }
    !looks_like_source_residue(&segment.source, &translation, target_lang)
}

fn merge_watchability_fragments(
    segments: &mut Vec<Step5FinalSegment>,
    subtitle_length_reference: u32,
    target_lang: &str,
) {
    if segments.len() < 2 {
        return;
    }

    let max_watch_units = (f64::from(subtitle_length_reference.max(1))
        * WATCHABILITY_MERGE_LEN_RATIO)
        .max(WATCHABILITY_SPLIT_TRIGGER);
    let mut merged = Vec::<Step5FinalSegment>::with_capacity(segments.len());
    let mut index = 0usize;

    while index < segments.len() {
        if index + 1 >= segments.len() {
            merged.push(segments[index].clone());
            break;
        }

        let left = &segments[index];
        let right = &segments[index + 1];

        if can_merge_watchability_fragments(left, right, max_watch_units, target_lang) {
            let merged_segment = merge_watchability_pair(left, right, target_lang);
            if is_watchability_fragment_issue(
                &merged_segment.source,
                &merged_segment.translation,
                target_lang,
            ) {
                merged.push(left.clone());
            } else {
                merged.push(merged_segment);
                index += 1;
            }
        } else {
            merged.push(left.clone());
        }
        index += 1;
    }

    if merged.len() == segments.len() {
        return;
    }
    for (index, segment) in merged.iter_mut().enumerate() {
        segment.segment_id = index + 1;
    }
    *segments = merged;
}

fn can_merge_watchability_fragments(
    left: &Step5FinalSegment,
    right: &Step5FinalSegment,
    max_watch_units: f64,
    target_lang: &str,
) -> bool {
    if left.translation.trim().is_empty() || right.translation.trim().is_empty() {
        return false;
    }
    if left.end > right.start {
        return false;
    }
    if right.start - left.end > WATCHABILITY_MERGE_TIME_GAP_SECONDS {
        return false;
    }
    if right.end - left.start > WATCHABILITY_MERGE_TIME_BUDGET_SECONDS {
        return false;
    }
    if is_terminal_punctuation(left.translation.trim().chars().last().unwrap_or_default()) {
        return false;
    }

    let left_frag = ends_with_short_dangling_fragment(&left.translation);
    if !left_frag && !is_watchability_fragment_issue(&left.source, &left.translation, target_lang) {
        return false;
    }

    if !starts_with_continuation_fragment(&right.translation, target_lang) {
        return false;
    }

    let merged_source = merge_watchability_text(&left.source, &right.source, " ", target_lang);
    if merged_source.is_empty() {
        return false;
    }
    let merged_translation =
        merge_watchability_text(&left.translation, &right.translation, "", target_lang);
    if merged_translation.is_empty() {
        return false;
    }

    if text_length_units(&merged_translation, target_lang) > max_watch_units {
        return false;
    }

    let repaired =
        repair_single_watchability_line(&merged_source, &merged_translation, target_lang);
    !is_watchability_fragment_issue(&merged_source, &repaired, target_lang)
}

fn merge_watchability_pair(
    left: &Step5FinalSegment,
    right: &Step5FinalSegment,
    target_lang: &str,
) -> Step5FinalSegment {
    let source = merge_watchability_text(&left.source, &right.source, " ", target_lang);
    let merged_translation =
        merge_watchability_text(&left.translation, &right.translation, "", target_lang);
    let translation = normalize_inline_text(&repair_single_watchability_line(
        &source,
        &merged_translation,
        target_lang,
    ));
    let mut tokens = left.tokens.clone();
    tokens.extend(right.tokens.iter().cloned());
    Step5FinalSegment {
        segment_id: left.segment_id,
        start: left.start,
        end: right.end.max(left.end),
        source,
        translation,
        tokens,
    }
}

fn starts_with_continuation_fragment(text: &str, target_lang: &str) -> bool {
    let normalized = normalize_inline_text(text);
    if normalized.is_empty() {
        return false;
    }
    if use_char_units(target_lang, &normalized) {
        let starters = [
            "个", "这个", "那个", "这", "那", "然后", "并且", "而且", "而", "并", "因为", "所以",
            "如果", "还", "继续", "将", "与", "和",
        ];
        return starters.iter().any(|prefix| normalized.starts_with(prefix));
    }

    let first_token = normalized
        .split_whitespace()
        .next()
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    if first_token.is_empty() {
        return false;
    }
    let starters = [
        "a", "an", "the", "to", "of", "and", "or", "with", "for", "this", "that", "if", "so",
        "then", "while", "it", "you", "we", "they",
    ];
    starters
        .iter()
        .any(|starter| first_token == *starter || normalized.starts_with(&format!("{starter} ")))
}

fn merge_watchability_text(left: &str, right: &str, separator: &str, _target_lang: &str) -> String {
    let left_clean = sanitize_translation_candidate(left);
    let right_clean = sanitize_translation_candidate(right);
    if left_clean.is_empty() {
        return right_clean;
    }
    if right_clean.is_empty() {
        return left_clean;
    }
    let mut merged = left_clean;
    if !separator.is_empty() {
        merged.push_str(separator);
    }
    merged.push_str(&right_clean);
    normalize_inline_text(&merged)
}

fn split_long_final_segment_for_watchability(
    segment: &Step5FinalSegment,
    target_lang: &str,
) -> Option<(Step5FinalSegment, Step5FinalSegment)> {
    if segment.translation.trim().is_empty() {
        return None;
    }

    if segment.tokens.len() >= 2 {
        split_final_segment_by_source_tokens(segment, target_lang)
    } else {
        split_final_segment_without_tokens(segment)
    }
}

fn split_final_segment_by_source_tokens(
    segment: &Step5FinalSegment,
    target_lang: &str,
) -> Option<(Step5FinalSegment, Step5FinalSegment)> {
    let split_index = split_token_index_by_readability(&segment.tokens, target_lang)?;
    let (left_tokens, right_tokens) = segment.tokens.split_at(split_index + 1);
    let right_tokens = right_tokens.to_vec();
    if right_tokens.is_empty() {
        return None;
    }
    let source_left = normalize_inline_text(&build_source_from_tokens(left_tokens));
    let source_right = normalize_inline_text(&build_source_from_tokens(&right_tokens));
    if source_left.is_empty() || source_right.is_empty() {
        return None;
    }
    let left_source_units = text_length_units(&source_left, target_lang);
    let right_source_units = text_length_units(&source_right, target_lang).max(1.0);
    let weights = vec![left_source_units.max(1.0), right_source_units];
    let translations = split_translation_evenly_by_weights(&segment.translation, 2, &weights);
    if translations.len() != 2 {
        return None;
    }
    let translation_left = normalize_inline_text(&translations[0]);
    let translation_right = normalize_inline_text(&translations[1]);
    if translation_left.is_empty() || translation_right.is_empty() {
        return None;
    }
    let left_start = segment.start.max(0.0);
    let (left_end, right_start) = source_split_times(left_tokens, &right_tokens, segment);
    Some((
        Step5FinalSegment {
            segment_id: segment.segment_id,
            start: left_start,
            end: left_end,
            source: source_left,
            translation: translation_left,
            tokens: left_tokens.to_vec(),
        },
        Step5FinalSegment {
            segment_id: segment.segment_id,
            start: right_start,
            end: segment.end.max(right_start),
            source: source_right,
            translation: translation_right,
            tokens: right_tokens,
        },
    ))
}

fn split_final_segment_without_tokens(
    segment: &Step5FinalSegment,
) -> Option<(Step5FinalSegment, Step5FinalSegment)> {
    let translations = split_translation_evenly_by_weights(&segment.translation, 2, &[1.0, 1.0]);
    if translations.len() != 2 {
        return None;
    }
    let source_parts = split_translation_evenly_by_weights(&segment.source, 2, &[1.0, 1.0]);
    if source_parts.len() != 2 {
        return None;
    }
    let left_len = segment.end - segment.start;
    if !left_len.is_finite() || left_len <= 0.0 {
        return None;
    }
    let mid = segment.start + (left_len * 0.5);
    Some((
        Step5FinalSegment {
            segment_id: segment.segment_id,
            start: segment.start,
            end: mid,
            source: normalize_inline_text(&source_parts[0]),
            translation: normalize_inline_text(&translations[0]),
            tokens: Vec::new(),
        },
        Step5FinalSegment {
            segment_id: segment.segment_id,
            start: mid,
            end: segment.end,
            source: normalize_inline_text(&source_parts[1]),
            translation: normalize_inline_text(&translations[1]),
            tokens: Vec::new(),
        },
    ))
}

fn split_token_index_by_readability(tokens: &[Step5Token], target_lang: &str) -> Option<usize> {
    if tokens.len() < 2 {
        return None;
    }
    let mut target_units = 0.0f64;
    let mut token_units = Vec::<f64>::with_capacity(tokens.len());
    for token in tokens {
        let unit = text_length_units(&token.text, target_lang);
        target_units += unit;
        token_units.push(unit);
    }
    let mut preferred = target_units / 2.0;
    if !preferred.is_finite() || preferred <= 0.0 {
        preferred = (tokens.len() as f64) / 2.0;
    }
    let mut cumulative = 0.0f64;
    for (index, unit) in token_units.iter().enumerate() {
        cumulative += unit;
        if cumulative >= preferred {
            if index == tokens.len() - 1 {
                return Some(index.saturating_sub(1));
            }
            return Some(index);
        }
    }
    Some((tokens.len() / 2).max(1) - 1)
}

fn source_split_times(
    left_tokens: &[Step5Token],
    right_tokens: &[Step5Token],
    segment: &Step5FinalSegment,
) -> (f64, f64) {
    let left_end = left_tokens
        .last()
        .map(|token| token.end.max(token.start))
        .unwrap_or(segment.end)
        .max(segment.start)
        .min(segment.end.max(segment.start));
    let right_start = right_tokens
        .first()
        .map(|token| token.start.min(segment.end).max(segment.start))
        .unwrap_or(left_end)
        .max(left_end);
    if (right_start - left_end).abs() < MIN_READABLE_DURATION_SECONDS {
        let mid = (segment.start + segment.end.max(segment.start)) / 2.0;
        (mid, mid)
    } else {
        (left_end, right_start)
    }
}

fn split_line_quality_score(lines: &[String]) -> i64 {
    if lines.is_empty() {
        return i64::MIN / 8;
    }
    let signatures = line_signatures(lines);
    let signature_counts = signature_counts(&signatures);
    let mut unique = HashSet::<String>::new();
    let mut score = 0i64;

    for (index, line) in lines.iter().enumerate() {
        let normalized = normalize_inline_text(line);
        if normalized.is_empty() {
            score -= 40;
            continue;
        }
        score += 20;
        let signature = signatures.get(index).cloned().unwrap_or_default();
        if !signature.is_empty() {
            unique.insert(signature.clone());
        }
        if signature.len() >= 6 && signature_counts.get(&signature).copied().unwrap_or(0) >= 2 {
            score -= 18;
        }
        score -= line_fragment_penalty(&normalized);
    }
    score + (unique.len() as i64 * 6) - line_redundancy_penalty(&signatures)
}

fn line_fragment_penalty(text: &str) -> i64 {
    let normalized = normalize_inline_text(text);
    if normalized.is_empty() {
        return 0;
    }
    let char_count = normalized.chars().count();
    let ends_with_terminal = normalized
        .chars()
        .last()
        .map(is_terminal_punctuation)
        .unwrap_or(false);
    let starts_with_punct = normalized
        .chars()
        .next()
        .map(|ch| matches!(ch, ',' | '，' | '、' | '。' | ':' | '：' | ';' | '；'))
        .unwrap_or(false);
    let mut penalty = 0i64;
    if starts_with_punct {
        penalty += 8;
    }
    if char_count <= 4 && !ends_with_terminal {
        penalty += 6;
    }
    if ends_with_connector_like_fragment(&normalized) {
        penalty += 8;
    }
    if char_count <= 8 && ends_with_short_dangling_fragment(&normalized) {
        penalty += 10;
    }
    penalty
}

fn line_redundancy_penalty(signatures: &[String]) -> i64 {
    if signatures.len() <= 1 {
        return 0;
    }
    let mut penalty = 0i64;
    for left_index in 0..signatures.len() {
        let left = signatures[left_index].as_str();
        if left.len() < 8 {
            continue;
        }
        for right_index in (left_index + 1)..signatures.len() {
            let right = signatures[right_index].as_str();
            if right.len() < 8 || left == right {
                continue;
            }
            let (shorter, longer) = if left.len() <= right.len() {
                (left, right)
            } else {
                (right, left)
            };
            if !longer.contains(shorter) {
                continue;
            }
            let overlap_ratio = shorter.len() as f64 / longer.len() as f64;
            if overlap_ratio >= 0.45 {
                penalty += if overlap_ratio >= 0.7 { 18 } else { 12 };
            }
        }
    }
    penalty
}

fn is_terminal_punctuation(ch: char) -> bool {
    matches!(
        ch,
        '.' | '!' | '?' | ';' | '。' | '！' | '？' | '；' | '，' | ','
    )
}

fn ends_with_short_dangling_fragment(text: &str) -> bool {
    let normalized = normalize_inline_text(text);
    if normalized.is_empty() {
        return false;
    }
    let suffixes = ["一个", "做一个", "这个", "那个", "这笔", "那笔", "这", "那"];
    suffixes.iter().any(|suffix| normalized.ends_with(suffix))
}

fn ends_with_connector_like_fragment(text: &str) -> bool {
    let normalized = normalize_inline_text(text);
    if normalized.is_empty() {
        return false;
    }
    let cjk_connectors = [
        "然后", "而且", "并且", "因为", "所以", "但是", "如果", "为了", "以及", "还有", "并", "和",
        "与", "及", "或", "来", "去", "在", "对", "把", "将", "大约",
    ];
    if cjk_connectors
        .iter()
        .any(|suffix| normalized.ends_with(suffix))
    {
        return true;
    }
    let lower = normalized.to_ascii_lowercase();
    let ascii_connectors = [
        "and", "or", "to", "for", "with", "that", "which", "when", "if", "but", "so",
    ];
    ascii_connectors
        .iter()
        .any(|suffix| lower.ends_with(suffix))
}

fn is_watchability_fragment_issue(source: &str, translation: &str, target_lang: &str) -> bool {
    let normalized = normalize_inline_text(translation);
    if normalized.is_empty() {
        return false;
    }
    let source_units = count_word_units(source);
    if source_units < 6 {
        return false;
    }
    if let Some(leading_number) = leading_number_anchor(&normalized) {
        let source_numbers = extract_numbers(source);
        let source_matches = source_numbers.iter().any(|value| value == &leading_number);
        if !source_matches {
            return true;
        }
    }
    let has_terminal = normalized
        .chars()
        .last()
        .map(is_terminal_punctuation)
        .unwrap_or(false);
    if has_terminal {
        return false;
    }
    if ends_with_connector_like_fragment(&normalized)
        || ends_with_short_dangling_fragment(&normalized)
    {
        return true;
    }
    let fragment_penalty = line_fragment_penalty(&normalized);
    let line_units = text_length_units(&normalized, target_lang);
    fragment_penalty >= 8 && line_units <= 14.0
}

fn choose_better_alignment(
    parent: &Step5SplitParent,
    aligned_lines: &[String],
    fallback_lines: &[String],
    target_lang: &str,
) -> Vec<String> {
    let aligned_score = alignment_candidate_score(parent, aligned_lines, target_lang);
    let fallback_score = alignment_candidate_score(parent, fallback_lines, target_lang);
    if fallback_score > aligned_score + 2 {
        return fallback_lines.to_vec();
    }
    aligned_lines.to_vec()
}

fn alignment_candidate_score(
    parent: &Step5SplitParent,
    lines: &[String],
    target_lang: &str,
) -> i64 {
    let mut score = split_line_quality_score(lines);
    for (index, part) in parent.parts.iter().enumerate() {
        let line = lines
            .get(index)
            .map(|value| normalize_inline_text(value))
            .unwrap_or_default();
        if line.is_empty() {
            score -= 40;
            continue;
        }
        if looks_like_source_residue(&part.source, &line, target_lang) {
            score -= 24;
        }
        if has_tail_ellipsis(&line) {
            score -= 16;
        }
        let source_numbers = extract_numbers(&part.source);
        if !source_numbers.is_empty() {
            let line_numbers = extract_numbers(&line);
            let missing = source_numbers
                .iter()
                .filter(|value| !line_numbers.contains(*value))
                .count();
            score -= (missing as i64) * 14;
        }
        if let Some(line_leading_number) = leading_number_anchor(&line) {
            let source_leading_number = leading_number_anchor(&part.source);
            let source_matches_leading = source_leading_number
                .as_ref()
                .map(|value| value == &line_leading_number)
                .unwrap_or(false);
            if !source_matches_leading {
                score -= 12;
            }
        }
        let source_units = count_word_units(&part.source) as f64;
        let line_units = text_length_units(&line, target_lang);
        if source_units >= 8.0 && line_units <= 5.0 {
            score -= 14;
        } else if source_units >= 6.0 && line_units <= 4.0 {
            score -= 8;
        }
    }
    if parent.parts.len() >= 2 {
        for index in 0..(parent.parts.len() - 1) {
            let current_source_numbers = extract_numbers(&parent.parts[index].source);
            let next_source_numbers = extract_numbers(&parent.parts[index + 1].source);
            if next_source_numbers.is_empty() {
                continue;
            }
            let current_translation_numbers = lines
                .get(index)
                .map(|line| extract_numbers(line))
                .unwrap_or_default();
            if current_translation_numbers.is_empty() {
                continue;
            }
            let mut next_only = HashSet::<String>::new();
            for value in next_source_numbers {
                if !current_source_numbers.contains(&value) {
                    next_only.insert(value);
                }
            }
            if next_only.is_empty() {
                continue;
            }
            let leaked = next_only
                .iter()
                .any(|value| current_translation_numbers.contains(value));
            if leaked {
                score -= 40;
            }
        }
    }
    score
}

fn has_empty_or_duplicated_long_line(lines: &[String]) -> bool {
    if lines
        .iter()
        .any(|line| normalize_inline_text(line).is_empty() || has_tail_ellipsis(line))
    {
        return true;
    }
    let signatures = line_signatures(lines);
    let signature_counts = signature_counts(&signatures);
    signatures.iter().any(|signature| {
        signature.len() >= 6 && signature_counts.get(signature).copied().unwrap_or(0) >= 2
    })
}

fn line_signatures(lines: &[String]) -> Vec<String> {
    lines.iter().map(|line| line_signature(line)).collect()
}

fn line_signature(text: &str) -> String {
    normalize_inline_text(text)
        .to_lowercase()
        .chars()
        .filter(|ch| is_meaningful_text_char(*ch))
        .collect::<String>()
}

fn signature_counts(signatures: &[String]) -> HashMap<String, usize> {
    let mut out = HashMap::<String, usize>::new();
    for signature in signatures {
        if signature.is_empty() {
            continue;
        }
        *out.entry(signature.clone()).or_insert(0) += 1;
    }
    out
}

fn looks_like_full_parent_copy(text: &str, parent_draft: &str) -> bool {
    let normalized = normalize_inline_text(text);
    let draft = normalize_inline_text(parent_draft);
    if normalized.is_empty() || draft.is_empty() {
        return false;
    }
    let normalized_len = normalized.chars().count();
    let draft_len = draft.chars().count();
    if normalized_len < 12 || draft_len < 12 {
        return false;
    }
    let shorter = normalized_len.min(draft_len) as f64;
    let longer = normalized_len.max(draft_len) as f64;
    if shorter / longer < 0.82 {
        return false;
    }
    normalized.contains(&draft) || draft.contains(&normalized)
}

fn target_prefers_cjk(target_lang: &str) -> bool {
    let normalized = target_lang.trim().to_ascii_lowercase();
    normalized.starts_with("zh") || normalized.starts_with("ja") || normalized.starts_with("ko")
}

fn extract_ascii_words(text: &str) -> Vec<String> {
    let mut out = Vec::<String>::new();
    let mut current = String::new();
    for ch in text.chars() {
        if ch.is_ascii_alphabetic() {
            current.push(ch.to_ascii_lowercase());
            continue;
        }
        if current.len() >= 2 {
            out.push(current.clone());
        }
        current.clear();
    }
    if current.len() >= 2 {
        out.push(current);
    }
    out
}

fn looks_like_source_residue(source: &str, translation: &str, target_lang: &str) -> bool {
    if !target_prefers_cjk(target_lang) {
        return false;
    }
    let translation_words = extract_ascii_words(translation);
    if translation_words.len() < 4 {
        return false;
    }
    let source_words = extract_ascii_words(source)
        .into_iter()
        .collect::<HashSet<_>>();
    if source_words.is_empty() {
        return false;
    }
    let overlap = translation_words
        .iter()
        .filter(|word| source_words.contains(*word))
        .count();
    let overlap_ratio = overlap as f64 / translation_words.len() as f64;
    let cjk_count = translation.chars().filter(|ch| is_cjk_char(*ch)).count();
    overlap_ratio >= 0.6 && cjk_count <= 2
}

fn looks_like_non_cjk_translation_for_cjk_target(text: &str, target_lang: &str) -> bool {
    if !target_prefers_cjk(target_lang) {
        return false;
    }
    let normalized = normalize_inline_text(text);
    if normalized.is_empty() {
        return false;
    }
    let cjk_count = normalized.chars().filter(|ch| is_cjk_char(*ch)).count();
    let ascii_words = extract_ascii_words(&normalized);
    cjk_count <= 1 && ascii_words.len() >= 4
}

fn shared_number_count(left: &HashSet<String>, right: &HashSet<String>) -> usize {
    if left.is_empty() || right.is_empty() {
        return 0;
    }
    left.iter().filter(|value| right.contains(*value)).count()
}

fn neighbor_source_numbers(
    source_numbers_by_part: &[HashSet<String>],
    index: usize,
) -> HashSet<String> {
    let mut out = HashSet::<String>::new();
    if index > 0 {
        for value in &source_numbers_by_part[index - 1] {
            out.insert(value.clone());
        }
    }
    if index + 1 < source_numbers_by_part.len() {
        for value in &source_numbers_by_part[index + 1] {
            out.insert(value.clone());
        }
    }
    out
}

fn repair_aligned_lines(
    parent: &Step5SplitParent,
    aligned: &[String],
    fallback: &[String],
    target_lang: &str,
) -> Vec<String> {
    let mut out = Vec::<String>::with_capacity(parent.parts.len());
    let source_numbers_by_part = parent
        .parts
        .iter()
        .map(|part| extract_numbers(&part.source))
        .collect::<Vec<_>>();
    let aligned_numbers_by_part = aligned
        .iter()
        .map(|line| extract_numbers(line))
        .collect::<Vec<_>>();
    let aligned_signatures = line_signatures(aligned);
    let fallback_signatures = line_signatures(fallback);
    let aligned_signature_counts = signature_counts(&aligned_signatures);
    let parent_draft = normalize_inline_text(&parent.draft_translation);

    for (index, part) in parent.parts.iter().enumerate() {
        let source_numbers = source_numbers_by_part
            .get(index)
            .cloned()
            .unwrap_or_default();
        let fallback_text = fallback
            .get(index)
            .map(|value| sanitize_translation_candidate(value))
            .unwrap_or_default();
        let mut text = aligned
            .get(index)
            .map(|value| sanitize_translation_candidate(value))
            .unwrap_or_default();
        let signature = aligned_signatures.get(index).cloned().unwrap_or_default();
        let is_duplicate_line = signature.len() >= 6
            && aligned_signature_counts
                .get(&signature)
                .copied()
                .unwrap_or(0)
                >= 2
            && signature != fallback_signatures.get(index).cloned().unwrap_or_default();
        if is_duplicate_line
            && !is_unusable_translation(&fallback_text)
            && !has_tail_ellipsis(&fallback_text)
        {
            text = fallback_text.clone();
        }
        if parent.parts.len() > 1
            && looks_like_full_parent_copy(&text, &parent_draft)
            && !is_unusable_translation(&fallback_text)
            && !looks_like_full_parent_copy(&fallback_text, &parent_draft)
        {
            text = fallback_text.clone();
        }
        if !source_numbers.is_empty() && !is_unusable_translation(&fallback_text) {
            let current_penalty = numeric_alignment_penalty(&source_numbers, &text);
            let fallback_penalty = numeric_alignment_penalty(&source_numbers, &fallback_text);
            if fallback_penalty < current_penalty {
                text = fallback_text.clone();
            }
        }
        if let Some(leading_anchor) = leading_number_anchor(&part.source) {
            let text_numbers = extract_numbers(&text);
            if !text_numbers.contains(&leading_anchor) {
                let fallback_numbers = extract_numbers(&fallback_text);
                if fallback_numbers.contains(&leading_anchor)
                    && !is_unusable_translation(&fallback_text)
                {
                    text = fallback_text.clone();
                }
            }
            let text_numbers_after = extract_numbers(&text);
            if !text_numbers_after.contains(&leading_anchor) && !text.is_empty() {
                text = sanitize_translation_candidate(&format!("{leading_anchor} {text}"));
            }
        }
        if let Some(text_leading_number) = leading_number_anchor(&text) {
            let source_leading_number = leading_number_anchor(&part.source);
            let source_matches_leading = source_leading_number
                .as_ref()
                .map(|value| value == &text_leading_number)
                .unwrap_or(false);
            if !source_matches_leading {
                let fallback_leading_number = leading_number_anchor(&fallback_text);
                let fallback_matches_source = source_leading_number
                    .as_ref()
                    .map(|value| fallback_leading_number.as_ref() == Some(value))
                    .unwrap_or(fallback_leading_number.is_none());
                if fallback_matches_source && !is_unusable_translation(&fallback_text) {
                    text = fallback_text.clone();
                } else {
                    let stripped = strip_leading_number_token(&text);
                    if !stripped.is_empty() {
                        text = stripped;
                    }
                }
            }
        }
        let text_numbers = extract_numbers(&text);
        if source_numbers.is_empty() && !text_numbers.is_empty() {
            let neighbor_numbers = neighbor_source_numbers(&source_numbers_by_part, index);
            let text_neighbor_hits = shared_number_count(&text_numbers, &neighbor_numbers);
            if text_neighbor_hits > 0 {
                let fallback_numbers = extract_numbers(&fallback_text);
                let fallback_hits = shared_number_count(&fallback_numbers, &neighbor_numbers);
                if fallback_hits < text_neighbor_hits && !is_unusable_translation(&fallback_text) {
                    text = fallback_text.clone();
                }
                let text_numbers_after = extract_numbers(&text);
                let leaked_numbers = text_numbers_after
                    .iter()
                    .filter(|value| neighbor_numbers.contains(*value))
                    .cloned()
                    .collect::<HashSet<_>>();
                if !leaked_numbers.is_empty() {
                    if let Some(trimmed) = trim_before_leaked_number_anchor(&text, &leaked_numbers)
                    {
                        text = trimmed;
                    } else if let Some(leading) = leading_number_anchor(&text) {
                        if leaked_numbers.contains(&leading) {
                            let stripped = strip_leading_number_token(&text);
                            if !stripped.is_empty() {
                                text = stripped;
                            }
                        }
                    }
                }
            }
        }
        if looks_like_source_residue(&part.source, &text, target_lang)
            && !looks_like_source_residue(&part.source, &fallback_text, target_lang)
            && !is_unusable_translation(&fallback_text)
        {
            text = fallback_text.clone();
        }
        if is_unusable_translation(&text) {
            if !is_unusable_translation(&fallback_text) {
                text = fallback_text;
            }
        }
        if is_unusable_translation(&text) {
            text = normalize_inline_text(&part.source);
        }
        if has_tail_ellipsis(&text) {
            let trimmed = strip_tail_ellipsis(&text);
            if !trimmed.is_empty() {
                text = trimmed;
            }
        }
        if is_unusable_translation(&text) {
            text = "[缺失译文]".to_string();
        }
        out.push(text);
    }

    let out_signatures = line_signatures(&out);
    let out_signature_counts = signature_counts(&out_signatures);
    let fallback_score = split_line_quality_score(fallback);
    let out_score = split_line_quality_score(&out);
    if fallback_score > out_score {
        for (index, text) in out.iter_mut().enumerate() {
            let signature = out_signatures.get(index).cloned().unwrap_or_default();
            let is_duplicate_line = signature.len() >= 6
                && out_signature_counts.get(&signature).copied().unwrap_or(0) >= 2;
            if !is_duplicate_line {
                continue;
            }
            let fallback_text = fallback
                .get(index)
                .map(|value| normalize_inline_text(value))
                .unwrap_or_default();
            if !is_unusable_translation(&fallback_text) {
                *text = fallback_text;
            }
        }
    }

    if out.len() >= 2 {
        for index in 0..(out.len() - 1) {
            let left_source_numbers = source_numbers_by_part
                .get(index)
                .cloned()
                .unwrap_or_default();
            let right_source_numbers = source_numbers_by_part
                .get(index + 1)
                .cloned()
                .unwrap_or_default();

            let mut left_text =
                sanitize_translation_candidate(out.get(index).map(String::as_str).unwrap_or(""));
            let mut right_text = sanitize_translation_candidate(
                out.get(index + 1).map(String::as_str).unwrap_or(""),
            );

            let left_numbers = extract_numbers(&left_text);
            let right_numbers = extract_numbers(&right_text);
            let left_aligned_numbers = aligned_numbers_by_part
                .get(index)
                .cloned()
                .unwrap_or_default();
            let right_aligned_numbers = aligned_numbers_by_part
                .get(index + 1)
                .cloned()
                .unwrap_or_default();

            if !left_source_numbers.is_empty() && !right_source_numbers.is_empty() {
                let left_numbers_now = extract_numbers(&left_text);
                let right_numbers_now = extract_numbers(&right_text);
                let leaked_from_left = left_numbers_now
                    .iter()
                    .filter(|value| right_source_numbers.contains(*value))
                    .filter(|value| !left_source_numbers.contains(*value))
                    .cloned()
                    .collect::<HashSet<_>>();
                let leaked_from_right = right_numbers_now
                    .iter()
                    .filter(|value| left_source_numbers.contains(*value))
                    .filter(|value| !right_source_numbers.contains(*value))
                    .cloned()
                    .collect::<HashSet<_>>();

                let missing_on_right = right_source_numbers
                    .iter()
                    .filter(|value| !right_numbers_now.contains(*value))
                    .count();
                if !leaked_from_left.is_empty() && missing_on_right == 0 {
                    let left_fallback = fallback
                        .get(index)
                        .map(|value| sanitize_translation_candidate(value))
                        .unwrap_or_default();
                    if !is_unusable_translation(&left_fallback) {
                        let fallback_numbers = extract_numbers(&left_fallback);
                        let fallback_leak = leaked_from_left
                            .iter()
                            .filter(|value| fallback_numbers.contains(*value))
                            .count();
                        let current_leak = leaked_from_left
                            .iter()
                            .filter(|value| left_numbers_now.contains(*value))
                            .count();
                        if fallback_leak < current_leak {
                            left_text = left_fallback;
                        }
                    }
                    let left_numbers_after = extract_numbers(&left_text);
                    let remaining = leaked_from_left
                        .iter()
                        .filter(|value| left_numbers_after.contains(*value))
                        .cloned()
                        .collect::<HashSet<_>>();
                    if !remaining.is_empty() {
                        if let Some(trimmed) =
                            trim_before_leaked_number_anchor(&left_text, &remaining)
                        {
                            left_text = trimmed;
                        }
                    }
                }

                let missing_on_left = left_source_numbers
                    .iter()
                    .filter(|value| !left_numbers_now.contains(*value))
                    .count();
                if !leaked_from_right.is_empty() && missing_on_left == 0 {
                    let right_fallback = fallback
                        .get(index + 1)
                        .map(|value| sanitize_translation_candidate(value))
                        .unwrap_or_default();
                    if !is_unusable_translation(&right_fallback) {
                        let fallback_numbers = extract_numbers(&right_fallback);
                        let fallback_leak = leaked_from_right
                            .iter()
                            .filter(|value| fallback_numbers.contains(*value))
                            .count();
                        let current_leak = leaked_from_right
                            .iter()
                            .filter(|value| right_numbers_now.contains(*value))
                            .count();
                        if fallback_leak < current_leak {
                            right_text = right_fallback;
                        }
                    }
                    let right_numbers_after = extract_numbers(&right_text);
                    let remaining = leaked_from_right
                        .iter()
                        .filter(|value| right_numbers_after.contains(*value))
                        .cloned()
                        .collect::<HashSet<_>>();
                    if !remaining.is_empty() {
                        if let Some(trimmed) =
                            trim_before_leaked_number_anchor(&right_text, &remaining)
                        {
                            right_text = trimmed;
                        }
                    }
                }
            }

            if left_source_numbers.is_empty() && !right_source_numbers.is_empty() {
                let leaked_to_left = shared_number_count(&left_numbers, &right_source_numbers).max(
                    shared_number_count(&left_aligned_numbers, &right_source_numbers),
                );
                let missing_on_right = right_source_numbers
                    .iter()
                    .filter(|value| !right_numbers.contains(*value))
                    .count();
                if leaked_to_left > 0 && missing_on_right > 0 {
                    let left_fallback = fallback
                        .get(index)
                        .map(|value| normalize_inline_text(value))
                        .unwrap_or_default();
                    let right_fallback = fallback
                        .get(index + 1)
                        .map(|value| normalize_inline_text(value))
                        .unwrap_or_default();
                    if !is_unusable_translation(&right_fallback)
                        && numeric_alignment_penalty(&right_source_numbers, &right_fallback)
                            < numeric_alignment_penalty(&right_source_numbers, &right_text)
                    {
                        right_text = right_fallback;
                    }
                    if !is_unusable_translation(&left_fallback) {
                        let left_fallback_numbers = extract_numbers(&left_fallback);
                        let fallback_leak =
                            shared_number_count(&left_fallback_numbers, &right_source_numbers);
                        if fallback_leak < leaked_to_left {
                            left_text = left_fallback;
                        }
                    }
                    let right_numbers_after = extract_numbers(&right_text);
                    let mut remaining_missing = right_source_numbers
                        .iter()
                        .filter(|value| !right_numbers_after.contains(*value))
                        .filter(|value| {
                            left_numbers.contains(*value) || left_aligned_numbers.contains(*value)
                        })
                        .cloned()
                        .collect::<Vec<_>>();
                    if !remaining_missing.is_empty() && !right_text.is_empty() {
                        remaining_missing.sort();
                        let prefix = remaining_missing.join("/");
                        right_text = normalize_inline_text(&format!("{prefix} {right_text}"));
                    }
                }
                if leaked_to_left > 0 && missing_on_right == 0 {
                    let left_fallback = fallback
                        .get(index)
                        .map(|value| sanitize_translation_candidate(value))
                        .unwrap_or_default();
                    if !is_unusable_translation(&left_fallback) {
                        let left_fallback_numbers = extract_numbers(&left_fallback);
                        let fallback_leak =
                            shared_number_count(&left_fallback_numbers, &right_source_numbers);
                        if fallback_leak < leaked_to_left {
                            left_text = left_fallback;
                        }
                    }
                    let left_numbers_after = extract_numbers(&left_text);
                    let remaining_leak =
                        shared_number_count(&left_numbers_after, &right_source_numbers);
                    if remaining_leak > 0 {
                        let trim_numbers = left_numbers_after
                            .iter()
                            .filter(|value| right_source_numbers.contains(*value))
                            .cloned()
                            .collect::<HashSet<_>>();
                        if let Some(trimmed) =
                            trim_before_leaked_number_anchor(&left_text, &trim_numbers)
                        {
                            left_text = trimmed;
                        }
                    }
                }
            }

            if right_source_numbers.is_empty() && !left_source_numbers.is_empty() {
                let leaked_to_right =
                    shared_number_count(&right_numbers, &left_source_numbers).max(
                        shared_number_count(&right_aligned_numbers, &left_source_numbers),
                    );
                let missing_on_left = left_source_numbers
                    .iter()
                    .filter(|value| !left_numbers.contains(*value))
                    .count();
                if leaked_to_right > 0 && missing_on_left > 0 {
                    let left_fallback = fallback
                        .get(index)
                        .map(|value| normalize_inline_text(value))
                        .unwrap_or_default();
                    let right_fallback = fallback
                        .get(index + 1)
                        .map(|value| normalize_inline_text(value))
                        .unwrap_or_default();
                    if !is_unusable_translation(&left_fallback)
                        && numeric_alignment_penalty(&left_source_numbers, &left_fallback)
                            < numeric_alignment_penalty(&left_source_numbers, &left_text)
                    {
                        left_text = left_fallback;
                    }
                    if !is_unusable_translation(&right_fallback) {
                        let right_fallback_numbers = extract_numbers(&right_fallback);
                        let fallback_leak =
                            shared_number_count(&right_fallback_numbers, &left_source_numbers);
                        if fallback_leak < leaked_to_right {
                            right_text = right_fallback;
                        }
                    }
                    let left_numbers_after = extract_numbers(&left_text);
                    let mut remaining_missing = left_source_numbers
                        .iter()
                        .filter(|value| !left_numbers_after.contains(*value))
                        .filter(|value| {
                            right_numbers.contains(*value) || right_aligned_numbers.contains(*value)
                        })
                        .cloned()
                        .collect::<Vec<_>>();
                    if !remaining_missing.is_empty() && !left_text.is_empty() {
                        remaining_missing.sort();
                        let prefix = remaining_missing.join("/");
                        left_text = normalize_inline_text(&format!("{prefix} {left_text}"));
                    }
                }
            }

            out[index] = left_text;
            out[index + 1] = right_text;
        }
    }

    let mut source_number_universe = HashSet::<String>::new();
    for source_numbers in &source_numbers_by_part {
        for value in source_numbers {
            source_number_universe.insert(value.clone());
        }
    }
    if !source_number_universe.is_empty() {
        let mut number_keys = source_number_universe.into_iter().collect::<Vec<_>>();
        number_keys.sort();
        for number in number_keys {
            let mut missing_indexes = Vec::<usize>::new();
            let mut leaked_indexes = Vec::<usize>::new();
            for index in 0..out.len() {
                let source_has = source_numbers_by_part
                    .get(index)
                    .map(|values| values.contains(&number))
                    .unwrap_or(false);
                let line_numbers = out
                    .get(index)
                    .map(|line| extract_numbers(line))
                    .unwrap_or_default();
                let line_has = line_numbers.contains(&number);
                if source_has && !line_has {
                    missing_indexes.push(index);
                }
                if !source_has && line_has {
                    leaked_indexes.push(index);
                }
            }
            if missing_indexes.is_empty() {
                continue;
            }

            for index in &missing_indexes {
                let fallback_text = fallback
                    .get(*index)
                    .map(|value| sanitize_translation_candidate(value))
                    .unwrap_or_default();
                if is_unusable_translation(&fallback_text) {
                    continue;
                }
                if extract_numbers(&fallback_text).contains(&number) {
                    out[*index] = fallback_text;
                }
            }

            let unresolved_missing = missing_indexes
                .into_iter()
                .filter(|index| {
                    out.get(*index)
                        .map(|line| !extract_numbers(line).contains(&number))
                        .unwrap_or(true)
                })
                .collect::<Vec<_>>();
            let has_leak_evidence = !leaked_indexes.is_empty()
                || unresolved_missing.iter().any(|index| {
                    aligned_numbers_by_part
                        .get(*index)
                        .map(|values| values.contains(&number))
                        .unwrap_or(false)
                });
            if !has_leak_evidence {
                continue;
            }
            for index in unresolved_missing {
                let current = out.get(index).cloned().unwrap_or_default();
                if current.is_empty() {
                    continue;
                }
                let updated = prepend_missing_number_token(&current, &number);
                if !updated.is_empty() {
                    out[index] = updated;
                }
            }
        }
    }

    let source_lines = parent
        .parts
        .iter()
        .map(|part| part.source.clone())
        .collect::<Vec<_>>();
    repair_watchability_lines(&source_lines, &mut out, target_lang);
    out
}

fn repair_polished_translation(segment: &mut Step5FinalSegment) {
    let mut translation = sanitize_translation_candidate(&segment.translation);
    if is_unusable_translation(&translation) {
        translation = normalize_inline_text(&segment.source);
    }
    if has_tail_ellipsis(&translation) {
        let trimmed = strip_tail_ellipsis(&translation);
        if !trimmed.is_empty() {
            translation = trimmed;
        }
    }
    if is_unusable_translation(&translation) {
        translation = "[缺失译文]".to_string();
    }
    translation = append_missing_terminal_punctuation(&segment.source, &translation);
    segment.translation = translation;
}

fn append_missing_terminal_punctuation(source: &str, translation: &str) -> String {
    let translation = normalize_inline_text(translation);
    if translation.is_empty()
        || translation
            .chars()
            .last()
            .map(is_terminal_punctuation)
            .unwrap_or(false)
    {
        return translation;
    }

    let Some(source_terminal) = source.trim().chars().last() else {
        return translation;
    };
    if !is_terminal_punctuation(source_terminal) {
        return translation;
    }

    let mut out = translation;
    let punctuation = if out.chars().any(is_cjk_char) {
        match source_terminal {
            '?' | '？' => '？',
            '!' | '！' => '！',
            _ => '。',
        }
    } else {
        match source_terminal {
            '？' => '?',
            '！' => '!',
            '。' => '.',
            other => other,
        }
    };
    out.push(punctuation);
    out
}

fn source_contains_terminology_term(source_lower: &str, term_source_lower: &str) -> bool {
    let term = term_source_lower.trim();
    if term.is_empty() {
        return false;
    }
    if term.chars().any(is_cjk_char) {
        return source_lower.contains(term);
    }

    let is_single_ascii_token_term = term.chars().any(|ch| ch.is_ascii_alphabetic())
        && !term.chars().any(|ch| ch.is_whitespace() || is_cjk_char(ch));
    if !is_single_ascii_token_term {
        return source_lower.contains(term);
    }

    let mut search_start = 0usize;
    while let Some(offset) = source_lower[search_start..].find(term) {
        let start = search_start + offset;
        let end = start + term.len();
        let prev_blocks = source_lower[..start]
            .chars()
            .next_back()
            .map(|ch| ch.is_ascii_alphabetic())
            .unwrap_or(false);
        let next_blocks = source_lower[end..]
            .chars()
            .next()
            .map(|ch| ch.is_ascii_alphabetic())
            .unwrap_or(false);
        if !prev_blocks && !next_blocks {
            return true;
        }
        search_start = end;
    }

    false
}

fn repair_watchability_fragments(segments: &mut [Step5FinalSegment], target_lang: &str) {
    if segments.is_empty() {
        return;
    }
    let source_lines = segments
        .iter()
        .map(|segment| segment.source.clone())
        .collect::<Vec<_>>();
    let mut translation_lines = segments
        .iter()
        .map(|segment| segment.translation.clone())
        .collect::<Vec<_>>();
    repair_watchability_lines(&source_lines, &mut translation_lines, target_lang);
    for (segment, translation) in segments.iter_mut().zip(translation_lines.into_iter()) {
        segment.translation = translation;
    }
}

fn apply_residual_watchability_overrides(segments: &mut [Step5FinalSegment], target_lang: &str) {
    for segment in segments.iter_mut() {
        let mut updated = sanitize_translation_candidate(&segment.translation);
        if is_watchability_fragment_issue(&segment.source, &updated, target_lang) {
            if let Some(trimmed) = trim_trailing_connector_fragment(&updated) {
                updated = trimmed;
            }
        }
        segment.translation = updated;
    }
}

fn repair_watchability_lines(
    source_lines: &[String],
    translation_lines: &mut [String],
    target_lang: &str,
) {
    if source_lines.len() != translation_lines.len() {
        return;
    }

    for index in 0..translation_lines.len() {
        translation_lines[index] = repair_single_watchability_line(
            &source_lines[index],
            &translation_lines[index],
            target_lang,
        );
    }

    for index in 0..translation_lines.len() {
        translation_lines[index] = repair_single_watchability_line(
            &source_lines[index],
            &translation_lines[index],
            target_lang,
        );
    }
}

fn repair_single_watchability_line(source: &str, translation: &str, target_lang: &str) -> String {
    let original = sanitize_translation_candidate(translation);
    if original.is_empty() {
        return original;
    }

    let mut updated = original.clone();

    if !is_watchability_fragment_issue(source, &updated, target_lang) {
        return updated;
    }

    if is_watchability_fragment_issue(source, &updated, target_lang) {
        if let Some(trimmed) = trim_trailing_connector_fragment(&updated) {
            updated = trimmed;
        }
    }
    updated
}

fn trim_trailing_connector_fragment(text: &str) -> Option<String> {
    let normalized = sanitize_translation_candidate(text);
    if normalized.is_empty() {
        return None;
    }
    let suffixes = [
        "而且",
        "并且",
        "因为",
        "所以",
        "但是",
        "如果",
        "为了",
        "以及",
        "还有",
        "并",
        "和",
        "与",
        "及",
        "或",
        "来",
        "去",
        "在",
        "对",
        "把",
        "将",
        "做一个",
        "大约",
    ];
    for suffix in suffixes {
        if !normalized.ends_with(suffix) {
            continue;
        }
        let trimmed = normalized
            .trim_end_matches(suffix)
            .trim_end_matches('，')
            .trim_end_matches(',')
            .trim();
        if !trimmed.is_empty() {
            return Some(normalize_inline_text(trimmed));
        }
    }
    None
}

fn sanitize_translation_candidate(raw: &str) -> String {
    let mut text = normalize_inline_text(raw);
    if text.is_empty() {
        return text;
    }
    if has_tail_ellipsis(&text) {
        let trimmed = strip_tail_ellipsis(&text);
        if !trimmed.is_empty() {
            text = trimmed;
        }
    }
    normalize_inline_text(&text)
}

fn prepend_missing_number_token(text: &str, number: &str) -> String {
    let normalized = sanitize_translation_candidate(text);
    if normalized.is_empty() {
        return normalized;
    }
    let mut numbers = Vec::<String>::new();
    if !number.trim().is_empty() {
        numbers.push(number.to_string());
    }
    let mut body = normalized.clone();
    for _ in 0..3 {
        let Some(leading) = leading_number_anchor(&body) else {
            break;
        };
        numbers.push(leading);
        let stripped = strip_leading_number_token(&body);
        if stripped.is_empty() || stripped == body {
            break;
        }
        body = stripped;
    }
    numbers.sort_by(|left, right| {
        let left_num = left.parse::<f64>().ok();
        let right_num = right.parse::<f64>().ok();
        match (left_num, right_num) {
            (Some(a), Some(b)) => a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal),
            _ => left.cmp(right),
        }
    });
    numbers.dedup();
    if numbers.is_empty() {
        return body;
    }
    let prefix = numbers.join("/");
    if body.is_empty() {
        return prefix;
    }
    normalize_inline_text(&format!("{prefix} {body}"))
}

fn leading_number_anchor(text: &str) -> Option<String> {
    let trimmed = text.trim_start();
    if trimmed.is_empty() {
        return None;
    }
    let mut raw = String::new();
    for ch in trimmed.chars() {
        if ch.is_ascii_digit() || ch == '.' || ch == ',' {
            raw.push(ch);
            continue;
        }
        break;
    }
    if raw.is_empty() {
        return None;
    }
    let value = parse_ascii_number(&raw);
    let normalized = normalize_numeric_value(value);
    if normalized.is_empty() {
        return None;
    }
    Some(normalized)
}

fn strip_leading_number_token(text: &str) -> String {
    let trimmed = text.trim_start();
    if trimmed.is_empty() {
        return String::new();
    }
    let chars = trimmed.char_indices().collect::<Vec<_>>();
    if chars.is_empty() {
        return String::new();
    }
    let mut number_end = 0usize;
    for (idx, ch) in &chars {
        if ch.is_ascii_digit() || *ch == '.' || *ch == ',' {
            number_end = idx + ch.len_utf8();
            continue;
        }
        break;
    }
    if number_end == 0 {
        return sanitize_translation_candidate(trimmed);
    }
    let remainder = trimmed
        .get(number_end..)
        .unwrap_or_default()
        .trim_start_matches(|value: char| {
            value.is_whitespace()
                || value == ','
                || value == '，'
                || value == '、'
                || value == ':'
                || value == '：'
                || value == '-'
        });
    sanitize_translation_candidate(remainder)
}

fn trim_before_leaked_number_anchor(
    text: &str,
    leaked_numbers: &HashSet<String>,
) -> Option<String> {
    if leaked_numbers.is_empty() {
        return None;
    }
    let mut chars = text.char_indices().peekable();
    while let Some((start, ch)) = chars.next() {
        if !ch.is_ascii_digit() {
            continue;
        }
        let mut end = start + ch.len_utf8();
        while let Some((idx, next_ch)) = chars.peek().copied() {
            if next_ch.is_ascii_digit() || next_ch == '.' || next_ch == ',' {
                end = idx + next_ch.len_utf8();
                chars.next();
                continue;
            }
            break;
        }
        let raw = text.get(start..end).unwrap_or_default();
        let normalized = normalize_numeric_value(parse_ascii_number(raw));
        if normalized.is_empty() || !leaked_numbers.contains(&normalized) {
            continue;
        }
        let head = text
            .get(..start)
            .unwrap_or_default()
            .trim_end_matches(|value: char| {
                value.is_whitespace()
                    || value == ','
                    || value == '，'
                    || value == '、'
                    || value == '：'
                    || value == ':'
                    || value == '-'
            });
        let trimmed = sanitize_translation_candidate(head);
        if trimmed.is_empty() {
            return None;
        }
        return Some(trimmed);
    }
    None
}

fn has_tail_ellipsis(text: &str) -> bool {
    let trimmed = text.trim_end();
    if trimmed.ends_with("...") || trimmed.ends_with('…') || trimmed.ends_with("。。") {
        return true;
    }
    let mut tail_marks = 0usize;
    for ch in trimmed.chars().rev() {
        if ch.is_whitespace() {
            continue;
        }
        if ch == '.' || ch == '。' || ch == '…' {
            tail_marks += 1;
            continue;
        }
        break;
    }
    tail_marks >= 2
}

fn strip_tail_ellipsis(text: &str) -> String {
    let mut out = text.trim_end().chars().collect::<Vec<_>>();
    while let Some(ch) = out.last().copied() {
        if ch.is_whitespace() || ch == '.' || ch == '…' || ch == '。' {
            out.pop();
            continue;
        }
        break;
    }
    normalize_inline_text(&out.into_iter().collect::<String>())
}

fn is_unusable_translation(text: &str) -> bool {
    let normalized = normalize_inline_text(text);
    if normalized.is_empty() {
        return true;
    }
    if has_tail_ellipsis(&normalized) {
        return true;
    }
    is_punctuation_only(&normalized)
}

fn is_punctuation_only(text: &str) -> bool {
    let normalized = normalize_inline_text(text);
    if normalized.is_empty() {
        return true;
    }
    !normalized.chars().any(is_meaningful_text_char)
}

fn is_meaningful_text_char(ch: char) -> bool {
    is_cjk_char(ch) || ch.is_ascii_alphanumeric() || ch.is_alphabetic() || ch.is_numeric()
}

fn extract_numbers(text: &str) -> HashSet<String> {
    let chars = text.chars().collect::<Vec<_>>();
    let mut out = HashSet::<String>::new();
    let mut index = 0usize;
    while index < chars.len() {
        if chars[index].is_ascii_digit() {
            let mut end = index;
            while end < chars.len()
                && (chars[end].is_ascii_digit() || chars[end] == '.' || chars[end] == ',')
            {
                end += 1;
            }
            let raw = chars[index..end].iter().collect::<String>();
            let mut value = parse_ascii_number(&raw);
            let prefix = chars[index.saturating_sub(24)..index]
                .iter()
                .collect::<String>()
                .to_ascii_lowercase();
            let prefix_trimmed = prefix.trim_end();
            if prefix_trimmed.ends_with("thousand and") {
                value += 1_000.0;
            } else if prefix_trimmed.ends_with("hundred and") {
                value += 100.0;
            }
            let mut next = end;
            let mut consumed_end = end;
            while next < chars.len() && chars[next].is_whitespace() {
                next += 1;
            }
            let has_gap = next > end;
            let has_trailing_punctuation = raw.ends_with(',')
                || raw.ends_with('.')
                || raw.ends_with('，')
                || raw.ends_with('。');
            if !has_trailing_punctuation {
                if let Some((multiplier, consumed)) = parse_number_suffix(&chars[next..], has_gap) {
                    value *= multiplier;
                    consumed_end = next + consumed;
                }
            }
            let normalized = normalize_numeric_value(value);
            if !normalized.is_empty() {
                out.insert(normalized);
            }
            index = consumed_end.max(index + 1);
            continue;
        }
        if is_chinese_number_char(chars[index]) {
            let mut end = index + 1;
            while end < chars.len() && is_chinese_number_char(chars[end]) {
                end += 1;
            }
            let raw = chars[index..end].iter().collect::<String>();
            if let Some(value) = parse_chinese_number(&raw) {
                let normalized = normalize_numeric_value(value);
                if !normalized.is_empty() {
                    out.insert(normalized);
                }
            }
            index = end;
            continue;
        }
        index += 1;
    }
    out
}

fn parse_ascii_number(raw: &str) -> f64 {
    let cleaned = raw
        .trim_matches(|value: char| value == '.' || value == ',')
        .replace(',', "");
    cleaned.parse::<f64>().unwrap_or(0.0)
}

fn parse_number_suffix(chars: &[char], has_gap: bool) -> Option<(f64, usize)> {
    let Some(first) = chars.first().copied() else {
        return None;
    };
    if !has_gap {
        match first {
            'k' | 'K' => return Some((1_000.0, 1)),
            'm' | 'M' => return Some((1_000_000.0, 1)),
            'b' | 'B' => return Some((1_000_000_000.0, 1)),
            'w' | 'W' | '万' => return Some((10_000.0, 1)),
            '亿' => return Some((100_000_000.0, 1)),
            '千' => return Some((1_000.0, 1)),
            '百' => return Some((100.0, 1)),
            _ => {}
        }
    }
    if !first.is_ascii_alphabetic() {
        return None;
    }
    let mut word = String::new();
    let mut consumed = 0usize;
    for ch in chars {
        if ch.is_ascii_alphabetic() {
            word.push(ch.to_ascii_lowercase());
            consumed += 1;
            continue;
        }
        break;
    }
    match word.as_str() {
        "k" => Some((1_000.0, consumed)),
        "m" => Some((1_000_000.0, consumed)),
        "b" => Some((1_000_000_000.0, consumed)),
        "grand" | "thousand" => Some((1_000.0, consumed)),
        "million" => Some((1_000_000.0, consumed)),
        "billion" => Some((1_000_000_000.0, consumed)),
        _ => None,
    }
}

fn normalize_numeric_value(value: f64) -> String {
    if !value.is_finite() || value < 0.0 {
        return String::new();
    }
    let rounded = (value * 1000.0).round() / 1000.0;
    if (rounded - rounded.round()).abs() < 1e-6 {
        return format!("{}", rounded.round() as i64);
    }
    let text = format!("{rounded:.3}");
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn is_chinese_number_char(ch: char) -> bool {
    matches!(
        ch,
        '零' | '〇'
            | '一'
            | '二'
            | '三'
            | '四'
            | '五'
            | '六'
            | '七'
            | '八'
            | '九'
            | '十'
            | '百'
            | '千'
            | '万'
            | '亿'
            | '两'
            | '壹'
            | '贰'
            | '叁'
            | '肆'
            | '伍'
            | '陆'
            | '柒'
            | '捌'
            | '玖'
            | '拾'
            | '佰'
            | '仟'
            | '第'
    )
}

fn numeric_alignment_penalty(source_numbers: &HashSet<String>, candidate: &str) -> usize {
    if source_numbers.is_empty() {
        return 0;
    }
    let candidate_numbers = extract_numbers(candidate);
    let missing = source_numbers
        .iter()
        .filter(|value| !candidate_numbers.contains(*value))
        .count();
    let extra = candidate_numbers
        .iter()
        .filter(|value| !source_numbers.contains(*value))
        .count();
    missing.saturating_mul(2).saturating_add(extra)
}

fn chinese_digit_value(ch: char) -> Option<i64> {
    match ch {
        '零' | '〇' => Some(0),
        '一' | '壹' => Some(1),
        '二' | '贰' | '两' => Some(2),
        '三' | '叁' => Some(3),
        '四' | '肆' => Some(4),
        '五' | '伍' => Some(5),
        '六' | '陆' => Some(6),
        '七' | '柒' => Some(7),
        '八' | '捌' => Some(8),
        '九' | '玖' => Some(9),
        _ => None,
    }
}

fn chinese_unit_value(ch: char) -> Option<i64> {
    match ch {
        '十' | '拾' => Some(10),
        '百' | '佰' => Some(100),
        '千' | '仟' => Some(1_000),
        '万' => Some(10_000),
        '亿' => Some(100_000_000),
        _ => None,
    }
}

fn parse_chinese_number(raw: &str) -> Option<f64> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let normalized = trimmed.trim_start_matches('第');
    if normalized.is_empty() {
        return None;
    }

    if normalized
        .chars()
        .all(|ch| chinese_digit_value(ch).is_some())
    {
        let mut value = 0i64;
        for ch in normalized.chars() {
            let digit = chinese_digit_value(ch)?;
            value = value.saturating_mul(10).saturating_add(digit);
        }
        return Some(value as f64);
    }

    let mut total = 0i64;
    let mut section = 0i64;
    let mut number = 0i64;
    let mut saw_numeric = false;

    for ch in normalized.chars() {
        if let Some(digit) = chinese_digit_value(ch) {
            number = digit;
            saw_numeric = true;
            continue;
        }
        let Some(unit) = chinese_unit_value(ch) else {
            return None;
        };
        saw_numeric = true;
        if unit < 10_000 {
            if number == 0 {
                number = 1;
            }
            section = section.saturating_add(number.saturating_mul(unit));
        } else {
            section = section.saturating_add(number);
            if section == 0 {
                section = 1;
            }
            total = total.saturating_add(section.saturating_mul(unit));
            section = 0;
        }
        number = 0;
    }

    if !saw_numeric {
        return None;
    }
    let value = total.saturating_add(section).saturating_add(number);
    if value <= 0 {
        return None;
    }
    Some(value as f64)
}

fn split_clauses(text: &str) -> Vec<String> {
    let mut out = Vec::<String>::new();
    let mut current = String::new();
    let chars = text.chars().collect::<Vec<_>>();
    for (index, ch) in chars.iter().enumerate() {
        let ch = *ch;
        current.push(ch);
        if is_clause_boundary_char(&chars, index) {
            let chunk = normalize_inline_text(&current);
            if !chunk.is_empty() {
                out.push(chunk);
            }
            current.clear();
        }
    }
    let tail = normalize_inline_text(&current);
    if !tail.is_empty() {
        out.push(tail);
    }
    out
}

fn is_clause_boundary_char(chars: &[char], index: usize) -> bool {
    let ch = chars.get(index).copied().unwrap_or_default();
    if matches!(ch, '.' | '!' | '?' | ';' | '。' | '！' | '？' | '；') {
        return true;
    }
    if !matches!(ch, ',' | '，' | '、' | ':' | '：') {
        return false;
    }
    let prev = index.checked_sub(1).and_then(|idx| chars.get(idx)).copied();
    let next = chars.get(index + 1).copied();
    if matches!(ch, ',' | ':')
        && prev.map(|value| value.is_ascii_digit()).unwrap_or(false)
        && next.map(|value| value.is_ascii_digit()).unwrap_or(false)
    {
        return false;
    }
    true
}

fn select_terms_for_text(
    source: &str,
    entries: &[Step5TerminologyEntry],
    max_terms: usize,
) -> Vec<Step5TerminologyEntry> {
    if entries.is_empty() {
        return Vec::new();
    }
    if max_terms == 0 {
        return Vec::new();
    }
    let source_lower = source.to_lowercase();
    let mut seen = HashSet::<String>::new();
    let mut picked = Vec::<Step5TerminologyEntry>::new();
    for entry in entries {
        if picked.len() >= max_terms {
            break;
        }
        let key = entry.source.trim().to_lowercase();
        if key.is_empty() || !source_contains_terminology_term(&source_lower, &key) {
            continue;
        }
        if !seen.insert(key) {
            continue;
        }
        picked.push(entry.clone());
    }
    picked
}
fn ends_with_sentence_punctuation(text: &str) -> bool {
    let t = text.trim_end();
    t.ends_with('.')
        || t.ends_with('!')
        || t.ends_with('?')
        || t.ends_with('。')
        || t.ends_with('！')
        || t.ends_with('？')
        || t.ends_with(';')
        || t.ends_with('；')
}

fn normalize_inline_text(raw: &str) -> String {
    raw.replace('\r', " ")
        .replace('\n', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests;
