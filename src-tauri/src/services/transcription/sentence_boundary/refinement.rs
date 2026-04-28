use std::collections::HashMap;

use crate::services::llm::batch::run_indexed_concurrent_with_progress;
use crate::services::llm::client::OpenAiCompatLlmClient;
use crate::services::llm::port::{LlmCallContext, LlmConfig, LlmJsonTask, next_llm_request_id};
use crate::services::prompts::sentence_boundary::{
    SemanticBoundaryPromptCandidate,
    build_semantic_refinement_prompt as build_semantic_refinement_prompt_text,
};
use crate::services::transcribe::WordTokenDto;

use super::responses::validate_semantic_refinement_response;
use super::text::join_words;
use super::types::{SemanticBoundaryCandidate, SemanticRefinementTask, SentenceBoundaryRequest};

pub(super) async fn run_semantic_refinement_tasks(
    request: &SentenceBoundaryRequest,
    refinement_tasks: Vec<SemanticRefinementTask>,
) -> HashMap<usize, Vec<usize>> {
    if refinement_tasks.is_empty() || !has_llm_settings(request) {
        return HashMap::new();
    }

    let Ok(llm_client) = OpenAiCompatLlmClient::new(LlmConfig::new(
        request.translate_base_url.clone(),
        request.translate_api_key.clone(),
        request.translate_model.clone(),
    )) else {
        return HashMap::new();
    };

    let tasks = refinement_tasks
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
        phase: "step2_semantic_boundary_refine".to_string(),
    };
    let task_snapshot = refinement_tasks.clone();
    let results = run_indexed_concurrent_with_progress(
        tasks,
        request.llm_concurrency.max(1) as usize,
        {
            let llm_client = llm_client.clone();
            let context = context.clone();
            move |task| {
                let llm_client = llm_client.clone();
                let context = context.clone();
                let task_snapshot = task_snapshot.clone();
                async move {
                    let Some(refinement_task) = task_snapshot.get(task.id) else {
                        return Err(format!("missing semantic refinement task {}", task.id));
                    };
                    let llm_id = task.request_id.clone();
                    let call = llm_client
                        .call_json_validated(
                            &context,
                            &llm_id,
                            &task.user_prompt,
                            task.response_validator.as_ref(),
                            |value| validate_semantic_refinement_response(value, refinement_task),
                        )
                        .await
                        .map_err(|err| {
                            format!(
                                "step2 semantic boundary refine failed (llmId={}): {}",
                                llm_id, err.message
                            )
                        })?;
                    Ok((refinement_task.span_index, call.value))
                }
            }
        },
        |msg| msg,
        |_done, _total| {},
    )
    .await;

    let mut out = HashMap::<usize, Vec<usize>>::new();
    for (_, result) in results {
        let Ok((span_index, splits)) = result else {
            continue;
        };
        if !splits.is_empty() {
            out.insert(span_index, splits);
        }
    }
    out
}

pub(super) fn prepare_semantic_refinement_prompt(
    source_lang: &str,
    words: &[WordTokenDto],
    start: usize,
    end: usize,
    word_limit: usize,
    desired_parts: usize,
    candidates: &[SemanticBoundaryCandidate],
) -> String {
    let source_text = join_words(words[start..=end].iter().map(|word| word.word.as_str()));
    let candidate_items = candidates
        .iter()
        .map(|candidate| SemanticBoundaryPromptCandidate {
            id: candidate.id,
            split_after_token: candidate.split_after - start + 1,
            left_preview: preview_words(words, start, candidate.split_after, 8, false),
            right_preview: preview_words(words, candidate.split_after + 1, end, 8, true),
            reason: candidate.reason.clone(),
        })
        .collect::<Vec<_>>();
    build_semantic_refinement_prompt_text(
        source_lang,
        &source_text,
        word_limit,
        desired_parts,
        &candidate_items,
    )
}

pub(super) fn has_llm_settings(request: &SentenceBoundaryRequest) -> bool {
    !request.translate_base_url.trim().is_empty()
        && !request.translate_model.trim().is_empty()
        && !request.translate_api_key.trim().is_empty()
}

fn preview_words(
    words: &[WordTokenDto],
    start: usize,
    end: usize,
    limit: usize,
    from_start: bool,
) -> String {
    if start >= words.len() || end >= words.len() || start > end {
        return String::new();
    }
    let span = &words[start..=end];
    if span.len() <= limit {
        return join_words(span.iter().map(|word| word.word.as_str()));
    }
    if from_start {
        let text = join_words(span.iter().take(limit).map(|word| word.word.as_str()));
        format!("{text} ...")
    } else {
        let text = join_words(
            span.iter()
                .skip(span.len().saturating_sub(limit))
                .map(|word| word.word.as_str()),
        );
        format!("... {text}")
    }
}
