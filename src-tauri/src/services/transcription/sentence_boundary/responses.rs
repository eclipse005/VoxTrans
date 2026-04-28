use std::collections::HashMap;

use serde_json::Value;

use crate::services::llm::client::LlmSemanticValidationError;

use super::semantic::{score_absolute_splits, translation_unit_word_limit_from_span};
use super::types::SemanticRefinementTask;

pub(super) fn validate_semantic_refinement_response(
    value: Value,
    task: &SemanticRefinementTask,
) -> Result<Vec<usize>, LlmSemanticValidationError> {
    let Some(items) = value
        .get("breakIds")
        .or_else(|| value.get("break_ids"))
        .or_else(|| value.get("boundaryIds"))
        .or_else(|| value.get("boundaries"))
        .or_else(|| value.get("splits"))
        .and_then(|v| v.as_array())
    else {
        return Err(LlmSemanticValidationError::retryable(
            "breakIds array is required",
        ));
    };

    let candidate_by_id = task
        .candidates
        .iter()
        .map(|candidate| (candidate.id, candidate.split_after))
        .collect::<HashMap<_, _>>();
    let mut selected_splits = Vec::<usize>::new();
    for item in items {
        let id = item
            .as_u64()
            .map(|v| v as usize)
            .or_else(|| item.as_str().and_then(|s| s.trim().parse::<usize>().ok()));
        let Some(id) = id else {
            continue;
        };
        let Some(split_after) = candidate_by_id.get(&id).copied() else {
            return Err(LlmSemanticValidationError::retryable(format!(
                "break id {id} is not in candidateBoundaries",
            )));
        };
        selected_splits.push(split_after);
    }

    selected_splits.sort_unstable();
    selected_splits.dedup();
    validate_semantic_splits(&selected_splits, task)
}

fn validate_semantic_splits(
    selected_splits: &[usize],
    task: &SemanticRefinementTask,
) -> Result<Vec<usize>, LlmSemanticValidationError> {
    let required_boundaries = task.desired_parts.saturating_sub(1);
    if selected_splits.len() < required_boundaries {
        return Err(LlmSemanticValidationError::retryable(format!(
            "expected at least {required_boundaries} break ids",
        )));
    }
    if selected_splits
        .iter()
        .any(|split| *split < task.span_start || *split >= task.span_end)
    {
        return Err(LlmSemanticValidationError::retryable(
            "break ids must stay inside the span",
        ));
    }

    let selected_score = score_absolute_splits(
        selected_splits,
        task.span_start,
        task.span_end,
        task.desired_parts,
        translation_unit_word_limit_from_span(task.span_start, task.span_end, task.desired_parts),
    );
    let fallback_score = score_absolute_splits(
        &task.fallback_splits,
        task.span_start,
        task.span_end,
        task.desired_parts,
        translation_unit_word_limit_from_span(task.span_start, task.span_end, task.desired_parts),
    );
    if selected_score > fallback_score * 1.6 + 12.0 {
        return Err(LlmSemanticValidationError::retryable(
            "selected break ids are much less balanced than local fallback",
        ));
    }

    Ok(selected_splits.to_vec())
}
