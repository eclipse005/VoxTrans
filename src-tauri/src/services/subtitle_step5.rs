use std::collections::HashMap;
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

    let source_limit = request
        .subtitle_max_words_per_segment
        .clamp(8, 40) as f64;
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
                                |value| parse_source_split_parts(value, split_task.min_parts.max(2)),
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
        let fallback = heuristic_split_translation(&parent.draft_translation, count);
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
        let fallback =
            heuristic_split_translation(&parent.draft_translation, expected_count.max(1));
        let aligned = aligned_by_parent
            .get(&parent.parent_segment_id)
            .cloned()
            .unwrap_or(fallback);
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
            if text_length_units(&polished, &request.target_lang)
                <= text_length_units(&segment.translation, &request.target_lang) * 1.02
            {
                segment.translation = polished;
            }
        }
    } else if let Some(callback) = on_progress.as_ref() {
        callback(1, 1);
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
    let mandatory_set = mandatory_sorted.iter().copied().collect::<std::collections::HashSet<_>>();

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

            if let (Some(left), Some(right)) = (
                tokens.get(boundary.saturating_sub(1)),
                tokens.get(boundary),
            ) {
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

fn heuristic_split_translation(text: &str, expected_count: usize) -> Vec<String> {
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
    let clauses_total = clauses.len().max(1);

    let mut out = vec![String::new(); expected_count];
    for (index, clause) in clauses.into_iter().enumerate() {
        let bucket = index * expected_count / clauses_total;
        let target = bucket.min(expected_count - 1);
        if out[target].is_empty() {
            out[target] = clause;
        } else {
            out[target].push(' ');
            out[target].push_str(&clause);
        }
    }
    out.into_iter()
        .map(|line| normalize_inline_text(&line))
        .collect()
}

fn split_clauses(text: &str) -> Vec<String> {
    let mut out = Vec::<String>::new();
    let mut current = String::new();
    for ch in text.chars() {
        current.push(ch);
        if matches!(ch, '.' | '!' | '?' | ';' | '。' | '！' | '？' | '；') {
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
    let source_lower = source.to_lowercase();
    let mut picked = entries
        .iter()
        .filter(|entry| {
            let key = entry.source.trim().to_lowercase();
            !key.is_empty() && source_lower.contains(&key)
        })
        .take(max_terms)
        .cloned()
        .collect::<Vec<_>>();
    if picked.len() >= max_terms {
        return picked;
    }
    for entry in entries {
        if picked.len() >= max_terms {
            break;
        }
        if picked
            .iter()
            .any(|existing| existing.source.eq_ignore_ascii_case(&entry.source))
        {
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
    use super::{
        BuildStep5SourceSplitRequest, Step5DraftSegment, Step5Token,
        build_step_5_1_source_split_with_progress, merge_tiny_ranges_for_readability,
        split_token_ranges,
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
}
