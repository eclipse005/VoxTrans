use std::collections::HashMap;

use crate::services::transcribe::WordTokenDto;

use super::MAX_LLM_SEMANTIC_CANDIDATES;
use super::semantic_boundaries::{
    is_structural_boundary, semantic_boundary_reason, semantic_boundary_score,
};
use super::types::SemanticBoundaryCandidate;

pub(super) fn build_llm_semantic_candidates(
    words: &[WordTokenDto],
    start: usize,
    end: usize,
    word_limit: usize,
    desired_parts: usize,
    fallback_splits: &[usize],
) -> Vec<SemanticBoundaryCandidate> {
    if end <= start {
        return Vec::new();
    }

    let total_words = end.saturating_sub(start) + 1;
    let targets = (1..desired_parts)
        .map(|part_index| {
            let target_offset = ((total_words * part_index) as f64 / desired_parts as f64).round();
            start
                .saturating_add(target_offset.max(1.0) as usize)
                .saturating_sub(1)
                .min(end.saturating_sub(1))
        })
        .collect::<Vec<_>>();
    let mut candidate_by_split = HashMap::<usize, SemanticBoundaryCandidate>::new();

    for split_after in start..end {
        let nearest_target = targets
            .iter()
            .map(|target| split_after.abs_diff(*target))
            .min()
            .unwrap_or(usize::MAX);
        let structural = is_structural_boundary(words, split_after);
        let near_target = nearest_target <= word_limit.max(4) / 3;
        if !structural && !near_target && !fallback_splits.contains(&split_after) {
            continue;
        }
        let target = targets
            .iter()
            .min_by_key(|target| split_after.abs_diff(**target))
            .copied()
            .unwrap_or(split_after);
        let reason = semantic_boundary_reason(words, split_after);
        let score = semantic_boundary_score(words, start, end, split_after, target, word_limit);
        candidate_by_split.insert(
            split_after,
            SemanticBoundaryCandidate {
                id: 0,
                split_after,
                reason,
                score,
            },
        );
    }

    for split_after in fallback_splits {
        if *split_after >= start && *split_after < end {
            let target = targets
                .iter()
                .min_by_key(|target| split_after.abs_diff(**target))
                .copied()
                .unwrap_or(*split_after);
            candidate_by_split
                .entry(*split_after)
                .or_insert_with(|| SemanticBoundaryCandidate {
                    id: 0,
                    split_after: *split_after,
                    reason: "local_fallback".to_string(),
                    score: semantic_boundary_score(
                        words,
                        start,
                        end,
                        *split_after,
                        target,
                        word_limit,
                    ),
                });
        }
    }

    let mut candidates = candidate_by_split.into_values().collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        left.score
            .partial_cmp(&right.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.split_after.cmp(&right.split_after))
    });
    candidates.truncate(MAX_LLM_SEMANTIC_CANDIDATES);
    candidates.sort_by_key(|candidate| candidate.split_after);
    for (index, candidate) in candidates.iter_mut().enumerate() {
        candidate.id = index + 1;
    }
    candidates
}
