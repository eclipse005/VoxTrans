use std::collections::HashMap;

use crate::services::transcribe::WordTokenDto;

use super::refinement::{
    has_llm_settings, prepare_semantic_refinement_prompt, run_semantic_refinement_tasks,
};
use super::semantic_boundaries::{desired_semantic_part_count, semantic_boundary_score};
pub(super) use super::semantic_boundaries::{
    score_absolute_splits, should_refine_semantic_span, should_split_semantic_span,
    translation_unit_word_limit, translation_unit_word_limit_from_span,
};
use super::semantic_candidates::build_llm_semantic_candidates;
use super::text::ends_with_terminal_punctuation;
use super::timing::gap_ms;
use super::types::{SemanticRefinementTask, SentenceBoundaryRequest, SplitReason};
use super::{HARD_SPLIT_GAP_MS, MIN_SEMANTIC_SEGMENT_WORDS};

pub(super) async fn build_split_points_with_optional_semantic_refinement(
    request: &SentenceBoundaryRequest,
    words: &[WordTokenDto],
) -> Vec<(usize, SplitReason)> {
    let word_limit = translation_unit_word_limit(request.subtitle_max_words_per_segment);
    let priority_split_points = build_high_priority_split_points(words);
    let spans = split_points_to_spans(words.len(), &priority_split_points);
    let mut fallback_by_span = HashMap::<usize, Vec<usize>>::new();
    let mut refinement_tasks = Vec::<SemanticRefinementTask>::new();
    let llm_available = has_llm_settings(request);

    for (span_index, (start, end)) in spans.iter().copied().enumerate() {
        let fallback_splits = build_semantic_fallback_splits(words, start, end, word_limit);
        if llm_available && should_refine_semantic_span(words, start, end, word_limit) {
            let desired_parts = desired_semantic_part_count(words, start, end, word_limit);
            let candidates = build_llm_semantic_candidates(
                words,
                start,
                end,
                word_limit,
                desired_parts,
                &fallback_splits,
            );
            if !candidates.is_empty() {
                refinement_tasks.push(SemanticRefinementTask {
                    task_id: refinement_tasks.len(),
                    span_index,
                    span_start: start,
                    span_end: end,
                    desired_parts,
                    fallback_splits: fallback_splits.clone(),
                    prompt: prepare_semantic_refinement_prompt(
                        &request.source_lang,
                        words,
                        start,
                        end,
                        word_limit,
                        desired_parts,
                        &candidates,
                    ),
                    candidates,
                });
            }
        }
        fallback_by_span.insert(span_index, fallback_splits);
    }

    let llm_splits = run_semantic_refinement_tasks(request, refinement_tasks).await;
    combine_span_split_points(
        &spans,
        &priority_split_points,
        &fallback_by_span,
        &llm_splits,
    )
}

#[cfg(test)]
pub(super) fn build_deterministic_split_points(
    words: &[WordTokenDto],
    length_fallback_word_limit: usize,
) -> Vec<(usize, SplitReason)> {
    if words.is_empty() {
        return Vec::new();
    }

    let priority_split_points = build_high_priority_split_points(words);
    let spans = split_points_to_spans(words.len(), &priority_split_points);
    let mut fallback_by_span = HashMap::<usize, Vec<usize>>::new();
    let llm_splits = HashMap::<usize, Vec<usize>>::new();
    for (span_index, (start, end)) in spans.iter().copied().enumerate() {
        fallback_by_span.insert(
            span_index,
            build_semantic_fallback_splits(words, start, end, length_fallback_word_limit),
        );
    }
    combine_span_split_points(
        &spans,
        &priority_split_points,
        &fallback_by_span,
        &llm_splits,
    )
}

fn build_high_priority_split_points(words: &[WordTokenDto]) -> Vec<(usize, SplitReason)> {
    let mut out = Vec::<(usize, SplitReason)>::new();
    for index in 0..words.len() {
        let high_priority_reason = if ends_with_terminal_punctuation(&words[index].word) {
            Some(SplitReason::TerminalPunctuation)
        } else if index + 1 < words.len()
            && gap_ms(words[index].end, words[index + 1].start) >= HARD_SPLIT_GAP_MS
        {
            Some(SplitReason::HardPause)
        } else {
            None
        };

        if let Some(reason) = high_priority_reason {
            push_split_point(&mut out, index, reason);
        }
    }
    out
}

fn combine_span_split_points(
    spans: &[(usize, usize)],
    priority_split_points: &[(usize, SplitReason)],
    fallback_by_span: &HashMap<usize, Vec<usize>>,
    llm_splits: &HashMap<usize, Vec<usize>>,
) -> Vec<(usize, SplitReason)> {
    let priority_by_end = priority_split_points
        .iter()
        .copied()
        .collect::<HashMap<usize, SplitReason>>();
    let mut out = Vec::<(usize, SplitReason)>::new();
    for (span_index, (_start, end)) in spans.iter().copied().enumerate() {
        if let Some(splits) = llm_splits.get(&span_index) {
            for split_after in splits {
                push_split_point(&mut out, *split_after, SplitReason::LlmSemanticRefinement);
            }
        } else if let Some(splits) = fallback_by_span.get(&span_index) {
            for split_after in splits {
                push_split_point(&mut out, *split_after, SplitReason::LengthFallback);
            }
        }
        if let Some(reason) = priority_by_end.get(&end).copied() {
            push_split_point(&mut out, end, reason);
        }
    }
    out.sort_by_key(|(index, _)| *index);
    out.dedup_by_key(|(index, _)| *index);
    out
}

fn build_semantic_fallback_splits(
    words: &[WordTokenDto],
    start: usize,
    end: usize,
    word_limit: usize,
) -> Vec<usize> {
    if !should_split_semantic_span(words, start, end, word_limit) {
        return Vec::new();
    }
    let desired_parts = desired_semantic_part_count(words, start, end, word_limit);
    if desired_parts <= 1 || end <= start {
        return Vec::new();
    }

    let mut splits = Vec::<usize>::new();
    let total_words = end.saturating_sub(start) + 1;
    let mut cursor = start;
    for part_index in 1..desired_parts {
        let remaining_parts = desired_parts.saturating_sub(part_index);
        let target_offset = ((total_words * part_index) as f64 / desired_parts as f64).round();
        let target = start
            .saturating_add(target_offset.max(1.0) as usize)
            .saturating_sub(1)
            .min(end.saturating_sub(1));
        let min_split = cursor
            .saturating_add(MIN_SEMANTIC_SEGMENT_WORDS)
            .saturating_sub(1)
            .min(end.saturating_sub(1));
        let max_split = end.saturating_sub(remaining_parts * MIN_SEMANTIC_SEGMENT_WORDS);
        if min_split > max_split || min_split >= end {
            break;
        }

        let mut best = None::<(usize, f64)>;
        for split_after in min_split..=max_split.min(end.saturating_sub(1)) {
            let score =
                semantic_boundary_score(words, cursor, end, split_after, target, word_limit);
            match best {
                Some((_best_split, best_score)) if best_score <= score => {}
                _ => best = Some((split_after, score)),
            }
        }

        let Some((split_after, _)) = best else {
            break;
        };
        if split_after < cursor || split_after >= end {
            break;
        }
        splits.push(split_after);
        cursor = split_after + 1;
    }

    splits.sort_unstable();
    splits.dedup();
    splits
}

fn push_split_point(
    split_points: &mut Vec<(usize, SplitReason)>,
    index: usize,
    reason: SplitReason,
) {
    if split_points.last().map(|(end, _)| *end) == Some(index) {
        return;
    }
    split_points.push((index, reason));
}

pub(super) fn split_points_to_spans(
    word_total: usize,
    split_points: &[(usize, SplitReason)],
) -> Vec<(usize, usize)> {
    if word_total == 0 {
        return Vec::new();
    }

    let mut out = Vec::<(usize, usize)>::new();
    let mut cursor = 0usize;
    for (end, _) in split_points.iter().copied() {
        if end < cursor || end + 1 >= word_total {
            continue;
        }
        out.push((cursor, end));
        cursor = end + 1;
    }
    out.push((cursor, word_total - 1));
    out
}
