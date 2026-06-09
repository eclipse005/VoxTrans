use std::collections::HashMap;
use std::sync::Arc;

use crate::services::llm::batch::run_indexed_concurrent_idempotent;
use crate::services::llm::client::OpenAiCompatLlmClient;
use crate::services::llm::port::{LlmCallContext, LlmConfig, LlmJsonTask, next_llm_request_id};
use crate::services::prompts::subtitle_step5::{
    Step5PromptLine, Step5PromptTerm, build_align_prompt, build_source_split_prompt,
};

use super::alignment_repair::repair_aligned_lines;
use super::alignment_score::choose_better_alignment;
use super::constants::{MAX_TERMS_PER_LINE, OBVIOUS_OVERLONG_RATIO};
use super::language_units::text_length_units;
use super::request_validation::validate_step5_align_request;
use super::responses::{validate_align_response, validate_source_split_response};
use super::source_split_boundaries::{map_source_parts_to_boundaries, normalize_split_boundaries};
use super::source_text::build_source_from_tokens;
use super::split_parts::boundary_ids_to_ranges;
use super::stage_models::Step5SplitTask;
use super::terminology_filter::select_terms_for_text;
use super::text_utils::normalize_inline_text;
use super::translation_split::heuristic_split_translation;
use super::types::{
    BuildStep5TranslationAlignRequest, BuildStep5TranslationAlignResponse, Step5AlignedParent,
    Step5AlignedPart, Step5SplitParent, Step5SplitPart,
};
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
    ?;

    let first_pass_total = request.parents.len().max(1);
    let second_pass_budget = estimate_second_pass_budget(&request.parents);
    let progress_total = first_pass_total + second_pass_budget;
    let first_pass_progress = on_progress.as_ref().map(|callback| {
        let callback = Arc::clone(callback);
        Arc::new(move |current: usize, _total: usize| {
            callback(current.min(first_pass_total), progress_total);
        }) as Arc<dyn Fn(usize, usize) + Send + Sync>
    });

    let first_pass =
        align_once(&request, &llm_client, &request.parents, first_pass_progress).await?;
    let second_pass = build_second_pass_aligned_response(
        &request,
        &llm_client,
        &first_pass,
        on_progress.clone(),
        first_pass_total,
        progress_total,
    )
    .await?;
    if let Some(second_pass) = second_pass {
        return Ok(second_pass);
    }

    Ok(first_pass)
}

async fn build_second_pass_aligned_response(
    request: &BuildStep5TranslationAlignRequest,
    llm_client: &OpenAiCompatLlmClient,
    first_pass: &BuildStep5TranslationAlignResponse,
    on_progress: Option<Arc<dyn Fn(usize, usize) + Send + Sync>>,
    first_pass_total: usize,
    progress_total: usize,
) -> Result<Option<BuildStep5TranslationAlignResponse>, String> {
    let effective_limits = crate::services::subtitle_length::effective_subtitle_limits_from_preset(
        &request.source_lang,
        &request.target_lang,
        &request.subtitle_length_preset,
    );
    let source_limit = effective_limits.source_limit as f64;
    let target_limit = effective_limits.target_limit as f64;
    let mut changed = false;
    let mut refined = Vec::<Step5AlignedParent>::new();
    let mut second_pass_done = 0usize;

    for aligned_parent in &first_pass.parents {
        let mut parts = Vec::<Step5AlignedPart>::new();
        for aligned_part in &aligned_parent.parts {
            let second_pass_candidate = aligned_part.tokens.len() > 1;
            let source_units = text_length_units(&aligned_part.source, &request.source_lang);
            let target_units = text_length_units(&aligned_part.translation, &request.target_lang);
            let needs_split = second_pass_candidate
                && (source_units > source_limit || target_units > target_limit);
            if !needs_split {
                parts.push(Step5AlignedPart {
                    part_id: parts.len() + 1,
                    start: aligned_part.start,
                    end: aligned_part.end,
                    source: aligned_part.source.clone(),
                    translation: aligned_part.translation.clone(),
                    tokens: aligned_part.tokens.clone(),
                });
                if second_pass_candidate {
                    second_pass_done += 1;
                    report_second_pass_progress(
                        &on_progress,
                        first_pass_total + second_pass_done,
                        progress_total,
                    );
                }
                continue;
            }
            let must_split = source_units > source_limit * OBVIOUS_OVERLONG_RATIO
                || target_units > target_limit * OBVIOUS_OVERLONG_RATIO;

            let split_parts = split_aligned_part_for_second_pass(
                request,
                llm_client,
                aligned_part,
                source_limit,
                target_limit,
                must_split,
            )
            .await?;
            if split_parts.len() <= 1 {
                parts.push(Step5AlignedPart {
                    part_id: parts.len() + 1,
                    start: aligned_part.start,
                    end: aligned_part.end,
                    source: aligned_part.source.clone(),
                    translation: aligned_part.translation.clone(),
                    tokens: aligned_part.tokens.clone(),
                });
                second_pass_done += 1;
                report_second_pass_progress(
                    &on_progress,
                    first_pass_total + second_pass_done,
                    progress_total,
                );
                continue;
            }

            let local_parent = Step5SplitParent {
                parent_segment_id: aligned_parent.parent_segment_id,
                draft_translation: aligned_part.translation.clone(),
                parts: split_parts,
            };
            let local_aligned = align_once(request, llm_client, &[local_parent], None).await?;
            let Some(local_parent) = local_aligned.parents.into_iter().next() else {
                parts.push(Step5AlignedPart {
                    part_id: parts.len() + 1,
                    start: aligned_part.start,
                    end: aligned_part.end,
                    source: aligned_part.source.clone(),
                    translation: aligned_part.translation.clone(),
                    tokens: aligned_part.tokens.clone(),
                });
                second_pass_done += 1;
                report_second_pass_progress(
                    &on_progress,
                    first_pass_total + second_pass_done,
                    progress_total,
                );
                continue;
            };
            changed = true;
            for mut part in local_parent.parts {
                part.part_id = parts.len() + 1;
                parts.push(part);
            }
            second_pass_done += 1;
            report_second_pass_progress(
                &on_progress,
                first_pass_total + second_pass_done,
                progress_total,
            );
        }
        refined.push(Step5AlignedParent {
            parent_segment_id: aligned_parent.parent_segment_id,
            parts,
        });
    }

    let part_total = refined
        .iter()
        .map(|parent| parent.parts.len())
        .sum::<usize>();
    Ok(if changed {
        Some(BuildStep5TranslationAlignResponse {
            parent_total: refined.len(),
            part_total,
            parents: refined,
        })
    } else {
        None
    })
}

fn estimate_second_pass_budget(parents: &[Step5SplitParent]) -> usize {
    parents
        .iter()
        .map(|parent| {
            parent
                .parts
                .iter()
                .filter(|part| part.tokens.len() > 1)
                .count()
        })
        .sum::<usize>()
}

fn report_second_pass_progress(
    on_progress: &Option<Arc<dyn Fn(usize, usize) + Send + Sync>>,
    current: usize,
    total: usize,
) {
    if let Some(callback) = on_progress.as_ref() {
        callback(current.min(total), total.max(1));
    }
}

async fn split_aligned_part_for_second_pass(
    request: &BuildStep5TranslationAlignRequest,
    llm_client: &OpenAiCompatLlmClient,
    part: &Step5AlignedPart,
    source_limit: f64,
    target_limit: f64,
    must_split: bool,
) -> Result<Vec<Step5SplitPart>, String> {
    let prompt = build_source_split_prompt(
        &request.source_lang,
        &request.target_lang,
        &part.source,
        &part.translation,
        &part.source,
        source_limit,
        target_limit,
        2,
        must_split,
    );
    let context = LlmCallContext {
        task_id: request.task_id.clone(),
        media_path: Some(request.media_path.clone()),
        phase: "step_5_2_second_pass_source_split".to_string(),
        store: request.unit_store.as_ref().map(|us| us.store().clone()),
    };
    let llm_id = next_llm_request_id();
    let call = llm_client
        .call_json_validated(&context, &llm_id, &prompt, None, |value| {
            validate_source_split_response(value, &part.source, must_split)
        })
        .await;
    let Ok(call) = call else {
        return Ok(vec![aligned_part_to_split_part(part, 1)]);
    };
    let boundaries =
        map_source_parts_to_boundaries(&call.value, &part.tokens, &request.source_lang);
    let boundaries = normalize_split_boundaries(&boundaries, part.tokens.len(), &[], &[], 1);
    let ranges = boundary_ids_to_ranges(&boundaries, part.tokens.len());
    if ranges.len() <= 1 {
        return Ok(vec![aligned_part_to_split_part(part, 1)]);
    }
    Ok(ranges
        .into_iter()
        .enumerate()
        .map(|(index, (start_idx, end_idx))| {
            let tokens = part.tokens[start_idx..=end_idx].to_vec();
            let start = tokens
                .first()
                .map(|token| token.start)
                .unwrap_or(part.start);
            let end = tokens.last().map(|token| token.end).unwrap_or(part.end);
            Step5SplitPart {
                part_id: index + 1,
                start,
                end,
                source: build_source_from_tokens(&tokens),
                tokens,
            }
        })
        .collect())
}

fn aligned_part_to_split_part(part: &Step5AlignedPart, part_id: usize) -> Step5SplitPart {
    Step5SplitPart {
        part_id,
        start: part.start,
        end: part.end,
        source: part.source.clone(),
        tokens: part.tokens.clone(),
    }
}

async fn align_once(
    request: &BuildStep5TranslationAlignRequest,
    llm_client: &OpenAiCompatLlmClient,
    parents: &[Step5SplitParent],
    on_progress: Option<Arc<dyn Fn(usize, usize) + Send + Sync>>,
) -> Result<BuildStep5TranslationAlignResponse, String> {
    let mut aligned_by_parent = HashMap::<usize, Vec<String>>::new();
    let mut split_tasks = Vec::<Step5SplitTask>::new();

    for parent in parents {
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
            store: request.unit_store.as_ref().map(|us| us.store().clone()),
        };
        let split_tasks_for_worker = split_tasks.clone();

        // Build precomputed map from domain table.
        let (precomputed, persist_store) = if let Some(ref us) = request.unit_store {
            let rows = us.load_translation_aligns().await.unwrap_or_default();
            let mut map = HashMap::<usize, Vec<String>>::new();
            for row in rows {
                map.insert(row.parent_index, row.aligned_lines);
            }
            (map, Some(us.clone()))
        } else {
            (HashMap::new(), None)
        };

        let results = run_indexed_concurrent_idempotent(
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
            precomputed,
            move |idx: usize, val: Vec<String>| {
                let store = persist_store.clone();
                async move {
                    if let Some(ref us) = store {
                        us.save_translation_align(
                            &crate::services::pipeline::TranslationAlignRow {
                                parent_index: idx,
                                aligned_lines: val,
                            },
                        )
                        .await?;
                    }
                    Ok(())
                }
            },
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

    let total = parents.len().max(1);
    if let Some(callback) = on_progress.as_ref() {
        callback(0, total);
    }
    let mut output_parents = Vec::<Step5AlignedParent>::new();
    let mut part_total = 0usize;
    for parent in parents {
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
