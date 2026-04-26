use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

use crate::services::llm::batch::run_indexed_concurrent_with_progress;
use crate::services::llm::client::{LlmSemanticValidationError, OpenAiCompatLlmClient};
use crate::services::llm::port::{LlmCallContext, LlmConfig, LlmJsonTask, next_llm_request_id};
use crate::services::transcribe::WordTokenDto;
use voxtrans_core::subtitle::beautify::beautify_words_for_subtitle;
use voxtrans_core::subtitle::segmenter::WordToken;
use voxtrans_core::subtitle::srt::{SrtCue, to_srt_from_cues};

const HARD_SPLIT_GAP_MS: u64 = 2_000;
#[cfg(test)]
const DEFAULT_SUBTITLE_MAX_WORDS_PER_SEGMENT: u32 = 20;
const MAX_UNPUNCTUATED_DURATION_MS: u64 = 24_000;
const SOFT_SPLIT_GAP_MS: u64 = 350;
const MIN_SEMANTIC_SEGMENT_WORDS: usize = 5;
const MAX_LLM_SEMANTIC_CANDIDATES: usize = 16;

#[derive(Debug, Clone)]
pub struct SentenceBoundaryRequest {
    pub task_id: String,
    pub media_path: String,
    pub source_lang: String,
    pub words: Vec<WordTokenDto>,
    pub subtitle_max_words_per_segment: u32,
    pub translate_api_key: String,
    pub translate_base_url: String,
    pub translate_model: String,
    pub llm_concurrency: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceSentenceStep2 {
    pub task_id: String,
    pub media_path: String,
    pub source_lang: String,
    pub hard_split_gap_ms: u64,
    pub micro_chunk_total: usize,
    pub boundary_total: usize,
    pub sentence_total: usize,
    pub micro_chunks: Vec<MicroChunk>,
    pub boundaries: Vec<BoundaryDecision>,
    pub translation_sentences: Vec<SourceSentence>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MicroChunk {
    pub chunk_id: usize,
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
    pub word_start: usize,
    pub word_end: usize,
    pub gap_before_ms: u64,
    pub gap_after_ms: u64,
    pub hard_split_before: bool,
    pub hard_split_after: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BoundaryDecision {
    pub left_chunk_id: usize,
    pub right_chunk_id: usize,
    pub gap_ms: u64,
    pub rule_decision: BoundaryDecisionKind,
    pub llm_decision: BoundaryDecisionKind,
    pub final_decision: BoundaryDecisionKind,
    pub confidence: f64,
    pub reason_tag: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceSentence {
    pub sentence_id: usize,
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
    pub word_start: usize,
    pub word_end: usize,
    pub chunk_start: usize,
    pub chunk_end: usize,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BoundaryDecisionKind {
    HardSplit,
    Split,
    Merge,
    Unsure,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SplitReason {
    TerminalPunctuation,
    HardPause,
    LengthFallback,
    LlmSemanticRefinement,
}

#[derive(Debug, Clone)]
struct SemanticBoundaryCandidate {
    id: usize,
    split_after: usize,
    reason: String,
    score: f64,
}

#[derive(Debug, Clone)]
struct SemanticRefinementTask {
    task_id: usize,
    span_index: usize,
    span_start: usize,
    span_end: usize,
    desired_parts: usize,
    fallback_splits: Vec<usize>,
    candidates: Vec<SemanticBoundaryCandidate>,
    prompt: String,
}

pub async fn build_source_sentences_from_words_with_progress(
    request: SentenceBoundaryRequest,
    on_progress: Option<Arc<dyn Fn(usize, usize) + Send + Sync>>,
) -> Result<SourceSentenceStep2, String> {
    if request.words.is_empty() {
        return Err("words is empty".to_string());
    }

    let total = 4usize;
    if let Some(callback) = on_progress.as_ref() {
        callback(0, total);
    }

    let normalized_words = from_core_words(beautify_words_for_subtitle(to_core_words(
        request.words.clone(),
    )));
    if normalized_words.is_empty() {
        return Err("words is empty".to_string());
    }
    if let Some(callback) = on_progress.as_ref() {
        callback(1, total);
    }

    let micro_chunks = build_micro_chunks(&normalized_words);
    if micro_chunks.is_empty() {
        return Err("failed to build micro chunks".to_string());
    }
    if let Some(callback) = on_progress.as_ref() {
        callback(2, total);
    }

    let split_points =
        build_split_points_with_optional_semantic_refinement(&request, &normalized_words).await;
    let spans = split_points_to_spans(normalized_words.len(), &split_points);
    if spans.is_empty() {
        return Err("failed to build sentence spans".to_string());
    }
    if let Some(callback) = on_progress.as_ref() {
        callback(3, total);
    }

    let translation_sentences = build_sentences_from_word_spans(&normalized_words, &spans);
    let boundaries = build_boundaries_from_split_points(&micro_chunks, &split_points);
    if let Some(callback) = on_progress.as_ref() {
        callback(4, total);
    }

    Ok(SourceSentenceStep2 {
        task_id: request.task_id,
        media_path: request.media_path,
        source_lang: request.source_lang,
        hard_split_gap_ms: HARD_SPLIT_GAP_MS,
        micro_chunk_total: micro_chunks.len(),
        boundary_total: boundaries.len(),
        sentence_total: translation_sentences.len(),
        micro_chunks,
        boundaries,
        translation_sentences,
    })
}

pub fn source_sentences_to_srt(step2: &SourceSentenceStep2) -> String {
    let cues = step2
        .translation_sentences
        .iter()
        .map(|sentence| SrtCue {
            index: sentence.sentence_id,
            start_ms: sentence.start_ms,
            end_ms: sentence.end_ms,
            text: sentence.text.clone(),
        })
        .collect::<Vec<_>>();
    to_srt_from_cues(&cues)
}

#[cfg(test)]
fn build_deterministic_sentence_spans(words: &[WordTokenDto]) -> Vec<(usize, usize)> {
    let split_points = build_deterministic_split_points(
        words,
        translation_unit_word_limit(DEFAULT_SUBTITLE_MAX_WORDS_PER_SEGMENT),
    );
    split_points_to_spans(words.len(), &split_points)
}

async fn build_split_points_with_optional_semantic_refinement(
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
                    prompt: build_semantic_refinement_prompt(
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
fn build_deterministic_split_points(
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

async fn run_semantic_refinement_tasks(
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
                            |value| parse_semantic_refinement_breaks(value, refinement_task),
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

fn build_llm_semantic_candidates(
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

fn parse_semantic_refinement_breaks(
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

fn score_absolute_splits(
    splits: &[usize],
    start: usize,
    end: usize,
    desired_parts: usize,
    soft_limit: usize,
) -> f64 {
    if start > end {
        return 1_000_000.0;
    }
    let mut score = 0.0f64;
    let mut cursor = start;
    let total_words = end.saturating_sub(start) + 1;
    let target_len = (total_words as f64 / desired_parts.max(1) as f64).max(1.0);
    for split_after in splits.iter().copied().chain(std::iter::once(end)) {
        if split_after < cursor || split_after > end {
            score += 1_000.0;
            continue;
        }
        let len = split_after.saturating_sub(cursor) + 1;
        score += (len as f64 - target_len).abs();
        if len < MIN_SEMANTIC_SEGMENT_WORDS {
            score += (MIN_SEMANTIC_SEGMENT_WORDS - len) as f64 * 8.0 + 12.0;
        }
        if len > soft_limit {
            score += (len - soft_limit) as f64 * 6.0 + 10.0;
        }
        cursor = split_after + 1;
    }
    score
}

fn translation_unit_word_limit_from_span(start: usize, end: usize, desired_parts: usize) -> usize {
    let total = end.saturating_sub(start) + 1;
    ((total as f64 / desired_parts.max(1) as f64).ceil() as usize).max(MIN_SEMANTIC_SEGMENT_WORDS)
}

fn semantic_boundary_score(
    words: &[WordTokenDto],
    range_start: usize,
    range_end: usize,
    split_after: usize,
    target: usize,
    word_limit: usize,
) -> f64 {
    let left_len = split_after.saturating_sub(range_start) + 1;
    let right_len = range_end.saturating_sub(split_after);
    let mut score = split_after.abs_diff(target) as f64;
    score += semantic_boundary_penalty(words, split_after);
    if left_len < MIN_SEMANTIC_SEGMENT_WORDS {
        score += (MIN_SEMANTIC_SEGMENT_WORDS - left_len) as f64 * 8.0;
    }
    if right_len < MIN_SEMANTIC_SEGMENT_WORDS {
        score += (MIN_SEMANTIC_SEGMENT_WORDS - right_len) as f64 * 8.0;
    }
    if left_len > word_limit {
        score += (left_len - word_limit) as f64 * 2.5;
    }
    if right_len > word_limit {
        score += (right_len - word_limit) as f64 * 1.5;
    }
    score
}

fn semantic_boundary_penalty(words: &[WordTokenDto], split_after: usize) -> f64 {
    let mut penalty = 0.0f64;
    let Some(left) = words.get(split_after) else {
        return 100.0;
    };
    let Some(right) = words.get(split_after + 1) else {
        return 100.0;
    };
    let left_word = normalize_ascii_word(&left.word);
    let right_word = normalize_ascii_word(&right.word);
    let gap = gap_ms(left.end, right.start);

    if gap >= SOFT_SPLIT_GAP_MS {
        penalty -= 3.0;
    }
    if ends_with_terminal_punctuation(&left.word) {
        penalty -= 5.0;
    } else if ends_with_soft_punctuation(&left.word) {
        penalty -= 8.0;
        if is_pronoun_or_auxiliary_start(&right_word) {
            penalty += 8.0;
        }
    }
    if is_semantic_clause_start(&right_word) {
        penalty -= 7.0;
    }
    if is_dangling_tail_word(&left_word) {
        penalty += 8.0;
    }
    if is_bad_segment_start_word(&right_word) {
        penalty += 7.0;
    }
    penalty
}

fn semantic_boundary_reason(words: &[WordTokenDto], split_after: usize) -> String {
    let Some(left) = words.get(split_after) else {
        return "candidate".to_string();
    };
    let Some(right) = words.get(split_after + 1) else {
        return "candidate".to_string();
    };
    let right_word = normalize_ascii_word(&right.word);
    let gap = gap_ms(left.end, right.start);
    if gap >= SOFT_SPLIT_GAP_MS {
        "pause".to_string()
    } else if ends_with_terminal_punctuation(&left.word) {
        "terminal_punctuation".to_string()
    } else if ends_with_soft_punctuation(&left.word) {
        "soft_punctuation".to_string()
    } else if is_semantic_clause_start(&right_word) {
        format!("before_{right_word}")
    } else {
        "balanced_length".to_string()
    }
}

fn is_structural_boundary(words: &[WordTokenDto], split_after: usize) -> bool {
    let Some(left) = words.get(split_after) else {
        return false;
    };
    let Some(right) = words.get(split_after + 1) else {
        return false;
    };
    let right_word = normalize_ascii_word(&right.word);
    gap_ms(left.end, right.start) >= SOFT_SPLIT_GAP_MS
        || ends_with_terminal_punctuation(&left.word)
        || ends_with_soft_punctuation(&left.word)
        || is_semantic_clause_start(&right_word)
}

fn build_semantic_refinement_prompt(
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
        .map(|candidate| {
            serde_json::json!({
                "id": candidate.id,
                "splitAfterToken": candidate.split_after - start + 1,
                "leftPreview": preview_words(words, start, candidate.split_after, 8, false),
                "rightPreview": preview_words(words, candidate.split_after + 1, end, 8, true),
                "reason": candidate.reason,
            })
        })
        .collect::<Vec<_>>();

    serde_json::json!({
        "task": "refine_long_asr_sentence_boundaries_for_translation",
        "rule": "Think internally, but output JSON only.",
        "sourceLanguage": source_lang,
        "sourceText": source_text,
        "preferredParts": desired_parts,
        "softMaxWordsPerPart": word_limit,
        "candidateBoundaries": candidate_items,
        "constraints": [
            "Pick only ids from candidateBoundaries.",
            "Return breakIds in reading order.",
            "Split long ASR text into semantically complete translation units.",
            "Prefer likely missing punctuation and clause boundaries.",
            "Avoid fragments that start or end with dangling function words.",
            "Do not rewrite, translate, or add text."
        ],
        "output": {
            "breakIds": [1, 2]
        }
    })
    .to_string()
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

fn should_refine_semantic_span(
    words: &[WordTokenDto],
    start: usize,
    end: usize,
    word_limit: usize,
) -> bool {
    if !should_split_semantic_span(words, start, end, word_limit) {
        return false;
    }
    let required_boundaries = desired_semantic_part_count(words, start, end, word_limit)
        .saturating_sub(1)
        .max(1);
    semantic_boundary_signal_count(words, start, end) < required_boundaries
}

fn should_split_semantic_span(
    words: &[WordTokenDto],
    start: usize,
    end: usize,
    word_limit: usize,
) -> bool {
    if words.is_empty() || start >= words.len() || end >= words.len() || start >= end {
        return false;
    }
    let word_count = end.saturating_sub(start) + 1;
    word_count > word_limit || span_duration_ms(words, start, end) > MAX_UNPUNCTUATED_DURATION_MS
}

fn semantic_boundary_signal_count(words: &[WordTokenDto], start: usize, end: usize) -> usize {
    if words.is_empty() || start >= words.len() || end >= words.len() || start >= end {
        return 0;
    }

    let mut count = 0usize;
    for split_after in start..end {
        let Some(left) = words.get(split_after) else {
            continue;
        };
        let Some(right) = words.get(split_after + 1) else {
            continue;
        };
        let right_word = normalize_ascii_word(&right.word);
        if ends_with_soft_punctuation(&left.word)
            || gap_ms(left.end, right.start) >= SOFT_SPLIT_GAP_MS
            || is_semantic_clause_start(&right_word)
        {
            count += 1;
        }
    }
    count
}

fn desired_semantic_part_count(
    words: &[WordTokenDto],
    start: usize,
    end: usize,
    word_limit: usize,
) -> usize {
    if words.is_empty() || start >= words.len() || end >= words.len() || start > end {
        return 1;
    }
    let word_count = end.saturating_sub(start) + 1;
    let word_parts = if word_limit == 0 {
        1
    } else {
        word_count.div_ceil(word_limit).max(1)
    };
    let duration = span_duration_ms(words, start, end);
    let duration_parts = if duration <= MAX_UNPUNCTUATED_DURATION_MS {
        1
    } else {
        duration.div_ceil(MAX_UNPUNCTUATED_DURATION_MS).max(1) as usize
    };
    word_parts.max(duration_parts).max(1)
}

fn translation_unit_word_limit(subtitle_max_words_per_segment: u32) -> usize {
    (subtitle_max_words_per_segment.clamp(8, 40) as usize).max(MIN_SEMANTIC_SEGMENT_WORDS)
}

fn has_llm_settings(request: &SentenceBoundaryRequest) -> bool {
    !request.translate_base_url.trim().is_empty()
        && !request.translate_model.trim().is_empty()
        && !request.translate_api_key.trim().is_empty()
}

fn normalize_ascii_word(raw: &str) -> String {
    raw.chars()
        .filter(|ch| ch.is_ascii_alphabetic() || *ch == '\'')
        .flat_map(|ch| ch.to_lowercase())
        .collect::<String>()
}

fn is_semantic_clause_start(word: &str) -> bool {
    matches!(
        word,
        "although"
            | "and"
            | "as"
            | "because"
            | "before"
            | "but"
            | "due"
            | "except"
            | "if"
            | "just"
            | "maybe"
            | "or"
            | "otherwise"
            | "since"
            | "so"
            | "then"
            | "though"
            | "unless"
            | "until"
            | "when"
            | "where"
            | "while"
            | "which"
            | "yet"
    )
}

fn is_pronoun_or_auxiliary_start(word: &str) -> bool {
    matches!(
        word,
        "i" | "you"
            | "he"
            | "she"
            | "it"
            | "we"
            | "they"
            | "is"
            | "are"
            | "was"
            | "were"
            | "am"
            | "do"
            | "does"
            | "did"
            | "can"
            | "could"
            | "will"
            | "would"
            | "should"
            | "might"
            | "may"
    )
}

fn is_bad_segment_start_word(word: &str) -> bool {
    matches!(
        word,
        "a" | "an"
            | "at"
            | "by"
            | "for"
            | "from"
            | "in"
            | "into"
            | "of"
            | "on"
            | "the"
            | "to"
            | "with"
    )
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
            | "if"
            | "in"
            | "into"
            | "of"
            | "on"
            | "or"
            | "so"
            | "that"
            | "the"
            | "then"
            | "to"
            | "when"
            | "where"
            | "which"
            | "while"
            | "with"
    )
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

fn split_points_to_spans(
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

fn build_micro_chunks(words: &[WordTokenDto]) -> Vec<MicroChunk> {
    words
        .iter()
        .enumerate()
        .map(|(index, word)| {
            let gap_before_ms = index
                .checked_sub(1)
                .and_then(|prev| words.get(prev))
                .map(|prev| gap_ms(prev.end, word.start))
                .unwrap_or(0);
            let gap_after_ms = words
                .get(index + 1)
                .map(|next| gap_ms(word.end, next.start))
                .unwrap_or(0);
            MicroChunk {
                chunk_id: index + 1,
                start_ms: seconds_to_ms(word.start),
                end_ms: seconds_to_ms(word.end.max(word.start)),
                text: word.word.clone(),
                word_start: index,
                word_end: index,
                gap_before_ms,
                gap_after_ms,
                hard_split_before: gap_before_ms >= HARD_SPLIT_GAP_MS,
                hard_split_after: gap_after_ms >= HARD_SPLIT_GAP_MS,
            }
        })
        .collect()
}

fn build_sentences_from_word_spans(
    words: &[WordTokenDto],
    spans: &[(usize, usize)],
) -> Vec<SourceSentence> {
    spans
        .iter()
        .filter_map(|(start, end)| {
            if *start >= words.len() || *end >= words.len() || start > end {
                return None;
            }
            Some((*start, *end))
        })
        .enumerate()
        .map(|(index, (start, end))| SourceSentence {
            sentence_id: index + 1,
            start_ms: seconds_to_ms(words[start].start),
            end_ms: seconds_to_ms(words[end].end.max(words[start].start)),
            text: join_words(words[start..=end].iter().map(|word| word.word.as_str())),
            word_start: start,
            word_end: end,
            chunk_start: start + 1,
            chunk_end: end + 1,
        })
        .collect()
}

fn build_boundaries_from_split_points(
    micro_chunks: &[MicroChunk],
    split_points: &[(usize, SplitReason)],
) -> Vec<BoundaryDecision> {
    if micro_chunks.len() < 2 {
        return Vec::new();
    }

    let mut split_by_end = std::collections::HashMap::<usize, SplitReason>::new();
    for (end, reason) in split_points.iter().copied() {
        split_by_end.insert(end, reason);
    }

    (0..micro_chunks.len() - 1)
        .map(|index| {
            let left = &micro_chunks[index];
            let right = &micro_chunks[index + 1];
            let split_reason = split_by_end.get(&index).copied();
            let (rule_decision, llm_decision, final_decision, confidence, reason_tag) =
                match split_reason {
                    Some(SplitReason::TerminalPunctuation) => (
                        BoundaryDecisionKind::Split,
                        BoundaryDecisionKind::Unknown,
                        BoundaryDecisionKind::Split,
                        1.0,
                        "terminal_punctuation",
                    ),
                    Some(SplitReason::HardPause) => (
                        BoundaryDecisionKind::HardSplit,
                        BoundaryDecisionKind::Unknown,
                        BoundaryDecisionKind::HardSplit,
                        1.0,
                        "hard_pause",
                    ),
                    Some(SplitReason::LengthFallback) => (
                        BoundaryDecisionKind::Split,
                        BoundaryDecisionKind::Unknown,
                        BoundaryDecisionKind::Split,
                        0.82,
                        "length_fallback",
                    ),
                    Some(SplitReason::LlmSemanticRefinement) => (
                        BoundaryDecisionKind::Unsure,
                        BoundaryDecisionKind::Split,
                        BoundaryDecisionKind::Split,
                        0.9,
                        "llm_semantic_refine",
                    ),
                    None => (
                        BoundaryDecisionKind::Merge,
                        BoundaryDecisionKind::Unknown,
                        BoundaryDecisionKind::Merge,
                        0.95,
                        "merge",
                    ),
                };
            BoundaryDecision {
                left_chunk_id: left.chunk_id,
                right_chunk_id: right.chunk_id,
                gap_ms: gap_ms(
                    (left.end_ms as f64) / 1000.0,
                    (right.start_ms as f64) / 1000.0,
                ),
                rule_decision,
                llm_decision,
                final_decision,
                confidence,
                reason_tag: reason_tag.to_string(),
            }
        })
        .collect()
}

fn span_duration_ms(words: &[WordTokenDto], start: usize, end: usize) -> u64 {
    if start >= words.len() || end >= words.len() || start > end {
        return 0;
    }
    ((words[end].end - words[start].start).max(0.0) * 1000.0).round() as u64
}

fn ends_with_terminal_punctuation(word: &str) -> bool {
    word.trim_end()
        .chars()
        .last()
        .map(|ch| matches!(ch, '.' | '!' | '?' | '。' | '！' | '？'))
        .unwrap_or(false)
}

fn ends_with_soft_punctuation(word: &str) -> bool {
    word.trim_end()
        .chars()
        .last()
        .map(|ch| matches!(ch, ',' | ';' | ':' | '，' | '；' | '：' | '、'))
        .unwrap_or(false)
}

fn join_words<'a>(parts: impl Iterator<Item = &'a str>) -> String {
    let mut out = String::new();
    let mut prev_has_spacing_word = false;
    let mut prev_allows_space_after = false;

    for raw in parts {
        let token = raw.trim();
        if token.is_empty() {
            continue;
        }
        let next_has_spacing_word = token_has_spacing_word(token);
        if !out.is_empty()
            && next_has_spacing_word
            && (prev_has_spacing_word || prev_allows_space_after)
        {
            out.push(' ');
        }
        out.push_str(token);
        prev_has_spacing_word = next_has_spacing_word;
        prev_allows_space_after = token_allows_space_after(token);
    }

    out.replace(" ,", ",")
        .replace(" .", ".")
        .replace(" !", "!")
        .replace(" ?", "?")
        .replace(" :", ":")
        .replace(" ;", ";")
}

fn token_allows_space_after(token: &str) -> bool {
    token
        .chars()
        .last()
        .map(|ch| {
            matches!(
                ch,
                ',' | ';' | ':' | '?' | '!' | '.' | '，' | '；' | '：' | '？' | '！' | '。'
            )
        })
        .unwrap_or(false)
}

fn token_has_spacing_word(token: &str) -> bool {
    token
        .chars()
        .any(|ch| ch.is_ascii_alphanumeric() || is_hangul(ch))
}

fn is_hangul(ch: char) -> bool {
    matches!(
        ch as u32,
        0x1100..=0x11FF
            | 0x3130..=0x318F
            | 0xA960..=0xA97F
            | 0xAC00..=0xD7AF
            | 0xD7B0..=0xD7FF
    )
}

fn gap_ms(left_end_sec: f64, right_start_sec: f64) -> u64 {
    ((right_start_sec - left_end_sec).max(0.0) * 1000.0).round() as u64
}

fn seconds_to_ms(value: f64) -> u64 {
    (value.max(0.0) * 1000.0).round() as u64
}

fn to_core_words(words: Vec<WordTokenDto>) -> Vec<WordToken> {
    words
        .into_iter()
        .map(|word| WordToken {
            start: word.start,
            end: word.end,
            word: word.word,
        })
        .collect()
}

fn from_core_words(words: Vec<WordToken>) -> Vec<WordTokenDto> {
    words
        .into_iter()
        .map(|word| WordTokenDto {
            start: word.start,
            end: word.end,
            word: word.word,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        BoundaryDecisionKind, DEFAULT_SUBTITLE_MAX_WORDS_PER_SEGMENT, HARD_SPLIT_GAP_MS,
        build_deterministic_sentence_spans, build_micro_chunks,
        build_source_sentences_from_words_with_progress, ends_with_terminal_punctuation,
    };
    use crate::services::transcribe::WordTokenDto;

    fn w(index: usize, text: &str) -> WordTokenDto {
        let start = index as f64 * 0.5;
        WordTokenDto {
            start,
            end: start + 0.3,
            word: text.to_string(),
        }
    }

    fn request(words: Vec<WordTokenDto>) -> super::SentenceBoundaryRequest {
        super::SentenceBoundaryRequest {
            task_id: "task-1".to_string(),
            media_path: "demo.mp4".to_string(),
            source_lang: "en".to_string(),
            words,
            subtitle_max_words_per_segment: DEFAULT_SUBTITLE_MAX_WORDS_PER_SEGMENT,
            translate_api_key: String::new(),
            translate_base_url: String::new(),
            translate_model: String::new(),
            llm_concurrency: 16,
        }
    }

    #[test]
    fn deterministic_spans_split_on_terminal_punctuation() {
        let words = vec![
            w(0, "Hello"),
            w(1, "world."),
            w(2, "Next"),
            w(3, "sentence?"),
        ];

        let spans = build_deterministic_sentence_spans(&words);

        assert_eq!(spans, vec![(0, 1), (2, 3)]);
    }

    #[test]
    fn length_fallback_still_splits_overlong_terminal_sentence() {
        let words = "All right, in this video, we're going to be talking about daily review habits and how they affect your focus and your planning mindset."
            .split_whitespace()
            .enumerate()
            .map(|(index, token)| w(index, token))
            .collect::<Vec<_>>();

        let spans = build_deterministic_sentence_spans(&words);

        assert!(
            spans.len() > 1,
            "overlong terminal sentence should still be shortened before translation"
        );
        assert!(spans.iter().all(|(start, end)| end - start < 20));
    }

    #[test]
    fn length_fallback_prefers_soft_punctuation_for_very_long_runs() {
        let words = (0..45)
            .map(|index| {
                let token = if index == 29 { "checkpoint," } else { "word" };
                w(index, token)
            })
            .collect::<Vec<_>>();

        let spans = build_deterministic_sentence_spans(&words);

        assert_eq!(spans, vec![(0, 19), (20, 29), (30, 44)]);
    }

    #[test]
    fn duration_fallback_splits_slow_unpunctuated_runs_under_word_limit() {
        let words = (0..30)
            .map(|index| WordTokenDto {
                start: index as f64,
                end: index as f64 + 0.2,
                word: format!("w{index}"),
            })
            .collect::<Vec<_>>();

        let spans = build_deterministic_sentence_spans(&words);

        assert_eq!(spans, vec![(0, 14), (15, 29)]);
    }

    #[test]
    fn deterministic_spans_split_long_unpunctuated_runs_without_llm() {
        let words = (0..45)
            .map(|index| w(index, &format!("w{index}")))
            .collect::<Vec<_>>();

        let spans = build_deterministic_sentence_spans(&words);

        assert!(spans.len() > 1, "long unpunctuated ASR run should be split");
        assert_eq!(spans.first(), Some(&(0, 19)));
        assert_eq!(spans.last(), Some(&(30, 44)));
    }

    #[test]
    fn long_missing_punctuation_span_is_split_before_step4_translation() {
        let text = "It's something I've been trying to do every week just to get a good idea of how I'm performing against the reference list of literally reviewing every high quality example that I see because sometimes your execution slips, you might skip examples due to hesitation or maybe you choose weaker examples because you're not thinking straight.";
        let words = text
            .split_whitespace()
            .enumerate()
            .map(|(index, token)| w(index, token))
            .collect::<Vec<_>>();

        let spans = build_deterministic_sentence_spans(&words);
        let texts = spans
            .iter()
            .map(|(start, end)| {
                super::join_words(words[*start..=*end].iter().map(|word| word.word.as_str()))
            })
            .collect::<Vec<_>>();

        assert!(
            texts.len() >= 3,
            "long ASR span should be refined before translation"
        );
        assert!(
            texts
                .iter()
                .all(|text| text.split_whitespace().count() <= 25),
            "step4 should not receive very long translation units: {texts:?}"
        );
        assert!(
            !texts
                .first()
                .map(|text| text.contains("hesitation"))
                .unwrap_or(false),
            "first translation unit should not absorb the next idea: {texts:?}"
        );
        assert!(
            !texts
                .first()
                .map(|text| text.ends_with(','))
                .unwrap_or(false),
            "first translation unit should not be a comma-hanging half sentence: {texts:?}"
        );
    }

    #[test]
    fn long_punctuation_sparse_span_uses_llm_refinement_when_available() {
        let text = "This long sentence has no useful internal punctuation it keeps running through several separate ideas the recognizer only produced a final period";
        let words = text
            .split_whitespace()
            .enumerate()
            .map(|(index, token)| w(index, token))
            .collect::<Vec<_>>();
        let word_limit = super::translation_unit_word_limit(DEFAULT_SUBTITLE_MAX_WORDS_PER_SEGMENT);

        assert!(super::should_split_semantic_span(
            &words,
            0,
            words.len() - 1,
            word_limit
        ));
        assert!(super::should_refine_semantic_span(
            &words,
            0,
            words.len() - 1,
            word_limit
        ));
    }

    #[test]
    fn long_punctuation_rich_span_stays_on_local_boundaries() {
        let text = "First we check the outline, then we confirm the references, because timing still matters, and finally we wait for a clean draft before sending feedback.";
        let words = text
            .split_whitespace()
            .enumerate()
            .map(|(index, token)| w(index, token))
            .collect::<Vec<_>>();
        let word_limit = super::translation_unit_word_limit(DEFAULT_SUBTITLE_MAX_WORDS_PER_SEGMENT);

        assert!(super::should_split_semantic_span(
            &words,
            0,
            words.len() - 1,
            word_limit
        ));
        assert!(!super::should_refine_semantic_span(
            &words,
            0,
            words.len() - 1,
            word_limit
        ));
    }

    #[test]
    fn short_span_skips_semantic_splitting_and_refinement() {
        let text = "This short sentence is already fine.";
        let words = text
            .split_whitespace()
            .enumerate()
            .map(|(index, token)| w(index, token))
            .collect::<Vec<_>>();
        let word_limit = super::translation_unit_word_limit(DEFAULT_SUBTITLE_MAX_WORDS_PER_SEGMENT);

        assert!(!super::should_split_semantic_span(
            &words,
            0,
            words.len() - 1,
            word_limit
        ));
        assert!(!super::should_refine_semantic_span(
            &words,
            0,
            words.len() - 1,
            word_limit
        ));
    }

    #[test]
    fn short_unpunctuated_fragment_merges_into_next_punctuated_sentence() {
        let words = vec![w(0, "well"), w(1, "let's"), w(2, "start.")];

        let spans = build_deterministic_sentence_spans(&words);

        assert_eq!(spans, vec![(0, 2)]);
    }

    #[test]
    fn hard_pause_splits_even_without_punctuation() {
        let words = vec![
            WordTokenDto {
                start: 0.0,
                end: 0.2,
                word: "Okay".to_string(),
            },
            WordTokenDto {
                start: 2.4,
                end: 2.7,
                word: "next".to_string(),
            },
        ];

        let spans = build_deterministic_sentence_spans(&words);

        assert_eq!(spans, vec![(0, 0), (1, 1)]);
    }

    #[test]
    fn step2_builds_same_response_shape_without_llm_settings() {
        let words = vec![w(0, "Hello"), w(1, "world."), w(2, "Again.")];

        let response = tauri::async_runtime::block_on(
            build_source_sentences_from_words_with_progress(request(words), None),
        )
        .expect("step2 should not require llm settings");

        assert_eq!(response.sentence_total, 2);
        assert_eq!(response.translation_sentences[0].text, "Hello world.");
        assert_eq!(response.translation_sentences[1].text, "Again.");
        assert_eq!(response.boundary_total, 2);
        assert_eq!(
            response.boundaries[1].final_decision,
            BoundaryDecisionKind::Split
        );
        assert_eq!(
            response.boundaries[1].reason_tag,
            "terminal_punctuation".to_string()
        );
    }

    #[test]
    fn hard_pause_forces_micro_chunk_boundary() {
        let words = vec![
            WordTokenDto {
                start: 0.0,
                end: 0.2,
                word: "Hello".to_string(),
            },
            WordTokenDto {
                start: 2.4,
                end: 2.7,
                word: "world".to_string(),
            },
        ];

        let chunks = build_micro_chunks(&words);
        assert_eq!(chunks.len(), 2);
        assert!(chunks[0].hard_split_after);
        assert_eq!(chunks[0].gap_after_ms, HARD_SPLIT_GAP_MS + 200);
    }

    #[test]
    fn punctuation_still_closes_atom_when_available() {
        assert!(ends_with_terminal_punctuation("you."));
        assert!(ends_with_terminal_punctuation("真的吗？"));
        assert!(!ends_with_terminal_punctuation("because"));
    }

    #[test]
    fn standalone_ascii_punctuation_keeps_following_space() {
        let words = vec![w(0, "Alright"), w(1, ","), w(2, "welcome.")];

        let response = tauri::async_runtime::block_on(
            build_source_sentences_from_words_with_progress(request(words), None),
        )
        .expect("step2 should build sentence");

        assert_eq!(response.translation_sentences[0].text, "Alright, welcome.");
    }
}
