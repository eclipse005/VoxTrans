use std::collections::HashMap;
use std::sync::Arc;

use crate::services::llm::batch::run_indexed_concurrent_with_progress;
use crate::services::llm::client::OpenAiCompatLlmClient;
use crate::services::llm::port::{LlmCallContext, LlmConfig, LlmJsonTask, next_llm_request_id};
use crate::services::prompts::subtitle_step5::{
    Step5PromptLine, Step5PromptTerm, build_align_prompt,
};

use super::alignment_repair::repair_aligned_lines;
use super::alignment_score::choose_better_alignment;
use super::constants::MAX_TERMS_PER_LINE;
use super::request_validation::validate_step5_align_request;
use super::responses::validate_align_response;
use super::stage_models::Step5SplitTask;
use super::terminology_filter::select_terms_for_text;
use super::text_utils::normalize_inline_text;
use super::translation_split::heuristic_split_translation;
use super::types::{
    BuildStep5TranslationAlignRequest, BuildStep5TranslationAlignResponse, Step5AlignedParent,
    Step5AlignedPart,
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
