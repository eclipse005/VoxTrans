use std::collections::HashMap;
use std::sync::Arc;

use crate::services::llm::batch::run_indexed_concurrent_with_progress;
use crate::services::llm::client::OpenAiCompatLlmClient;
use crate::services::llm::port::{LlmCallContext, LlmConfig, LlmJsonTask, next_llm_request_id};
use crate::services::prompts::subtitle_step5::build_source_split_prompt;

use super::constants::OBVIOUS_OVERLONG_RATIO;
use super::language_units::{text_length_units, use_char_units};
use super::responses::validate_source_split_response;
use super::source_split::hard_pause_boundaries;
use super::source_split_boundaries::{map_source_parts_to_boundaries, normalize_split_boundaries};
use super::source_split_readability::merge_tiny_ranges_for_readability;
use super::source_text::build_source_from_tokens;
use super::split_parts::{
    boundary_ids_to_ranges, build_single_split_part, build_split_parts_from_ranges,
};
use super::stage_models::{Step51LlmSplitTask, Step51SplitWorkItem};
use super::text_utils::ends_with_sentence_punctuation;
use super::text_utils::normalize_inline_text;
use super::types::{BuildStep5SourceSplitRequest, BuildStep5SourceSplitResponse, Step5SplitParent};

const MAX_LLM_SPLIT_ROUNDS: usize = 1;

pub async fn build_step_5_1_source_split_with_progress(
    request: BuildStep5SourceSplitRequest,
    on_progress: Option<Arc<dyn Fn(usize, usize) + Send + Sync>>,
) -> Result<BuildStep5SourceSplitResponse, String> {
    if request.segments.is_empty() {
        return Err("segments is required".to_string());
    }

    let subtitle_length_preset = crate::services::subtitle_length::normalize_subtitle_length_preset(
        &request.subtitle_length_preset,
    );
    let effective_limits = crate::services::subtitle_length::effective_subtitle_limits_from_preset(
        &request.source_lang,
        &request.target_lang,
        &subtitle_length_preset,
    );
    let source_limit = effective_limits.source_limit as f64;
    let target_limit = effective_limits.target_limit as f64;
    let mut work_items = Vec::<Step51SplitWorkItem>::new();
    let mut ranges_by_work_index = Vec::<Vec<(usize, usize)>>::new();

    for segment in request.segments.clone() {
        let draft_translation = normalize_inline_text(&segment.draft_translation);
        let mandatory_boundaries = hard_pause_boundaries(&segment.tokens);
        let initial_ranges = if segment.tokens.is_empty() {
            Vec::new()
        } else {
            boundary_ids_to_ranges(&mandatory_boundaries, segment.tokens.len())
        };
        ranges_by_work_index.push(initial_ranges);
        work_items.push(Step51SplitWorkItem {
            segment,
            draft_translation,
            mandatory_boundaries,
        });
    }

    let mut llm_client = None::<OpenAiCompatLlmClient>;
    for round in 1..=MAX_LLM_SPLIT_ROUNDS {
        let tasks = build_round_tasks(
            round,
            &work_items,
            &ranges_by_work_index,
            &request.source_lang,
            &request.target_lang,
            source_limit,
            target_limit,
        );
        if tasks.is_empty() {
            break;
        }
        if llm_client.is_none() {
            if request.translate_api_key.trim().is_empty() {
                return Err("translateApiKey is required".to_string());
            }
            if request.translate_base_url.trim().is_empty() {
                return Err("translateBaseUrl is required".to_string());
            }
            if request.translate_model.trim().is_empty() {
                return Err("translateModel is required".to_string());
            }
            llm_client = Some(
                OpenAiCompatLlmClient::new(LlmConfig::new(
                    request.translate_base_url.clone(),
                    request.translate_api_key.clone(),
                    request.translate_model.clone(),
                ))
                .map_err(|err| err.message)?,
            );
        }
        if let Some(client) = llm_client.as_ref() {
            let split_boundaries =
                run_llm_split_round(&request, client, tasks, source_limit, target_limit).await;
            apply_round_boundaries(&mut ranges_by_work_index, split_boundaries);
        }
    }

    let total = work_items.len().max(1);
    if let Some(callback) = on_progress.as_ref() {
        callback(0, total);
    }
    let mut parents = Vec::<Step5SplitParent>::new();
    let mut part_total = 0usize;
    for (work_index, work) in work_items.into_iter().enumerate() {
        let mut ranges = ranges_by_work_index
            .get(work_index)
            .cloned()
            .unwrap_or_default();
        ranges = merge_tiny_ranges_for_readability(
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
        subtitle_length_preset,
        parent_total: parents.len(),
        part_total,
        parents,
    })
}

fn build_round_tasks(
    round: usize,
    work_items: &[Step51SplitWorkItem],
    ranges_by_work_index: &[Vec<(usize, usize)>],
    source_lang: &str,
    target_lang: &str,
    source_limit: f64,
    target_limit: f64,
) -> Vec<Step51LlmSplitTask> {
    let mut tasks = Vec::<Step51LlmSplitTask>::new();
    for (work_index, work) in work_items.iter().enumerate() {
        if work.segment.tokens.len() <= 1 {
            continue;
        }
        let full_source_text = build_source_from_tokens(&work.segment.tokens);
        let full_source_units = text_length_units(&full_source_text, source_lang).max(1.0);
        let target_units = text_length_units(&work.draft_translation, target_lang);
        let ranges = ranges_by_work_index
            .get(work_index)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        for range in ranges {
            let (start, end) = *range;
            if end <= start || end >= work.segment.tokens.len() {
                continue;
            }
            let source_text = build_source_from_tokens(&work.segment.tokens[start..=end]);
            let source_units = text_length_units(&source_text, source_lang);
            let target_estimate = target_units * (source_units / full_source_units.max(1.0));
            if !needs_binary_split(source_units, target_estimate, source_limit, target_limit) {
                continue;
            }
            let require_split = source_units > source_limit * OBVIOUS_OVERLONG_RATIO
                || target_estimate > target_limit * OBVIOUS_OVERLONG_RATIO;
            tasks.push(Step51LlmSplitTask {
                task_id: tasks.len(),
                work_index,
                source_lang: source_lang.to_string(),
                tokens: work.segment.tokens.clone(),
                range: *range,
                source_text: source_text.clone(),
                require_split,
                prompt: build_source_split_prompt(
                    source_lang,
                    target_lang,
                    &full_source_text,
                    &work.draft_translation,
                    &source_text,
                    source_limit,
                    target_limit,
                    round,
                    require_split,
                ),
            });
        }
    }
    tasks
}

fn needs_binary_split(
    source_units: f64,
    target_estimate: f64,
    source_limit: f64,
    target_limit: f64,
) -> bool {
    source_units > source_limit || target_estimate > target_limit
}

async fn run_llm_split_round(
    request: &BuildStep5SourceSplitRequest,
    llm_client: &OpenAiCompatLlmClient,
    llm_tasks: Vec<Step51LlmSplitTask>,
    _source_limit: f64,
    _target_limit: f64,
) -> Vec<(usize, (usize, usize), Vec<usize>)> {
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
                                    &split_task.source_text,
                                    split_task.require_split,
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
                    let (start, end) = split_task.range;
                    let relative_boundaries = map_source_parts_to_boundaries(
                        &call.value,
                        &split_task.tokens[start..=end],
                        &split_task.source_lang,
                    );
                    let boundaries = relative_boundaries
                        .into_iter()
                        .map(|boundary| start + boundary)
                        .collect::<Vec<_>>();
                    let boundaries =
                        prefer_punctuation_fallback_for_cjk_boundaries(split_task, boundaries);
                    let boundaries = normalize_split_boundaries(
                        &boundaries,
                        split_task.tokens.len(),
                        &[],
                        &[],
                        1,
                    );
                    Ok((split_task.work_index, split_task.range, boundaries))
                }
            }
        },
        |msg| msg,
        |_done, _total| {},
    )
    .await;

    let mut out = Vec::<(usize, (usize, usize), Vec<usize>)>::new();
    let mut successful_task_ids = std::collections::HashSet::<usize>::new();
    for (_, result) in results {
        let Ok((work_index, range, boundaries)) = result else {
            continue;
        };
        if !boundaries.is_empty() {
            if let Some(task) = llm_tasks
                .iter()
                .find(|task| task.work_index == work_index && task.range == range)
            {
                successful_task_ids.insert(task.task_id);
            }
            out.push((work_index, range, boundaries));
        }
    }
    for task in llm_tasks {
        if successful_task_ids.contains(&task.task_id) || !task.require_split {
            continue;
        }
        if let Some(boundary) = fallback_binary_boundary(&task) {
            out.push((task.work_index, task.range, vec![boundary]));
        }
    }
    out
}

fn fallback_binary_boundary(task: &Step51LlmSplitTask) -> Option<usize> {
    let (start, end) = task.range;
    if end <= start || end >= task.tokens.len() {
        return None;
    }
    let slice = &task.tokens[start..=end];
    let total_units = slice
        .iter()
        .map(|token| text_length_units(&token.text, &task.source_lang))
        .sum::<f64>()
        .max(1.0);
    let midpoint = total_units / 2.0;
    let mut left_units = 0.0f64;
    let mut candidates = Vec::<(usize, f64, bool)>::new();
    for local_idx in 0..slice.len().saturating_sub(1) {
        left_units += text_length_units(&slice[local_idx].text, &task.source_lang);
        let right_units = (total_units - left_units).max(0.0);
        if left_units <= 0.0 || right_units <= 0.0 {
            continue;
        }
        let mut score = (left_units - midpoint).abs();
        if is_good_fallback_split_tail(&slice[local_idx].text) {
            score -= total_units * 0.25;
        }
        if left_units < 3.0 || right_units < 3.0 {
            score += total_units;
        }
        candidates.push((
            start + local_idx + 1,
            score,
            is_good_fallback_split_tail(&slice[local_idx].text),
        ));
    }
    let has_good_candidate = candidates.iter().any(|(_, _, is_good)| *is_good);
    if !has_good_candidate && use_char_units(&task.source_lang, &task.source_text) {
        return None;
    }
    candidates
        .into_iter()
        .filter(|(_, _, is_good)| !has_good_candidate || *is_good)
        .min_by(|(_, left_score, _), (_, right_score, _)| {
            left_score
                .partial_cmp(right_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(boundary, _score, _is_good)| boundary)
}

fn prefer_punctuation_fallback_for_cjk_boundaries(
    task: &Step51LlmSplitTask,
    boundaries: Vec<usize>,
) -> Vec<usize> {
    if boundaries.is_empty() || !use_char_units(&task.source_lang, &task.source_text) {
        return boundaries;
    }
    let has_unsafe_boundary = boundaries
        .iter()
        .copied()
        .any(|boundary| is_unsafe_cjk_boundary(task, boundary));
    if !has_unsafe_boundary {
        return boundaries;
    }
    let Some(boundary) = fallback_binary_boundary(task) else {
        return boundaries;
    };
    let Some(left) = task.tokens.get(boundary.saturating_sub(1)) else {
        return boundaries;
    };
    if is_good_fallback_split_tail(&left.text) {
        vec![boundary]
    } else {
        boundaries
    }
}

fn is_unsafe_cjk_boundary(task: &Step51LlmSplitTask, boundary: usize) -> bool {
    let (start, end) = task.range;
    if boundary <= start || boundary > end {
        return false;
    }
    let Some(left) = task.tokens.get(boundary.saturating_sub(1)) else {
        return false;
    };
    let Some(right) = task.tokens.get(boundary) else {
        return false;
    };
    if is_good_fallback_split_tail(&left.text) {
        return false;
    }
    ends_with_opening_punctuation(&left.text)
        || (ends_with_cjk(&left.text) && starts_with_cjk(&right.text))
}

fn ends_with_cjk(text: &str) -> bool {
    text.trim_end()
        .chars()
        .last()
        .map(is_cjk_char)
        .unwrap_or(false)
}

fn starts_with_cjk(text: &str) -> bool {
    text.trim_start()
        .chars()
        .next()
        .map(is_cjk_char)
        .unwrap_or(false)
}

fn is_cjk_char(ch: char) -> bool {
    ('\u{3400}'..='\u{9fff}').contains(&ch)
}

fn ends_with_opening_punctuation(text: &str) -> bool {
    text.trim_end()
        .chars()
        .last()
        .map(|ch| {
            matches!(
                ch,
                '《' | '“' | '‘' | '(' | '[' | '{' | '（' | '【' | '「' | '『'
            )
        })
        .unwrap_or(false)
}

fn is_good_fallback_split_tail(text: &str) -> bool {
    ends_with_sentence_punctuation(text)
        || text
            .trim_end()
            .chars()
            .last()
            .map(|ch| matches!(ch, '，' | '、' | '；' | '：' | ',' | ';' | ':'))
            .unwrap_or(false)
}

fn apply_round_boundaries(
    ranges_by_work_index: &mut [Vec<(usize, usize)>],
    split_boundaries: Vec<(usize, (usize, usize), Vec<usize>)>,
) {
    if split_boundaries.is_empty() {
        return;
    }
    let mut by_work_range = HashMap::<(usize, (usize, usize)), Vec<usize>>::new();
    for (work_index, range, boundaries) in split_boundaries {
        by_work_range.insert((work_index, range), boundaries);
    }
    for (work_index, ranges) in ranges_by_work_index.iter_mut().enumerate() {
        let mut next = Vec::<(usize, usize)>::new();
        for range in ranges.iter().copied() {
            let Some(boundaries) = by_work_range.get(&(work_index, range)).cloned() else {
                next.push(range);
                continue;
            };
            let local = boundaries
                .into_iter()
                .filter(|boundary| *boundary > range.0 && *boundary <= range.1)
                .map(|boundary| boundary - range.0)
                .collect::<Vec<_>>();
            let mut split = boundary_ids_to_ranges(&local, range.1 - range.0 + 1)
                .into_iter()
                .map(|(start, end)| (range.0 + start, range.0 + end))
                .collect::<Vec<_>>();
            if split.is_empty() {
                next.push(range);
            } else {
                next.append(&mut split);
            }
        }
        *ranges = next;
    }
}
