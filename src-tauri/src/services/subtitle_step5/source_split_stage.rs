use std::collections::HashMap;
use std::sync::Arc;

use crate::services::llm::batch::run_indexed_concurrent_with_progress;
use crate::services::llm::client::OpenAiCompatLlmClient;
use crate::services::llm::port::{LlmCallContext, LlmConfig, LlmJsonTask, next_llm_request_id};
use crate::services::prompts::subtitle_step5::build_source_split_prompt;

use super::language_units::text_length_units;
use super::responses::validate_source_split_response;
use super::source_split::{desired_split_parts, hard_pause_boundaries, split_token_ranges};
use super::source_split_boundaries::{map_source_parts_to_boundaries, normalize_split_boundaries};
use super::source_split_readability::finalize_readable_source_ranges;
use super::source_split_score::choose_preferred_split_ranges;
use super::source_text::build_source_from_tokens;
use super::split_parts::{
    boundary_ids_to_ranges, build_single_split_part, build_split_parts_from_ranges,
    ranges_to_boundary_ids,
};
use super::stage_models::{Step51LlmSplitTask, Step51SplitWorkItem};
use super::text_utils::normalize_inline_text;
use super::types::{BuildStep5SourceSplitRequest, BuildStep5SourceSplitResponse, Step5SplitParent};
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
        let ranges = finalize_readable_source_ranges(
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
