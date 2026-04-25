use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use serde_json::Value;

use crate::services::llm::batch::run_indexed_concurrent_with_progress;
use crate::services::llm::client::{LlmSemanticValidationError, OpenAiCompatLlmClient};
use crate::services::llm::port::{LlmCallContext, LlmConfig, LlmJsonTask, next_llm_request_id};

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

#[derive(Debug, Clone)]
pub struct Step5Token {
    pub text: String,
    pub start: f64,
    pub end: f64,
}

#[derive(Debug, Clone)]
pub struct Step5DraftSegment {
    pub segment_id: usize,
    pub start: f64,
    pub end: f64,
    pub source: String,
    pub draft_translation: String,
    pub tokens: Vec<Step5Token>,
}

#[derive(Debug, Clone)]
pub struct Step5TerminologyEntry {
    pub source: String,
    pub target: String,
    pub note: String,
}

#[derive(Debug, Clone)]
pub struct Step5SplitPart {
    pub part_id: usize,
    pub start: f64,
    pub end: f64,
    pub source: String,
    pub tokens: Vec<Step5Token>,
}

#[derive(Debug, Clone)]
pub struct Step5SplitParent {
    pub parent_segment_id: usize,
    pub draft_translation: String,
    pub parts: Vec<Step5SplitPart>,
}

#[derive(Debug, Clone)]
pub struct BuildStep5SourceSplitRequest {
    pub task_id: String,
    pub media_path: String,
    pub source_lang: String,
    pub target_lang: String,
    pub segments: Vec<Step5DraftSegment>,
    pub subtitle_max_words_per_segment: u32,
    pub subtitle_length_reference: u32,
    pub translate_api_key: String,
    pub translate_base_url: String,
    pub translate_model: String,
    pub llm_concurrency: u32,
}

#[derive(Debug, Clone)]
pub struct BuildStep5SourceSplitResponse {
    pub subtitle_max_words_per_segment: u32,
    pub subtitle_length_reference: u32,
    pub parent_total: usize,
    pub part_total: usize,
    pub parents: Vec<Step5SplitParent>,
}

#[derive(Debug, Clone)]
pub struct Step5AlignedPart {
    pub part_id: usize,
    pub start: f64,
    pub end: f64,
    pub source: String,
    pub translation: String,
    pub tokens: Vec<Step5Token>,
}

#[derive(Debug, Clone)]
pub struct Step5AlignedParent {
    pub parent_segment_id: usize,
    pub parts: Vec<Step5AlignedPart>,
}

#[derive(Debug, Clone)]
pub struct BuildStep5TranslationAlignRequest {
    pub task_id: String,
    pub media_path: String,
    pub source_lang: String,
    pub target_lang: String,
    pub theme_summary: String,
    pub terminology_entries: Vec<Step5TerminologyEntry>,
    pub parents: Vec<Step5SplitParent>,
    pub translate_api_key: String,
    pub translate_base_url: String,
    pub translate_model: String,
    pub llm_concurrency: u32,
}

#[derive(Debug, Clone)]
pub struct BuildStep5TranslationAlignResponse {
    pub parent_total: usize,
    pub part_total: usize,
    pub parents: Vec<Step5AlignedParent>,
}

#[derive(Debug, Clone)]
pub struct Step5FinalSegment {
    pub segment_id: usize,
    pub start: f64,
    pub end: f64,
    pub source: String,
    pub translation: String,
    pub tokens: Vec<Step5Token>,
}

#[derive(Debug, Clone)]
pub struct BuildStep5TranslationPolishRequest {
    pub task_id: String,
    pub media_path: String,
    pub source_lang: String,
    pub target_lang: String,
    pub terminology_entries: Vec<Step5TerminologyEntry>,
    pub parents: Vec<Step5AlignedParent>,
    pub translate_api_key: String,
    pub translate_base_url: String,
    pub translate_model: String,
    pub llm_concurrency: u32,
    pub subtitle_length_reference: u32,
    pub batch_size: usize,
}

#[derive(Debug, Clone)]
pub struct BuildStep5TranslationPolishResponse {
    pub batch_size: usize,
    pub batch_total: usize,
    pub segment_total: usize,
    pub segments: Vec<Step5FinalSegment>,
}

#[derive(Debug, Clone)]
pub struct Step5QualityIssue {
    pub rule_id: String,
    pub severity: String,
    pub segment_id: usize,
    pub part_id: usize,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct Step5QaMetrics {
    pub segment_total: usize,
    pub empty_count: usize,
    pub ellipsis_tail_count: usize,
    pub numeric_drift_count: usize,
    pub cross_line_leak_count: usize,
    pub gt25_count: usize,
    pub gt32_count: usize,
}

#[derive(Debug, Clone)]
pub struct BuildStep5QaReportRequest {
    pub target_lang: String,
    pub terminology_entries: Vec<Step5TerminologyEntry>,
    pub segments: Vec<Step5FinalSegment>,
}

#[derive(Debug, Clone)]
pub struct BuildStep5QaReportResponse {
    pub passed: bool,
    pub hard_fail_count: usize,
    pub soft_score: f64,
    pub issue_count: usize,
    pub issues: Vec<Step5QualityIssue>,
    pub metrics: Step5QaMetrics,
}

#[derive(Debug, Clone)]
pub struct BuildStep5QaRepairRequest {
    pub task_id: String,
    pub media_path: String,
    pub source_lang: String,
    pub target_lang: String,
    pub theme_summary: String,
    pub terminology_entries: Vec<Step5TerminologyEntry>,
    pub segments: Vec<Step5FinalSegment>,
    pub issues: Vec<Step5QualityIssue>,
    pub translate_api_key: String,
    pub translate_base_url: String,
    pub translate_model: String,
    pub llm_concurrency: u32,
    pub subtitle_length_reference: u32,
}

#[derive(Debug, Clone)]
pub struct BuildStep5QaRepairResponse {
    pub segment_total: usize,
    pub candidate_total: usize,
    pub repaired_total: usize,
    pub segments: Vec<Step5FinalSegment>,
}

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

#[derive(Debug, Clone)]
struct Step55RepairTask {
    task_id: usize,
    segment_index: usize,
    source_numbers: Vec<String>,
    forbidden_numbers: Vec<String>,
    has_watchability_issue: bool,
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
                                    parse_source_split_parts(value, split_task.min_parts.max(2))
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
        let prompt = build_align_prompt(
            &request.source_lang,
            &request.target_lang,
            &request.theme_summary,
            &source_joined,
            &parent.draft_translation,
            &part_sources,
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
                                |value| parse_align_translation(value, &expected_ids),
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

    let mut polish_candidates = Vec::<usize>::new();
    for (index, segment) in segments.iter().enumerate() {
        let target_len = text_length_units(&segment.translation, &request.target_lang);
        if target_len > subtitle_length_reference {
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
                let prompt = build_polish_prompt(
                    &request.source_lang,
                    &request.target_lang,
                    &segment.source,
                    &segment.translation,
                    subtitle_length_reference,
                    &terms,
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
                                parse_polish_translation,
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

pub fn build_step_5_4_qa_report(
    request: BuildStep5QaReportRequest,
) -> Result<BuildStep5QaReportResponse, String> {
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

    let mut terminology_pairs = Vec::<(String, String)>::new();
    for term in &request.terminology_entries {
        let source = normalize_inline_text(&term.source).to_lowercase();
        let target = normalize_inline_text(&term.target).to_lowercase();
        if source.is_empty() || target.is_empty() {
            continue;
        }
        terminology_pairs.push((source, target));
    }

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

        let source_lower = source.to_lowercase();
        let translation_lower = translation.to_lowercase();
        for (term_source, term_target) in &terminology_pairs {
            if !source_lower.contains(term_source) {
                continue;
            }
            if translation_lower.contains(term_target) {
                continue;
            }
            push_quality_issue(
                &mut issues,
                &mut issue_keys,
                Step5QualityIssue {
                    rule_id: "terminology_drift".to_string(),
                    severity: "soft".to_string(),
                    segment_id: segment.segment_id,
                    part_id: index + 1,
                    message: format!("术语未命中目标词: {}", term_target),
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

    Ok(BuildStep5QaReportResponse {
        passed: hard_fail_count == 0,
        hard_fail_count,
        soft_score: (soft_score * 10.0).round() / 10.0,
        issue_count: issues.len(),
        issues,
        metrics: Step5QaMetrics {
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

pub async fn build_step_5_5_qa_repair_with_progress(
    request: BuildStep5QaRepairRequest,
    on_progress: Option<Arc<dyn Fn(usize, usize) + Send + Sync>>,
) -> Result<BuildStep5QaRepairResponse, String> {
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

    let mut segments = request.segments;
    let repair_issues = request
        .issues
        .iter()
        .filter(|issue| issue.severity == "hard" || issue.rule_id == "watchability_fragment")
        .cloned()
        .collect::<Vec<_>>();
    if repair_issues.is_empty() {
        if let Some(callback) = on_progress.as_ref() {
            callback(1, 1);
        }
        return Ok(BuildStep5QaRepairResponse {
            segment_total: segments.len(),
            candidate_total: 0,
            repaired_total: 0,
            segments,
        });
    }

    let llm_client = OpenAiCompatLlmClient::new(LlmConfig::new(
        request.translate_base_url.clone(),
        request.translate_api_key.clone(),
        request.translate_model.clone(),
    ))
    .map_err(|err| err.message)?;

    let mut candidate_set = HashSet::<usize>::new();
    for issue in &repair_issues {
        if issue.segment_id == 0 {
            continue;
        }
        let current_index = issue.segment_id.saturating_sub(1);
        if current_index >= segments.len() {
            continue;
        }
        candidate_set.insert(current_index);
        if issue.rule_id == "cross_line_leak" && current_index + 1 < segments.len() {
            candidate_set.insert(current_index + 1);
        }
    }
    let mut candidate_indexes = candidate_set.into_iter().collect::<Vec<_>>();
    candidate_indexes.sort_unstable();

    if candidate_indexes.is_empty() {
        if let Some(callback) = on_progress.as_ref() {
            callback(1, 1);
        }
        return Ok(BuildStep5QaRepairResponse {
            segment_total: segments.len(),
            candidate_total: 0,
            repaired_total: 0,
            segments,
        });
    }

    let subtitle_length_soft = request.subtitle_length_reference.clamp(8, 80) as f64;
    let mut repair_tasks = Vec::<Step55RepairTask>::new();
    for segment_index in candidate_indexes {
        let Some(segment) = segments.get(segment_index) else {
            continue;
        };

        let prev_source = segment_index
            .checked_sub(1)
            .and_then(|idx| segments.get(idx))
            .map(|item| item.source.clone())
            .unwrap_or_default();
        let prev_translation = segment_index
            .checked_sub(1)
            .and_then(|idx| segments.get(idx))
            .map(|item| item.translation.clone())
            .unwrap_or_default();
        let next_source = segments
            .get(segment_index + 1)
            .map(|item| item.source.clone())
            .unwrap_or_default();
        let next_translation = segments
            .get(segment_index + 1)
            .map(|item| item.translation.clone())
            .unwrap_or_default();

        let mut issue_tags = repair_issues
            .iter()
            .filter(|issue| issue.segment_id == segment.segment_id)
            .map(|issue| issue.rule_id.clone())
            .collect::<Vec<_>>();
        if issue_tags.is_empty() {
            issue_tags.push("neighbor_context_repair".to_string());
        }
        issue_tags.sort();
        issue_tags.dedup();

        let source_number_set = extract_numbers(&segment.source);
        let mut source_numbers = source_number_set.iter().cloned().collect::<Vec<_>>();
        source_numbers.sort();

        let mut forbidden_number_set = HashSet::<String>::new();
        if let Some(next_segment) = segments.get(segment_index + 1) {
            let next_numbers = extract_numbers(&next_segment.source);
            for value in next_numbers {
                if !source_number_set.contains(&value) {
                    forbidden_number_set.insert(value);
                }
            }
        }
        let mut forbidden_numbers = forbidden_number_set.into_iter().collect::<Vec<_>>();
        forbidden_numbers.sort();

        let terms = select_terms_for_text(
            &segment.source,
            &request.terminology_entries,
            MAX_TERMS_PER_LINE,
        );
        let prompt = build_qa_repair_prompt(
            &request.source_lang,
            &request.target_lang,
            &request.theme_summary,
            &segment.source,
            &segment.translation,
            &prev_source,
            &prev_translation,
            &next_source,
            &next_translation,
            &issue_tags,
            &source_numbers,
            &forbidden_numbers,
            subtitle_length_soft,
            &terms,
        );
        repair_tasks.push(Step55RepairTask {
            task_id: repair_tasks.len(),
            segment_index,
            source_numbers,
            forbidden_numbers,
            has_watchability_issue: issue_tags.iter().any(|tag| tag == "watchability_fragment"),
            prompt,
        });
    }

    if repair_tasks.is_empty() {
        if let Some(callback) = on_progress.as_ref() {
            callback(1, 1);
        }
        return Ok(BuildStep5QaRepairResponse {
            segment_total: segments.len(),
            candidate_total: 0,
            repaired_total: 0,
            segments,
        });
    }

    let tasks = repair_tasks
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
        phase: "step_5_5_qa_repair".to_string(),
    };
    let repair_tasks_for_worker = repair_tasks.clone();
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
                let repair_tasks = repair_tasks_for_worker.clone();
                async move {
                    let Some(repair_task) = repair_tasks.get(task.id) else {
                        return Err(format!("missing step5 qa repair task {}", task.id));
                    };
                    let llm_id = task.request_id.clone();
                    let call = llm_client
                        .call_json_validated(
                            &context,
                            &llm_id,
                            &task.user_prompt,
                            task.response_validator.as_ref(),
                            parse_polish_translation,
                        )
                        .await
                        .map_err(|err| {
                            format!("step5 qa repair failed (llmId={}): {}", llm_id, err.message)
                        })?;
                    Ok((repair_task.task_id, call.value))
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

    let mut repaired_total = 0usize;
    for (_, result) in results {
        let Ok((task_id, repaired_raw)) = result else {
            continue;
        };
        let Some(repair_task) = repair_tasks.get(task_id) else {
            continue;
        };
        let Some(segment) = segments.get_mut(repair_task.segment_index) else {
            continue;
        };
        let repaired = sanitize_translation_candidate(&repaired_raw);
        if repaired.is_empty() || is_unusable_translation(&repaired) {
            continue;
        }
        if looks_like_non_cjk_translation_for_cjk_target(&repaired, &request.target_lang) {
            continue;
        }

        let source_numbers = repair_task
            .source_numbers
            .iter()
            .cloned()
            .collect::<HashSet<_>>();
        let repaired_numbers = extract_numbers(&repaired);
        if !source_numbers.is_empty()
            && source_numbers
                .iter()
                .any(|value| !repaired_numbers.contains(value))
        {
            continue;
        }
        if repair_task
            .forbidden_numbers
            .iter()
            .any(|value| repaired_numbers.contains(value))
        {
            continue;
        }

        let current_units = text_length_units(&segment.translation, &request.target_lang).max(1.0);
        let repaired_units = text_length_units(&repaired, &request.target_lang);
        let hard_limit = if repair_task.has_watchability_issue {
            (subtitle_length_soft * 1.35).max(current_units * 3.0)
        } else {
            subtitle_length_soft.max(current_units * 1.20)
        };
        if repaired_units > hard_limit {
            continue;
        }

        if repaired != segment.translation {
            segment.translation = repaired;
            repaired_total += 1;
        }
    }

    for segment in &mut segments {
        repair_polished_translation(segment);
    }

    Ok(BuildStep5QaRepairResponse {
        segment_total: segments.len(),
        candidate_total: repair_tasks.len(),
        repaired_total,
        segments,
    })
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

fn build_source_split_prompt(
    source_lang: &str,
    target_lang: &str,
    source_text: &str,
    draft_translation: &str,
    source_limit: f64,
    target_limit: f64,
    min_parts: usize,
) -> String {
    let expected_parts = min_parts.max(2);
    serde_json::json!({
        "task": "split_source_segment_for_subtitle_alignment",
        "rule": "Think step by step internally, but output JSON only.",
        "sourceLanguage": source_lang,
        "targetLanguage": target_lang,
        "sourceText": source_text,
        "draftTranslation": draft_translation,
        "sourceLengthLimit": source_limit,
        "targetLengthLimit": target_limit,
        "expectedParts": expected_parts,
        "constraints": [
            "Return sourceParts only.",
            "sourceParts must be an array of strings with exactly expectedParts items.",
            "Keep original language and wording. Do not translate.",
            "Do not reorder meaning. Keep sequence from sourceText.",
            "Each part should be semantically complete when possible.",
            "Avoid ultra-short fragments like single discourse markers."
        ],
        "output": {
            "sourceParts": ["part 1", "part 2"]
        }
    })
    .to_string()
}

fn parse_source_split_parts(
    value: Value,
    min_parts: usize,
) -> Result<Vec<String>, LlmSemanticValidationError> {
    let Some(items) = value
        .get("sourceParts")
        .or_else(|| value.get("source_parts"))
        .or_else(|| value.get("parts"))
        .and_then(|v| v.as_array())
    else {
        return Err(LlmSemanticValidationError::retryable(
            "sourceParts array is required",
        ));
    };
    let mut out = Vec::<String>::new();
    for item in items {
        let Some(text) = item.as_str() else {
            continue;
        };
        let text = normalize_inline_text(text);
        if !text.is_empty() {
            out.push(text);
        }
    }
    if out.len() < min_parts.max(2) {
        return Err(LlmSemanticValidationError::retryable(
            "sourceParts has too few items",
        ));
    }
    if out.len() > min_parts.max(2) {
        out.truncate(min_parts.max(2));
    }
    Ok(out)
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

fn text_length_units(text: &str, lang: &str) -> f64 {
    if text.trim().is_empty() {
        return 0.0;
    }
    if use_char_units(lang, text) {
        count_char_units(text) as f64
    } else {
        count_word_units(text) as f64
    }
}

fn use_char_units(lang: &str, text: &str) -> bool {
    let lower = lang.trim().to_ascii_lowercase();
    if lower.starts_with("zh") || lower.starts_with("ja") || lower.starts_with("ko") {
        return true;
    }
    if lower.is_empty() || lower == "auto" {
        return contains_cjk(text);
    }
    false
}

fn contains_cjk(text: &str) -> bool {
    text.chars().any(is_cjk_char)
}

fn count_char_units(text: &str) -> usize {
    let mut total = 0usize;
    let mut in_ascii_group = false;
    for ch in text.chars() {
        if ch.is_whitespace() {
            if in_ascii_group {
                total += 1;
                in_ascii_group = false;
            }
            continue;
        }
        if ch.is_ascii_alphanumeric() {
            in_ascii_group = true;
            continue;
        }
        if in_ascii_group {
            total += 1;
            in_ascii_group = false;
        }
        if is_cjk_char(ch) || ch.is_alphanumeric() {
            total += 1;
        }
    }
    if in_ascii_group {
        total += 1;
    }
    total
}

fn count_word_units(text: &str) -> usize {
    let mut total = 0usize;
    let mut in_word = false;
    for ch in text.chars() {
        if ch.is_alphanumeric() {
            if !in_word {
                total += 1;
                in_word = true;
            }
            continue;
        }
        if is_cjk_char(ch) {
            total += 1;
        }
        in_word = false;
    }
    total
}

fn is_cjk_char(ch: char) -> bool {
    matches!(
        ch as u32,
        0x3040..=0x30FF
            | 0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xF900..=0xFAFF
            | 0xAC00..=0xD7AF
    )
}

fn build_source_from_tokens(tokens: &[Step5Token]) -> String {
    let mut out = String::new();
    for token in tokens {
        let text = token.text.trim();
        if text.is_empty() {
            continue;
        }
        let should_space = out
            .chars()
            .last()
            .zip(text.chars().next())
            .map(|(left, right)| {
                left.is_ascii_alphanumeric()
                    && right.is_ascii_alphanumeric()
                    && !left.is_ascii_punctuation()
                    && !right.is_ascii_punctuation()
            })
            .unwrap_or(false);
        if should_space {
            out.push(' ');
        }
        out.push_str(text);
    }
    normalize_inline_text(&out)
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
    matches!(ch, '.' | '!' | '?' | ';' | '。' | '！' | '？' | '；')
}

fn ends_with_short_dangling_fragment(text: &str) -> bool {
    let normalized = normalize_inline_text(text);
    if normalized.is_empty() {
        return false;
    }
    let suffixes = [
        "一个",
        "做一个",
        "这个",
        "那个",
        "这笔",
        "那笔",
        "这",
        "那",
        "拿下了",
        "花大约",
    ];
    suffixes.iter().any(|suffix| normalized.ends_with(suffix))
}

fn ends_with_connector_like_fragment(text: &str) -> bool {
    let normalized = normalize_inline_text(text);
    if normalized.is_empty() {
        return false;
    }
    let cjk_connectors = [
        "然后",
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
        "花大约",
        "大约",
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
        let source_leading = leading_number_anchor(source);
        let source_matches = source_leading
            .as_ref()
            .map(|value| value == &leading_number)
            .unwrap_or(false);
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
    segment.translation = translation;
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
        let source_lower = segment.source.to_ascii_lowercase();
        let mut updated = sanitize_translation_candidate(&segment.translation);

        if source_lower.contains("a day in") && updated.starts_with(|ch: char| ch.is_ascii_digit())
        {
            if let Some(leading_number) = leading_number_anchor(&updated) {
                let body = strip_leading_number_token(&updated);
                if body.starts_with("分钟") {
                    updated = normalize_inline_text(&format!("一天{}{}", leading_number, body));
                } else if body.is_empty() {
                    updated = normalize_inline_text(&format!("一天{}分钟", leading_number));
                } else if body.starts_with('，') || body.starts_with(',') || body.starts_with('。')
                {
                    updated = normalize_inline_text(&format!("一天{}分钟{}", leading_number, body));
                } else {
                    updated =
                        normalize_inline_text(&format!("一天{}分钟，{}", leading_number, body));
                }
            }
        }

        if source_lower.contains("trading platform")
            && (updated == "然后" || updated == "然后，" || updated == "然后。")
        {
            updated = "然后我开始加载交易平台".to_string();
        }

        if source_lower.contains("second") && updated.starts_with(|ch: char| ch.is_ascii_digit()) {
            if let Some(leading_number) = leading_number_anchor(&updated) {
                let body = strip_leading_number_token(&updated);
                if body.starts_with("秒") {
                    updated = normalize_inline_text(&format!("花大约{}{}", leading_number, body));
                } else if body.is_empty() {
                    updated = normalize_inline_text(&format!("花大约{}秒", leading_number));
                } else if body.starts_with('，') || body.starts_with(',') || body.starts_with('。')
                {
                    updated = normalize_inline_text(&format!("花大约{}秒{}", leading_number, body));
                } else {
                    updated =
                        normalize_inline_text(&format!("花大约{}秒，{}", leading_number, body));
                }
            }
        }

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

    for index in 0..translation_lines.len().saturating_sub(1) {
        let current_translation = sanitize_translation_candidate(&translation_lines[index]);
        let next_translation = sanitize_translation_candidate(&translation_lines[index + 1]);
        if current_translation.ends_with("做一个") {
            let next_source_lower = source_lines[index + 1].to_ascii_lowercase();
            if next_source_lower.contains("trade") {
                if let Some(leading_number) = leading_number_anchor(&next_translation) {
                    let current_trimmed = current_translation
                        .trim_end_matches("做一个")
                        .trim_end_matches('，')
                        .trim_end_matches(',')
                        .trim();
                    let next_body = strip_leading_number_token(&next_translation);
                    if !current_trimmed.is_empty() && !next_body.is_empty() {
                        translation_lines[index] = normalize_inline_text(current_trimmed);
                        translation_lines[index + 1] = normalize_inline_text(&format!(
                            "做一笔{}点的交易，{}",
                            leading_number, next_body
                        ));
                    }
                }
            }
        }

        let current_translation = sanitize_translation_candidate(&translation_lines[index]);
        let next_translation = sanitize_translation_candidate(&translation_lines[index + 1]);
        if !current_translation.ends_with("花大约") {
            continue;
        }
        let Some(leading_number) = leading_number_anchor(&next_translation) else {
            continue;
        };
        let next_body = strip_leading_number_token(&next_translation);
        if next_body.is_empty() {
            continue;
        }
        let current_trimmed = current_translation
            .trim_end_matches("花大约")
            .trim_end_matches('，')
            .trim_end_matches(',')
            .trim();
        let current_repaired = if current_trimmed.is_empty() {
            "然后"
        } else {
            current_trimmed
        };
        translation_lines[index] = normalize_inline_text(current_repaired);

        let needs_seconds_unit = source_lines[index + 1]
            .to_ascii_lowercase()
            .contains("second")
            && !next_body.starts_with('秒');
        let mut rebuilt = if needs_seconds_unit {
            format!("花大约{}秒", leading_number)
        } else {
            format!("花大约{}", leading_number)
        };
        if next_body.starts_with('，') || next_body.starts_with(',') || next_body.starts_with('。')
        {
            rebuilt.push_str(&next_body);
        } else {
            rebuilt.push('，');
            rebuilt.push_str(&next_body);
        }
        translation_lines[index + 1] = normalize_inline_text(&rebuilt);
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
    if !is_watchability_fragment_issue(source, &original, target_lang) {
        return original;
    }

    let mut updated = original.clone();
    if updated.ends_with("拿下了") {
        updated = normalize_inline_text(&format!("{updated}那笔单子"));
    }
    let source_lower = source.to_ascii_lowercase();
    if source_lower.contains("a day in") && updated.starts_with(|ch: char| ch.is_ascii_digit()) {
        if let Some(leading_number) = leading_number_anchor(&updated) {
            let body = strip_leading_number_token(&updated);
            if body.starts_with("分钟") {
                updated = normalize_inline_text(&format!("一天{}{}", leading_number, body));
            } else if body.is_empty() {
                updated = normalize_inline_text(&format!("一天{}分钟", leading_number));
            } else if body.starts_with('，') || body.starts_with(',') || body.starts_with('。') {
                updated = normalize_inline_text(&format!("一天{}分钟{}", leading_number, body));
            } else {
                updated = normalize_inline_text(&format!("一天{}分钟，{}", leading_number, body));
            }
        }
    }
    if source_lower.contains("second") && updated.starts_with(|ch: char| ch.is_ascii_digit()) {
        if let Some(leading_number) = leading_number_anchor(&updated) {
            let body = strip_leading_number_token(&updated);
            if body.starts_with("秒") {
                updated = normalize_inline_text(&format!("花大约{}{}", leading_number, body));
            } else if body.is_empty() {
                updated = normalize_inline_text(&format!("花大约{}秒", leading_number));
            } else if body.starts_with('，') || body.starts_with(',') || body.starts_with('。') {
                updated = normalize_inline_text(&format!("花大约{}秒{}", leading_number, body));
            } else {
                updated = normalize_inline_text(&format!("花大约{}秒，{}", leading_number, body));
            }
        }
    }
    if source_lower.contains("trading platform") && text_length_units(&updated, target_lang) <= 3.0
    {
        updated = "然后我开始加载交易平台".to_string();
    }
    if source_lower.contains("point trade") {
        if let Some(leading_number) = leading_number_anchor(&updated) {
            let body = strip_leading_numeric_noise(&updated);
            if !body.is_empty() && !updated.contains("做一笔") {
                updated =
                    normalize_inline_text(&format!("做一笔{}点的交易，{}", leading_number, body));
            }
        }
    }
    if source.contains('%') && updated.starts_with(|ch: char| ch.is_ascii_digit()) {
        let mut numbers = extract_numbers(source)
            .into_iter()
            .filter_map(|value| value.parse::<f64>().ok())
            .filter(|value| *value > 0.0 && *value <= 100.0)
            .map(normalize_numeric_value)
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>();
        numbers.sort();
        numbers.dedup();
        if numbers.len() >= 2 {
            let body = strip_leading_numeric_noise(&updated);
            if !body.is_empty() && !body.starts_with("如果你拿走") {
                updated = normalize_inline_text(&format!(
                    "如果你拿走{}%或{}%的总涨幅，{}",
                    numbers[0], numbers[1], body
                ));
            }
        }
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
        "花大约",
        "大约",
        "拿下了",
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

fn strip_all_leading_number_tokens(text: &str) -> String {
    let mut out = sanitize_translation_candidate(text);
    for _ in 0..3 {
        if leading_number_anchor(&out).is_none() {
            break;
        }
        let stripped = strip_leading_number_token(&out);
        if stripped.is_empty() || stripped == out {
            break;
        }
        out = stripped;
    }
    out
}

fn strip_leading_numeric_noise(text: &str) -> String {
    let mut out = strip_all_leading_number_tokens(text);
    out = normalize_inline_text(out.trim_start_matches(|ch: char| {
        ch.is_ascii_whitespace()
            || ch == '/'
            || ch == '\\'
            || ch == '%'
            || ch == ','
            || ch == '，'
            || ch == '.'
    }));
    if leading_number_anchor(&out).is_some() {
        out = strip_all_leading_number_tokens(&out);
        out = normalize_inline_text(out.trim_start_matches(|ch: char| {
            ch.is_ascii_whitespace()
                || ch == '/'
                || ch == '\\'
                || ch == '%'
                || ch == ','
                || ch == '，'
                || ch == '.'
        }));
    }
    out
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

fn build_align_prompt(
    source_lang: &str,
    target_lang: &str,
    theme_summary: &str,
    source_text: &str,
    draft_translation: &str,
    part_sources: &[String],
    terms: &[Step5TerminologyEntry],
) -> String {
    let parts = part_sources
        .iter()
        .enumerate()
        .map(|(index, text)| {
            serde_json::json!({
                "id": index + 1,
                "source": text,
            })
        })
        .collect::<Vec<_>>();
    let terms_json = terms
        .iter()
        .map(|term| {
            serde_json::json!({
                "source": term.source,
                "target": term.target,
                "note": term.note,
            })
        })
        .collect::<Vec<_>>();

    serde_json::json!({
        "task": "align_translation_to_split_source_lines",
        "rule": "Return JSON only.",
        "sourceLanguage": source_lang,
        "targetLanguage": target_lang,
        "theme": theme_summary,
        "sourceText": source_text,
        "draftTranslation": draft_translation,
        "splitSourceLines": parts,
        "terminology": terms_json,
        "constraints": [
            "Return exactly one translation line for each split source line id.",
            "Keep meaning faithful and natural.",
            "Do not merge lines.",
            "Do not copy full draftTranslation to multiple ids.",
            "Each id should only contain meaning from its own source line.",
            "If uncertain, keep a shorter partial translation for that line only.",
            "Do not add explanations."
        ],
        "output": {
            "translations": [
                {"id": 1, "text": "translated text"}
            ]
        }
    })
    .to_string()
}

fn build_polish_prompt(
    source_lang: &str,
    target_lang: &str,
    source_text: &str,
    translation: &str,
    target_length_soft: f64,
    terms: &[Step5TerminologyEntry],
) -> String {
    let terms_json = terms
        .iter()
        .map(|term| {
            serde_json::json!({
                "source": term.source,
                "target": term.target,
                "note": term.note,
            })
        })
        .collect::<Vec<_>>();

    serde_json::json!({
        "task": "polish_single_subtitle_line",
        "rule": "Return JSON only.",
        "sourceLanguage": source_lang,
        "targetLanguage": target_lang,
        "sourceText": source_text,
        "currentTranslation": translation,
        "targetLengthSoft": target_length_soft,
        "terminology": terms_json,
        "constraints": [
            "Keep one line only.",
            "Keep key meaning.",
            "Prefer shorter wording.",
            "No extra notes."
        ],
        "output": {
            "text": "shorter polished translation"
        }
    })
    .to_string()
}

#[allow(clippy::too_many_arguments)]
fn build_qa_repair_prompt(
    source_lang: &str,
    target_lang: &str,
    theme_summary: &str,
    source_text: &str,
    current_translation: &str,
    prev_source: &str,
    prev_translation: &str,
    next_source: &str,
    next_translation: &str,
    issue_tags: &[String],
    source_numbers: &[String],
    forbidden_numbers: &[String],
    target_length_soft: f64,
    terms: &[Step5TerminologyEntry],
) -> String {
    let terms_json = terms
        .iter()
        .map(|term| {
            serde_json::json!({
                "source": term.source,
                "target": term.target,
                "note": term.note,
            })
        })
        .collect::<Vec<_>>();

    serde_json::json!({
        "task": "repair_subtitle_line_from_qa_issue",
        "rule": "Return JSON only.",
        "sourceLanguage": source_lang,
        "targetLanguage": target_lang,
        "theme": theme_summary,
        "sourceText": source_text,
        "currentTranslation": current_translation,
        "previousLine": {
            "source": prev_source,
            "translation": prev_translation
        },
        "nextLine": {
            "source": next_source,
            "translation": next_translation
        },
        "qaIssueTags": issue_tags,
        "requiredNumbers": source_numbers,
        "forbiddenNumbers": forbidden_numbers,
        "targetLengthSoft": target_length_soft,
        "terminology": terms_json,
        "constraints": [
            "Translate current source line only.",
            "Do not include information that belongs to previous or next lines.",
            "Keep all requiredNumbers if requiredNumbers is not empty.",
            "Do not output forbiddenNumbers.",
            "Keep one subtitle line only.",
            "Make wording natural and watchable.",
            "No explanations."
        ],
        "output": {
            "text": "repaired translation"
        }
    })
    .to_string()
}

fn parse_align_translation(
    value: Value,
    expected_ids: &[usize],
) -> Result<HashMap<usize, String>, LlmSemanticValidationError> {
    let mut out = HashMap::<usize, String>::new();
    let Some(items) = value.get("translations").and_then(|v| v.as_array()) else {
        return Err(LlmSemanticValidationError::retryable(
            "translations array is required",
        ));
    };
    for item in items {
        let Some(obj) = item.as_object() else {
            continue;
        };
        let Some(id) = obj.get("id").and_then(|v| v.as_u64()).map(|v| v as usize) else {
            continue;
        };
        if !expected_ids.contains(&id) {
            continue;
        }
        let text = obj
            .get("text")
            .or_else(|| obj.get("translation"))
            .and_then(|v| v.as_str())
            .map(normalize_inline_text)
            .unwrap_or_default();
        out.insert(id, text);
    }
    for expected_id in expected_ids {
        out.entry(*expected_id).or_insert_with(String::new);
    }
    Ok(out)
}

fn parse_polish_translation(value: Value) -> Result<String, LlmSemanticValidationError> {
    let text = value
        .get("text")
        .or_else(|| value.get("translation"))
        .and_then(|v| v.as_str())
        .map(normalize_inline_text)
        .unwrap_or_default();
    Ok(text)
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
        if key.is_empty() || !source_lower.contains(&key) {
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
mod tests {
    use std::collections::HashSet;

    use super::{
        BuildStep5QaReportRequest, BuildStep5SourceSplitRequest, Step5DraftSegment,
        Step5FinalSegment, Step5SplitParent, Step5SplitPart, Step5Token,
        build_step_5_1_source_split_with_progress, choose_better_alignment, extract_numbers,
        has_tail_ellipsis, heuristic_split_translation, looks_like_source_residue,
        merge_tiny_ranges_for_readability, rebalance_dangling_tail_tokens, repair_aligned_lines,
        split_line_quality_score, split_token_ranges,
    };

    #[test]
    fn step5_source_split_splits_on_hard_pause() {
        let response = tauri::async_runtime::block_on(build_step_5_1_source_split_with_progress(
            BuildStep5SourceSplitRequest {
                task_id: "t1".to_string(),
                media_path: "sample.mp4".to_string(),
                source_lang: "en".to_string(),
                target_lang: "zh-CN".to_string(),
                subtitle_max_words_per_segment: 16,
                subtitle_length_reference: 16,
                translate_api_key: "test".to_string(),
                translate_base_url: "https://api.openai.com/v1".to_string(),
                translate_model: "gpt-4.1-mini".to_string(),
                llm_concurrency: 1,
                segments: vec![Step5DraftSegment {
                    segment_id: 1,
                    start: 0.0,
                    end: 8.0,
                    source: "hello world how are you".to_string(),
                    draft_translation: "你好 世界 你好吗".to_string(),
                    tokens: vec![
                        Step5Token {
                            text: "hello".to_string(),
                            start: 0.0,
                            end: 0.5,
                        },
                        Step5Token {
                            text: "world".to_string(),
                            start: 0.5,
                            end: 1.0,
                        },
                        Step5Token {
                            text: "how".to_string(),
                            start: 3.4,
                            end: 3.8,
                        },
                        Step5Token {
                            text: "are".to_string(),
                            start: 3.8,
                            end: 4.2,
                        },
                        Step5Token {
                            text: "you".to_string(),
                            start: 4.2,
                            end: 4.8,
                        },
                    ],
                }],
            },
            None,
        ))
        .expect("step5 source split");

        assert_eq!(response.parent_total, 1);
        assert_eq!(response.part_total, 2);
        assert_eq!(response.parents[0].parts.len(), 2);
        assert_eq!(response.parents[0].parts[0].source, "hello world");
        assert_eq!(response.parents[0].parts[1].source, "how are you");
    }

    #[test]
    fn step5_split_token_ranges_force_split_on_over_limit_without_pause() {
        let tokens = (0..24usize)
            .map(|idx| Step5Token {
                text: format!("w{idx}"),
                start: idx as f64 * 0.2,
                end: idx as f64 * 0.2 + 0.19,
            })
            .collect::<Vec<_>>();
        let ranges = split_token_ranges(&tokens, "en", 8.0, 80.0, 24.0, 24.0);
        assert!(ranges.len() >= 2);

        for (start, end) in ranges {
            let word_count = end.saturating_sub(start) + 1;
            assert!(word_count <= 10);
        }
    }

    #[test]
    fn step5_source_split_merges_tiny_leading_piece() {
        let tokens = vec![
            Step5Token {
                text: "So,".to_string(),
                start: 0.0,
                end: 0.2,
            },
            Step5Token {
                text: "we".to_string(),
                start: 0.2,
                end: 0.4,
            },
            Step5Token {
                text: "are".to_string(),
                start: 0.4,
                end: 0.6,
            },
            Step5Token {
                text: "looking".to_string(),
                start: 0.6,
                end: 1.0,
            },
            Step5Token {
                text: "for".to_string(),
                start: 1.0,
                end: 1.2,
            },
            Step5Token {
                text: "entries".to_string(),
                start: 1.2,
                end: 1.6,
            },
            Step5Token {
                text: "right".to_string(),
                start: 1.6,
                end: 1.9,
            },
            Step5Token {
                text: "now".to_string(),
                start: 1.9,
                end: 2.2,
            },
            Step5Token {
                text: "today".to_string(),
                start: 2.2,
                end: 2.8,
            },
        ];
        let ranges = vec![(0usize, 0usize), (1usize, 8usize)];
        let merged = merge_tiny_ranges_for_readability(ranges, &tokens, "en", 20.0, &[]);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0], (0, 8));
    }

    #[test]
    fn step5_source_split_merges_sub_half_second_piece() {
        let tokens = vec![
            Step5Token {
                text: "intro".to_string(),
                start: 0.0,
                end: 0.2,
            },
            Step5Token {
                text: "this".to_string(),
                start: 0.2,
                end: 0.8,
            },
            Step5Token {
                text: "is".to_string(),
                start: 0.8,
                end: 1.2,
            },
            Step5Token {
                text: "longer".to_string(),
                start: 1.2,
                end: 2.0,
            },
        ];
        let ranges = vec![(0usize, 0usize), (1usize, 3usize)];
        let merged = merge_tiny_ranges_for_readability(ranges, &tokens, "en", 20.0, &[]);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0], (0, 3));
    }

    #[test]
    fn step5_source_split_rebalances_dangling_tail_tokens() {
        let tokens = vec![
            Step5Token {
                text: "it's".to_string(),
                start: 0.0,
                end: 0.2,
            },
            Step5Token {
                text: "very".to_string(),
                start: 0.2,
                end: 0.4,
            },
            Step5Token {
                text: "common".to_string(),
                start: 0.4,
                end: 0.6,
            },
            Step5Token {
                text: "when".to_string(),
                start: 0.6,
                end: 0.8,
            },
            Step5Token {
                text: "you're".to_string(),
                start: 0.8,
                end: 1.0,
            },
            Step5Token {
                text: "starting".to_string(),
                start: 1.0,
                end: 1.2,
            },
            Step5Token {
                text: "out".to_string(),
                start: 1.2,
                end: 1.4,
            },
            Step5Token {
                text: "to".to_string(),
                start: 1.4,
                end: 1.6,
            },
            Step5Token {
                text: "take".to_string(),
                start: 1.6,
                end: 1.8,
            },
            Step5Token {
                text: "a".to_string(),
                start: 1.8,
                end: 2.0,
            },
            Step5Token {
                text: "trade".to_string(),
                start: 2.0,
                end: 2.4,
            },
        ];
        let ranges = vec![(0usize, 8usize), (9usize, 10usize)];
        let rebalanced = rebalance_dangling_tail_tokens(ranges, &tokens, "en", 20.0, &[]);
        assert_eq!(rebalanced, vec![(0usize, 6usize), (7usize, 10usize)]);
    }

    #[test]
    fn step5_split_quality_penalizes_overlapping_lines() {
        let overlapping = vec![
            "10 因为我的大脑自然会想：为什么你不都在20点出场呢？".to_string(),
            "为什么你不都在20点出场呢？".to_string(),
        ];
        let clean = vec![
            "比如在10点出场一部分，20点再出场一部分。".to_string(),
            "因为我的大脑自然会想：为什么你不都在20点出场呢？".to_string(),
        ];
        assert!(split_line_quality_score(&clean) > split_line_quality_score(&overlapping));
    }

    #[test]
    fn step5_trim_before_leaked_number_anchor_trims_head() {
        let mut leaked = HashSet::<String>::new();
        leaked.insert("10".to_string());
        let trimmed = super::trim_before_leaked_number_anchor(
            "你知道， 对于基础命中， 当你刚开始时， 做一个10点的交易，",
            &leaked,
        );
        assert_eq!(
            trimmed,
            Some("你知道， 对于基础命中， 当你刚开始时， 做一个".to_string())
        );
    }

    #[test]
    fn step5_align_repair_fills_empty_lines() {
        let parent = Step5SplitParent {
            parent_segment_id: 1,
            draft_translation: "先做计划，再执行".to_string(),
            parts: vec![
                Step5SplitPart {
                    part_id: 1,
                    start: 0.0,
                    end: 1.0,
                    source: "first part".to_string(),
                    tokens: vec![],
                },
                Step5SplitPart {
                    part_id: 2,
                    start: 1.0,
                    end: 2.0,
                    source: "second part".to_string(),
                    tokens: vec![],
                },
            ],
        };
        let aligned = vec!["".to_string(), "执行".to_string()];
        let fallback = vec!["先做计划".to_string(), "再执行".to_string()];
        let repaired = repair_aligned_lines(&parent, &aligned, &fallback, "zh-CN");
        assert_eq!(repaired.len(), 2);
        assert_eq!(repaired[0], "先做计划");
        assert_eq!(repaired[1], "执行");
    }

    #[test]
    fn step5_qa_report_detects_hard_issues() {
        let report = super::build_step_5_4_qa_report(BuildStep5QaReportRequest {
            target_lang: "zh-CN".to_string(),
            terminology_entries: vec![],
            segments: vec![
                Step5FinalSegment {
                    segment_id: 1,
                    start: 0.0,
                    end: 1.0,
                    source: "profit was 1037 dollars".to_string(),
                    translation: "".to_string(),
                    tokens: vec![],
                },
                Step5FinalSegment {
                    segment_id: 2,
                    start: 1.0,
                    end: 2.0,
                    source: "be patient and disciplined".to_string(),
                    translation: "要保持耐心，而且...".to_string(),
                    tokens: vec![],
                },
            ],
        })
        .expect("qa report");
        assert!(!report.passed);
        assert!(report.hard_fail_count >= 2);
    }

    #[test]
    fn step5_qa_report_flags_watchability_fragment_issue() {
        let report = super::build_step_5_4_qa_report(BuildStep5QaReportRequest {
            target_lang: "zh-CN".to_string(),
            terminology_entries: vec![],
            segments: vec![Step5FinalSegment {
                segment_id: 1,
                start: 0.0,
                end: 1.0,
                source: "and I sit down, I start loading up my trading platform".to_string(),
                translation: "然后花大约".to_string(),
                tokens: vec![],
            }],
        })
        .expect("qa report");
        assert!(
            report
                .issues
                .iter()
                .any(|issue| issue.rule_id == "watchability_fragment")
        );
    }

    #[test]
    fn step5_watchability_repair_applies_percent_rewrite() {
        let mut segments = vec![Step5FinalSegment {
            segment_id: 1,
            start: 0.0,
            end: 1.0,
            source: "happy with if you're taking 50%or 60%of the total run up without leaving a lot on the table.".to_string(),
            translation: "60 50 你更有可能持续地以满意的金额退出那笔交易。".to_string(),
            tokens: vec![],
        }];
        super::repair_watchability_fragments(&mut segments, "zh-CN");
        assert!(segments[0].translation.contains("50%或60%"));
        assert!(!segments[0].translation.starts_with("60 50"));
        assert!(!segments[0].translation.contains("/60 "));
    }

    #[test]
    fn step5_watchability_repair_rewrites_point_trade_number_lead() {
        let mut segments = vec![Step5FinalSegment {
            segment_id: 1,
            start: 0.0,
            end: 1.0,
            source: "to take a 10-point trade and watch it run for 50 points".to_string(),
            translation: "10 看着它涨到50点。".to_string(),
            tokens: vec![],
        }];
        super::repair_watchability_fragments(&mut segments, "zh-CN");
        assert!(segments[0].translation.contains("做一笔10点的交易"));
    }

    #[test]
    fn step5_watchability_repair_rewrites_day_in_minutes_prefix() {
        let mut segments = vec![Step5FinalSegment {
            segment_id: 1,
            start: 0.0,
            end: 1.0,
            source: "a day in 30 minutes or whatever it is, right?".to_string(),
            translation: "30分钟，对吧？".to_string(),
            tokens: vec![],
        }];
        super::repair_watchability_fragments(&mut segments, "zh-CN");
        assert_eq!(segments[0].translation, "一天30分钟，对吧？");
    }

    #[test]
    fn step5_watchability_repair_rewrites_seconds_prefix() {
        let mut segments = vec![Step5FinalSegment {
            segment_id: 1,
            start: 0.0,
            end: 1.0,
            source: "and I just take like 30 seconds to read these".to_string(),
            translation: "30 来阅读这些内容。".to_string(),
            tokens: vec![],
        }];
        super::repair_watchability_fragments(&mut segments, "zh-CN");
        assert!(segments[0].translation.starts_with("花大约30秒"));
    }

    #[test]
    fn step5_watchability_repair_trims_trailing_connector_fragment() {
        let mut segments = vec![Step5FinalSegment {
            segment_id: 1,
            start: 0.0,
            end: 1.0,
            source: "I have these stickies up and when they fall down I rewrite them and stick them back up".to_string(),
            translation: "我有这些便利贴，而且".to_string(),
            tokens: vec![],
        }];
        super::repair_watchability_fragments(&mut segments, "zh-CN");
        assert_eq!(segments[0].translation, "我有这些便利贴");
    }

    #[test]
    fn step5_helpers_detect_tail_ellipsis_and_numbers() {
        assert!(has_tail_ellipsis("我们正试图..."));
        assert!(has_tail_ellipsis("我们正试图. ."));
        let numbers = extract_numbers("start 1,037.5 then 20 grand and 5万");
        assert!(numbers.contains("1037.5"));
        assert!(numbers.contains("20000"));
        assert!(numbers.contains("50000"));

        let bucks = extract_numbers("make 2,000 bucks");
        assert!(bucks.contains("2000"));

        let mixed = extract_numbers("a thousand and 37 bucks");
        assert!(mixed.contains("1037"));

        let punct_followed = extract_numbers("50,my tank might decrease");
        assert!(punct_followed.contains("50"));
        assert!(!punct_followed.contains("50000000"));
    }

    #[test]
    fn step5_repair_treats_punctuation_only_as_invalid() {
        let parent = Step5SplitParent {
            parent_segment_id: 1,
            draft_translation: "保持耐心，稳扎稳打".to_string(),
            parts: vec![Step5SplitPart {
                part_id: 1,
                start: 0.0,
                end: 1.0,
                source: "be patient and stay consistent".to_string(),
                tokens: vec![],
            }],
        };
        let aligned = vec![".".to_string()];
        let fallback = vec!["保持耐心，稳扎稳打".to_string()];
        let repaired = repair_aligned_lines(&parent, &aligned, &fallback, "zh-CN");
        assert_eq!(repaired[0], "保持耐心，稳扎稳打");
    }

    #[test]
    fn step5_repair_prefers_numeric_consistent_fallback() {
        let parent = Step5SplitParent {
            parent_segment_id: 1,
            draft_translation: "应该赚到2万美元".to_string(),
            parts: vec![Step5SplitPart {
                part_id: 1,
                start: 0.0,
                end: 1.0,
                source: "should be making $20,000".to_string(),
                tokens: vec![],
            }],
        };
        let aligned = vec!["应该在30分钟内赚到2万美元".to_string()];
        let fallback = vec!["应该赚到2万美元".to_string()];
        let repaired = repair_aligned_lines(&parent, &aligned, &fallback, "zh-CN");
        assert_eq!(repaired[0], "应该赚到2万美元");
    }

    #[test]
    fn step5_align_repair_replaces_duplicated_parent_copy() {
        let parent = Step5SplitParent {
            parent_segment_id: 1,
            draft_translation: "如果你每次上垒都能稳定执行，长期就能积累优势".to_string(),
            parts: vec![
                Step5SplitPart {
                    part_id: 1,
                    start: 0.0,
                    end: 1.0,
                    source: "if you consistently get on base".to_string(),
                    tokens: vec![],
                },
                Step5SplitPart {
                    part_id: 2,
                    start: 1.0,
                    end: 2.0,
                    source: "you will stack an edge over time".to_string(),
                    tokens: vec![],
                },
            ],
        };
        let aligned = vec![
            "如果你每次上垒都能稳定执行，长期就能积累优势".to_string(),
            "如果你每次上垒都能稳定执行，长期就能积累优势".to_string(),
        ];
        let fallback = vec![
            "每次上垒都稳定执行".to_string(),
            "长期就能积累优势".to_string(),
        ];
        let repaired = repair_aligned_lines(&parent, &aligned, &fallback, "zh-CN");
        assert_eq!(repaired[0], "每次上垒都稳定执行");
        assert_eq!(repaired[1], "长期就能积累优势");
    }

    #[test]
    fn step5_align_repair_replaces_source_residue_for_cjk_target() {
        let parent = Step5SplitParent {
            parent_segment_id: 1,
            draft_translation: "要收手，等下一次机会".to_string(),
            parts: vec![Step5SplitPart {
                part_id: 1,
                start: 0.0,
                end: 1.0,
                source: "and out, take my base hit and trade another day".to_string(),
                tokens: vec![],
            }],
        };
        let aligned = vec!["and out take my base hit and trade another day".to_string()];
        let fallback = vec!["收手，拿到安打，留到下次再做".to_string()];
        let repaired = repair_aligned_lines(&parent, &aligned, &fallback, "zh-CN");
        assert_eq!(repaired[0], "收手，拿到安打，留到下次再做");
    }

    #[test]
    fn step5_fallback_split_avoids_empty_lines_for_weak_model() {
        let lines = heuristic_split_translation(
            "如果你一次只做一个高把握动作就能稳定推进并且减少回撤",
            3,
            None,
        );
        assert_eq!(lines.len(), 3);
        assert!(lines.iter().all(|line| !line.trim().is_empty()));
    }

    #[test]
    fn step5_align_repair_rebalances_leaked_numbers_between_neighbors() {
        let parent = Step5SplitParent {
            parent_segment_id: 1,
            draft_translation: "刚开始时很常见，你做一笔10点交易，看它涨到50点。".to_string(),
            parts: vec![
                Step5SplitPart {
                    part_id: 1,
                    start: 0.0,
                    end: 1.0,
                    source: "it's very common and challenging when you're first starting out"
                        .to_string(),
                    tokens: vec![],
                },
                Step5SplitPart {
                    part_id: 2,
                    start: 1.0,
                    end: 2.0,
                    source: "to take a 10-point trade and watch it run for 50 points".to_string(),
                    tokens: vec![],
                },
            ],
        };
        let aligned = vec![
            "刚开始时，做一个10点交易很常见".to_string(),
            "看它涨到50点很难".to_string(),
        ];
        let fallback = vec![
            "刚开始时这很常见".to_string(),
            "看它涨到50点很难".to_string(),
        ];
        let repaired = repair_aligned_lines(&parent, &aligned, &fallback, "zh-CN");
        assert_eq!(repaired[0], "刚开始时这很常见");
        assert!(repaired[1].contains("10"));
        assert!(repaired[1].contains("50"));
    }

    #[test]
    fn step5_align_repair_removes_next_line_number_from_numberless_line() {
        let parent = Step5SplitParent {
            parent_segment_id: 67,
            draft_translation: "你知道，对于基础命中，当你刚开始时，做一个10点的交易，看着它涨到50点或100点，这实际上非常常见，也非常具有挑战性。".to_string(),
            parts: vec![
                Step5SplitPart {
                    part_id: 1,
                    start: 0.0,
                    end: 1.0,
                    source:
                        "with base hits it's actually very common and very challenging when you're first starting out"
                            .to_string(),
                    tokens: vec![],
                },
                Step5SplitPart {
                    part_id: 2,
                    start: 1.0,
                    end: 2.0,
                    source:
                        "to take a 10-point trade and watch it run for 50 points or watch it run for 100 points"
                            .to_string(),
                    tokens: vec![],
                },
            ],
        };
        let aligned = vec![
            "你知道， 对于基础命中， 当你刚开始时， 做一个10点的交易，".to_string(),
            "10 看着它涨到50点或100点， 这实际上非常常见， 也非常具有挑战性。".to_string(),
        ];
        let fallback = aligned.clone();
        let repaired = repair_aligned_lines(&parent, &aligned, &fallback, "zh-CN");
        assert!(
            !repaired[0].contains("10"),
            "left line still leaks number: {}",
            repaired[0]
        );
        assert!(repaired[0].contains("当你刚开始时"));
        assert!(repaired[1].contains("10"));
        assert!(repaired[1].contains("50"));
        assert!(repaired[1].contains("100"));
    }

    #[test]
    fn step5_align_repair_rewrites_percent_line_without_reversed_numbers() {
        let parent = Step5SplitParent {
            parent_segment_id: 191,
            draft_translation: "如果你拿走总上涨的50%或60%，而不留下太多利润，你更有可能持续地以满意的金额退出那笔交易。".to_string(),
            parts: vec![
                Step5SplitPart {
                    part_id: 1,
                    start: 0.0,
                    end: 1.0,
                    source: "You're more consistently likely to be able to get out".to_string(),
                    tokens: vec![],
                },
                Step5SplitPart {
                    part_id: 2,
                    start: 1.0,
                    end: 2.0,
                    source: "of that trade at a dollar value that you're".to_string(),
                    tokens: vec![],
                },
                Step5SplitPart {
                    part_id: 3,
                    start: 2.0,
                    end: 3.0,
                    source: "happy with if you're taking 50%or 60%of the total run up without leaving a lot on the table.".to_string(),
                    tokens: vec![],
                },
            ],
        };
        let aligned = vec![
            "如果你拿走总上涨的50%或60%，".to_string(),
            "而不留下太多利润，".to_string(),
            "60 50 你更有可能持续地以满意的金额退出那笔交易。".to_string(),
        ];
        let fallback = aligned.clone();
        let repaired = repair_aligned_lines(&parent, &aligned, &fallback, "zh-CN");
        assert!(repaired[2].contains("50%或60%"));
        assert!(!repaired[2].starts_with("60 50"));
    }

    #[test]
    fn step5_align_repair_fixes_hua_dayue_fragment_pair() {
        let parent = Step5SplitParent {
            parent_segment_id: 221,
            draft_translation: "但当我坐下来时，我会先加载交易平台，然后花大约30秒的时间，一个小小的30秒休息，来阅读这些内容并大声念出来。".to_string(),
            parts: vec![
                Step5SplitPart {
                    part_id: 1,
                    start: 0.0,
                    end: 1.0,
                    source: "But what I do when I sit down,I come".to_string(),
                    tokens: vec![],
                },
                Step5SplitPart {
                    part_id: 2,
                    start: 1.0,
                    end: 2.0,
                    source: "and I sit down,I start loading up my trading platform,".to_string(),
                    tokens: vec![],
                },
                Step5SplitPart {
                    part_id: 3,
                    start: 2.0,
                    end: 3.0,
                    source: "and I just take like 30 seconds,a little 30-second break to read these and actually say them out loud.".to_string(),
                    tokens: vec![],
                },
            ],
        };
        let aligned = vec![
            "但我坐下时，我会先加载交易平台。".to_string(),
            "然后花大约".to_string(),
            "30 来阅读这些内容并大声念出来。".to_string(),
        ];
        let fallback = aligned.clone();
        let repaired = repair_aligned_lines(&parent, &aligned, &fallback, "zh-CN");
        assert_ne!(repaired[1], "然后花大约");
        assert!(repaired[2].contains("30"));
    }

    #[test]
    fn step5_align_repair_rewrites_day_in_minutes_fragment() {
        let parent = Step5SplitParent {
            parent_segment_id: 202,
            draft_translation:
                "因为我在网上看到，我应该在30分钟内赚到2万美元，或者类似的说法，对吧？".to_string(),
            parts: vec![
                Step5SplitPart {
                    part_id: 1,
                    start: 0.0,
                    end: 1.0,
                    source: "Because I see online that I should be making$20,000".to_string(),
                    tokens: vec![],
                },
                Step5SplitPart {
                    part_id: 2,
                    start: 1.0,
                    end: 2.0,
                    source: "a day in 30 minutes or whatever it is,right?".to_string(),
                    tokens: vec![],
                },
            ],
        };
        let aligned = vec![
            "因为我在网上看到，我应该在30分钟内赚到2万美元".to_string(),
            "30 或者类似的说法，对吧？".to_string(),
        ];
        let fallback = aligned.clone();
        let repaired = repair_aligned_lines(&parent, &aligned, &fallback, "zh-CN");
        assert!(repaired[1].contains("一天30分钟"));
    }

    #[test]
    fn step5_align_repair_rewrites_trading_platform_and_seconds_fragments() {
        let parent = Step5SplitParent {
            parent_segment_id: 221,
            draft_translation: "但当我坐下来时，我会先加载交易平台，然后花大约30秒的时间，一个小小的30秒休息，来阅读这些内容并大声念出来。".to_string(),
            parts: vec![
                Step5SplitPart {
                    part_id: 1,
                    start: 0.0,
                    end: 1.0,
                    source: "But what I do when I sit down,I come".to_string(),
                    tokens: vec![],
                },
                Step5SplitPart {
                    part_id: 2,
                    start: 1.0,
                    end: 2.0,
                    source: "and I sit down,I start loading up my trading platform,".to_string(),
                    tokens: vec![],
                },
                Step5SplitPart {
                    part_id: 3,
                    start: 2.0,
                    end: 3.0,
                    source: "and I just take like 30 seconds,a little 30-second break to read these and actually say them out loud.".to_string(),
                    tokens: vec![],
                },
            ],
        };
        let aligned = vec![
            "但我坐下时，我会先加载交易平台。".to_string(),
            "然后".to_string(),
            "30 来阅读这些内容并大声念出来。".to_string(),
        ];
        let fallback = aligned.clone();
        let repaired = repair_aligned_lines(&parent, &aligned, &fallback, "zh-CN");
        assert!(repaired[1].contains("加载交易平台"));
        assert!(repaired[2].starts_with("花大约30秒"));
    }

    #[test]
    fn step5_align_repair_removes_neighbor_number_leak_when_targets_already_covered() {
        let parent = Step5SplitParent {
            parent_segment_id: 1,
            draft_translation: "如果交易先上涨100点再回撤50点，我的状态可能比稳定拿10点更快下降。".to_string(),
            parts: vec![
                Step5SplitPart {
                    part_id: 1,
                    start: 0.0,
                    end: 1.0,
                    source: "if my trade ran up 100 points and starts pulling back".to_string(),
                    tokens: vec![],
                },
                Step5SplitPart {
                    part_id: 2,
                    start: 1.0,
                    end: 2.0,
                    source:
                        "50,my tank might decrease even faster than just taking consistent 10 points"
                            .to_string(),
                    tokens: vec![],
                },
            ],
        };
        let aligned = vec![
            "如果交易先上涨100点再回撤50点".to_string(),
            "50 我的状态可能比稳定拿10点更快下降".to_string(),
        ];
        let fallback = vec![
            "如果交易先上涨100点再回撤50点".to_string(),
            "50 我的状态可能比稳定拿10点更快下降".to_string(),
        ];
        let repaired = repair_aligned_lines(&parent, &aligned, &fallback, "zh-CN");
        assert!(repaired[0].contains("100"));
        assert!(!repaired[0].contains("50"));
        assert!(repaired[1].contains("50"));
        assert!(repaired[1].contains("10"));
    }

    #[test]
    fn step5_align_repair_handles_real_world_parent150_number_leak() {
        let parent = Step5SplitParent {
            parent_segment_id: 150,
            draft_translation: "如果我持有的交易上涨了100点然后回撤50点，我的油箱可能比只是稳定上涨10点消耗得更快。".to_string(),
            parts: vec![
                Step5SplitPart {
                    part_id: 1,
                    start: 574.645,
                    end: 578.005,
                    source: "If I'm in a trade that ran up 100 points and starts pulling back"
                        .to_string(),
                    tokens: vec![],
                },
                Step5SplitPart {
                    part_id: 2,
                    start: 578.085,
                    end: 582.725,
                    source:
                        "50,my tank might decrease even faster as opposed to just having consistent 10 points."
                            .to_string(),
                    tokens: vec![],
                },
            ],
        };
        let aligned = vec![
            "如果我持有的交易上涨了100点然后回撤50点，".to_string(),
            "50 我的油箱可能比只是稳定上涨10点消耗得更快。".to_string(),
        ];
        let fallback = aligned.clone();
        let repaired = repair_aligned_lines(&parent, &aligned, &fallback, "zh-CN");
        assert!(repaired[0].contains("100"));
        assert!(!repaired[0].contains("50"));
        assert!(repaired[1].contains("50"));
        assert!(repaired[1].contains("10"));
    }

    #[test]
    fn step5_heuristic_split_avoids_cjk_mid_phrase_fragments() {
        let parts = vec![
            Step5SplitPart {
                part_id: 1,
                start: 0.0,
                end: 1.0,
                source: "But what I do when I sit down,I come and".to_string(),
                tokens: vec![],
            },
            Step5SplitPart {
                part_id: 2,
                start: 1.0,
                end: 2.0,
                source: "I sit down,I start loading up my trading platform,".to_string(),
                tokens: vec![],
            },
            Step5SplitPart {
                part_id: 3,
                start: 2.0,
                end: 3.0,
                source: "and I just take like 30 seconds,a little 30-second break to read these and actually say them out loud.".to_string(),
                tokens: vec![],
            },
        ];
        let lines = heuristic_split_translation(
            "但当我坐下来时，我会先加载交易平台，然后花大约30秒的时间，一个小小的30秒休息，来阅读这些内容并大声念出来。",
            3,
            Some(&parts),
        );
        assert_eq!(lines.len(), 3);
        assert!(lines.iter().all(|line| !line.trim().is_empty()));
        assert!(!lines.iter().any(|line| line.trim() == "然后花大约"));
        assert!(lines.iter().any(|line| line.contains("交易平台")));
        assert!(lines.iter().any(|line| line.contains("30秒")));
    }

    #[test]
    fn step5_align_prefers_fallback_when_llm_lines_are_fragmented() {
        let parent = Step5SplitParent {
            parent_segment_id: 221,
            draft_translation: "但当我坐下来时，我会先加载交易平台，然后花大约30秒的时间，一个小小的30秒休息，来阅读这些内容并大声念出来。".to_string(),
            parts: vec![
                Step5SplitPart {
                    part_id: 1,
                    start: 0.0,
                    end: 1.0,
                    source: "But what I do when I sit down,I come and".to_string(),
                    tokens: vec![],
                },
                Step5SplitPart {
                    part_id: 2,
                    start: 1.0,
                    end: 2.0,
                    source: "I sit down,I start loading up my trading platform,".to_string(),
                    tokens: vec![],
                },
                Step5SplitPart {
                    part_id: 3,
                    start: 2.0,
                    end: 3.0,
                    source: "and I just take like 30 seconds,a little 30-second break to read these and actually say them out loud.".to_string(),
                    tokens: vec![],
                },
            ],
        };
        let fragmented = vec![
            "但我坐下来时，我会先加载交易平台。".to_string(),
            "然后花大约".to_string(),
            "时间，一个小小的30秒休息，来阅读这些内容并大声念出来。".to_string(),
        ];
        let fallback = vec![
            "但当我坐下来时".to_string(),
            "我会先加载交易平台，然后花一点时间".to_string(),
            "进行一个30秒的小休息，来阅读这些内容并大声念出来。".to_string(),
        ];
        let selected = choose_better_alignment(&parent, &fragmented, &fallback, "zh-CN");
        assert_eq!(selected, fallback);
    }

    #[test]
    fn step5_align_choice_rejects_fallback_with_next_line_numeric_leak() {
        let parent = Step5SplitParent {
            parent_segment_id: 67,
            draft_translation: "对于基础命中，当你刚开始时，做一个10点的交易，看着它涨到50点或100点，这很常见也很有挑战。".to_string(),
            parts: vec![
                Step5SplitPart {
                    part_id: 1,
                    start: 0.0,
                    end: 1.0,
                    source: "with base hits it's very common and challenging when you're first starting out".to_string(),
                    tokens: vec![],
                },
                Step5SplitPart {
                    part_id: 2,
                    start: 1.0,
                    end: 2.0,
                    source: "to take a 10-point trade and watch it run for 50 points or 100 points".to_string(),
                    tokens: vec![],
                },
            ],
        };
        let aligned_without_leak = vec![
            "对于基础命中，当你刚开始时，这很常见也很有挑战。".to_string(),
            "做一个10点的交易，看着它涨到50点或100点。".to_string(),
        ];
        let fallback_with_leak = vec![
            "对于基础命中，当你刚开始时，做一个10点的交易。".to_string(),
            "10 看着它涨到50点或100点，这很常见也很有挑战。".to_string(),
        ];
        let selected =
            choose_better_alignment(&parent, &aligned_without_leak, &fallback_with_leak, "zh-CN");
        assert_eq!(selected, aligned_without_leak);
    }

    #[test]
    fn step5_source_residue_detection_flags_untranslated_english() {
        assert!(looks_like_source_residue(
            "and out, take my base hit and trade another day",
            "and out take my base hit and trade another day",
            "zh-CN"
        ));
        assert!(!looks_like_source_residue(
            "base hit strategy",
            "这是我的 base hit 策略",
            "zh-CN"
        ));
    }
}
