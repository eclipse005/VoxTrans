use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use voxtrans_core::subtitle::srt::{SrtCue, to_srt_from_cues};

use crate::services::llm::client::OpenAiCompatLlmClient;
use crate::services::llm::json_guard::JsonResponseValidator;
use crate::services::llm::port::{LlmCallContext, LlmConfig, LlmJsonTask, LlmPort};
use crate::services::task_log::TaskLogger;
use crate::services::translate::types::TranslateSegment;

const MAX_LAYOUT_ROUNDS: usize = 3;
const MAX_SPLIT_RATIO_DELTA: f64 = 0.28;
const MIN_SPLIT_SIDE_RATIO: f64 = 0.22;
const MIN_SOURCE_SIDE_UNITS: usize = 3;
const MIN_TARGET_SIDE_UNITS: usize = 6;

#[derive(Debug, Clone)]
pub struct SegmentOptimizeRequest {
    pub task_id: String,
    pub media_path: String,
    pub translate_api_key: String,
    pub translate_base_url: String,
    pub translate_model: String,
    pub llm_concurrency: u32,
    pub source_max_words_per_segment: u32,
    pub target_reference_len: u32,
    pub segments: Vec<TranslateSegment>,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SegmentOptimizeResponse {
    pub segments: Vec<TranslateSegment>,
    pub source_srt: String,
    pub target_srt: String,
    pub bilingual_srt_source_first: String,
    pub bilingual_srt_target_first: String,
    pub report: Value,
    pub applied_changes: Vec<SegmentOptimizeChange>,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SegmentOptimizeChange {
    pub kind: String,
    pub index: usize,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source_text: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub before_translated_text: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub after_translated_text: String,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub before_segments: Vec<SegmentTextPair>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub after_segments: Vec<SegmentTextPair>,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SegmentTextPair {
    pub source_text: String,
    pub translated_text: String,
}

#[derive(Debug, Clone)]
struct LayoutCandidate {
    index: usize,
}

#[derive(Debug, Clone)]
struct SplitProposal {
    index: usize,
    source_left: String,
    source_right: String,
    target_left: String,
    target_right: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
struct SplitReviewExtraction {
    #[serde(default)]
    action: Option<String>,
    #[serde(default)]
    reason: Option<String>,
    #[serde(default)]
    confidence: f64,
    #[serde(default)]
    revised: Option<SplitReviewRevised>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
struct SplitReviewRevised {
    #[serde(default)]
    source_left: Option<String>,
    #[serde(default)]
    source_right: Option<String>,
    #[serde(default)]
    target_left: Option<String>,
    #[serde(default)]
    target_right: Option<String>,
}

#[derive(Debug, Clone)]
enum ReviewedSplit {
    Pass {
        source_left: String,
        source_right: String,
        target_left: String,
        target_right: String,
        confidence: f64,
    },
    Revise {
        source_left: String,
        source_right: String,
        target_left: String,
        target_right: String,
        confidence: f64,
    },
    Reject,
}

pub async fn run_segment_optimize(
    request: SegmentOptimizeRequest,
) -> Result<SegmentOptimizeResponse, String> {
    if request.segments.is_empty() {
        return Err("segment optimize empty segments".to_string());
    }
    let mut segments = request.segments.clone();
    let mut applied_changes: Vec<SegmentOptimizeChange> = Vec::new();
    optimize_layout(
        &request,
        &mut segments,
        request.source_max_words_per_segment,
        request.target_reference_len,
        &mut applied_changes,
    )
    .await?;

    let source_srt = build_srt(&segments, false);
    let target_srt = build_srt(&segments, true);
    let bilingual_srt_source_first = build_bilingual_srt(&segments, true);
    let bilingual_srt_target_first = build_bilingual_srt(&segments, false);
    let report = json!({
        "mode": "segment_optimize",
        "appliedChangeTotal": applied_changes.len(),
        "segmentTotal": segments.len(),
    });

    let logger = TaskLogger::main_with_media(request.task_id.clone(), request.media_path.clone());
    logger.event(
        "segment_optimize.completed",
        Some(&json!({
            "segmentTotal": segments.len(),
            "appliedChangeTotal": applied_changes.len(),
        })),
    );
    logger.event(
        "segment_optimize.effect",
        Some(&json!({
            "segmentTotalBefore": request.segments.len(),
            "segmentTotalAfter": segments.len(),
            "appliedChangeTotal": applied_changes.len(),
            "appliedChanges": applied_changes.clone(),
        })),
    );

    Ok(SegmentOptimizeResponse {
        segments,
        source_srt,
        target_srt,
        bilingual_srt_source_first,
        bilingual_srt_target_first,
        report,
        applied_changes,
    })
}

async fn optimize_layout(
    request: &SegmentOptimizeRequest,
    segments: &mut Vec<TranslateSegment>,
    source_max_words_per_segment: u32,
    target_reference_len: u32,
    applied_changes: &mut Vec<SegmentOptimizeChange>,
) -> Result<(), String> {
    let llm_client = build_llm_client(request).ok();
    for _round in 0..MAX_LAYOUT_ROUNDS {
        let candidates = collect_layout_candidates(
            segments,
            source_max_words_per_segment as usize,
            target_reference_len as usize,
        );
        if candidates.is_empty() {
            break;
        }

        let mut proposals: Vec<SplitProposal> = Vec::new();
        for candidate in candidates {
            let index = candidate.index;
            if index >= segments.len() {
                continue;
            }
            let Some((s1, s2, t1, t2, _ratio_delta)) = find_best_split_pair(
                segments[index].source_text.as_str(),
                segments[index].translated_text.as_str(),
            ) else {
                continue;
            };
            proposals.push(SplitProposal {
                index,
                source_left: s1,
                source_right: s2,
                target_left: t1,
                target_right: t2,
            });
        }
        if proposals.is_empty() {
            break;
        }

        let reviewed = review_split_proposals(request, llm_client.as_ref(), segments, &proposals).await?;
        let mut approved_indexes = reviewed
            .iter()
            .filter_map(|(idx, decision)| match decision {
                ReviewedSplit::Reject => None,
                _ => Some(*idx),
            })
            .collect::<Vec<_>>();
        approved_indexes.sort_by(|a, b| b.cmp(a));

        let mut applied_any = false;
        for index in approved_indexes {
            if index >= segments.len() {
                continue;
            }
            let Some((s1, s2, t1, t2, ratio_delta, decision_tag, confidence)) = reviewed
                .iter()
                .find_map(|(idx, decision)| {
                    if *idx != index {
                        return None;
                    }
                    match decision {
                        ReviewedSplit::Pass {
                            source_left,
                            source_right,
                            target_left,
                            target_right,
                            confidence,
                        } => Some((
                            source_left.clone(),
                            source_right.clone(),
                            target_left.clone(),
                            target_right.clone(),
                            compute_ratio_delta(
                                source_left.as_str(),
                                source_right.as_str(),
                                target_left.as_str(),
                                target_right.as_str(),
                            )
                            .unwrap_or(0.99),
                            "pass".to_string(),
                            *confidence,
                        )),
                        ReviewedSplit::Revise {
                            source_left,
                            source_right,
                            target_left,
                            target_right,
                            confidence,
                        } => Some((
                            source_left.clone(),
                            source_right.clone(),
                            target_left.clone(),
                            target_right.clone(),
                            compute_ratio_delta(
                                source_left.as_str(),
                                source_right.as_str(),
                                target_left.as_str(),
                                target_right.as_str(),
                            )
                            .unwrap_or(0.99),
                            "revise".to_string(),
                            *confidence,
                        )),
                        ReviewedSplit::Reject => None,
                    }
                })
            else {
                continue;
            };
            let mid = (segments[index].start_ms + segments[index].end_ms) / 2;
            let first = TranslateSegment {
                start_ms: segments[index].start_ms,
                end_ms: mid.max(segments[index].start_ms + 1),
                source_text: s1.clone(),
                translated_text: t1.clone(),
            };
            let second = TranslateSegment {
                start_ms: first.end_ms,
                end_ms: segments[index].end_ms,
                source_text: s2.clone(),
                translated_text: t2.clone(),
            };
            let before_segments = vec![SegmentTextPair {
                source_text: segments[index].source_text.clone(),
                translated_text: segments[index].translated_text.clone(),
            }];
            let after_segments = vec![
                SegmentTextPair {
                    source_text: first.source_text.clone(),
                    translated_text: first.translated_text.clone(),
                },
                SegmentTextPair {
                    source_text: second.source_text.clone(),
                    translated_text: second.translated_text.clone(),
                },
            ];
            segments[index] = first;
            segments.insert(index + 1, second);
            applied_changes.push(SegmentOptimizeChange {
                kind: "split".to_string(),
                index,
                source_text: String::new(),
                before_translated_text: String::new(),
                after_translated_text: String::new(),
                reason: format!(
                    "segment_split_review_{decision_tag}_ratio_{ratio_delta:.2}_conf_{confidence:.2}"
                ),
                before_segments,
                after_segments,
            });
            applied_any = true;
        }
        if !applied_any {
            break;
        }
    }
    Ok(())
}

fn build_llm_client(request: &SegmentOptimizeRequest) -> Result<OpenAiCompatLlmClient, String> {
    OpenAiCompatLlmClient::new(LlmConfig::new(
        request.translate_base_url.clone(),
        request.translate_api_key.clone(),
        request.translate_model.clone(),
    ))
    .map_err(|err| err.message)
}

async fn review_split_proposals(
    request: &SegmentOptimizeRequest,
    llm_client: Option<&OpenAiCompatLlmClient>,
    segments: &[TranslateSegment],
    proposals: &[SplitProposal],
) -> Result<Vec<(usize, ReviewedSplit)>, String> {
    if proposals.is_empty() {
        return Ok(Vec::new());
    }
    let Some(client) = llm_client else {
        return Ok(proposals
            .iter()
            .map(|p| {
                (
                    p.index,
                    ReviewedSplit::Pass {
                        source_left: p.source_left.clone(),
                        source_right: p.source_right.clone(),
                        target_left: p.target_left.clone(),
                        target_right: p.target_right.clone(),
                        confidence: 0.5,
                    },
                )
            })
            .collect());
    };

    let validator = JsonResponseValidator::with_required_keys(&["action"]);
    let tasks = proposals
        .iter()
        .enumerate()
        .map(|(task_id, proposal)| {
            let current = segments
                .get(proposal.index)
                .cloned()
                .unwrap_or_else(|| TranslateSegment {
                    start_ms: 0,
                    end_ms: 0,
                    source_text: String::new(),
                    translated_text: String::new(),
                });
            let user_prompt = build_split_review_user_prompt(
                current.source_text.as_str(),
                current.translated_text.as_str(),
                proposal,
            );
            LlmJsonTask {
                id: task_id,
                system_prompt: build_split_review_system_prompt(),
                user_prompt,
                response_validator: Some(validator.clone()),
            }
        })
        .collect::<Vec<_>>();

    let context = LlmCallContext {
        task_id: request.task_id.clone(),
        media_path: Some(request.media_path.clone()),
        phase: "segment_optimize_review".to_string(),
    };
    let concurrency = request.llm_concurrency.clamp(1, 8) as usize;
    let results = client.call_batch_json(&context, tasks, concurrency).await;

    let mut reviewed: Vec<(usize, ReviewedSplit)> = Vec::with_capacity(proposals.len());
    for (task_id, result) in results {
        let Some(proposal) = proposals.get(task_id) else {
            continue;
        };
        let decision = match result {
            Ok(ok) => parse_split_review_decision(proposal, ok.json)?,
            Err(_) => ReviewedSplit::Pass {
                source_left: proposal.source_left.clone(),
                source_right: proposal.source_right.clone(),
                target_left: proposal.target_left.clone(),
                target_right: proposal.target_right.clone(),
                confidence: 0.5,
            },
        };
        reviewed.push((proposal.index, decision));
    }
    Ok(reviewed)
}

fn parse_split_review_decision(
    proposal: &SplitProposal,
    value: Value,
) -> Result<ReviewedSplit, String> {
    let extracted = serde_json::from_value::<SplitReviewExtraction>(value)
        .map_err(|err| format!("segment optimize review parse failed: {err}"))?;
    let action = extracted
        .action
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    let confidence = extracted.confidence.clamp(0.0, 1.0);
    match action.as_str() {
        "pass" => Ok(ReviewedSplit::Pass {
            source_left: proposal.source_left.clone(),
            source_right: proposal.source_right.clone(),
            target_left: proposal.target_left.clone(),
            target_right: proposal.target_right.clone(),
            confidence,
        }),
        "reject" => Ok(ReviewedSplit::Reject),
        "revise" => {
            let revised = extracted.revised.ok_or_else(|| {
                "segment optimize review invalid: revise without revised payload".to_string()
            })?;
            let source_left = revised.source_left.unwrap_or_default().trim().to_string();
            let source_right = revised.source_right.unwrap_or_default().trim().to_string();
            let target_left = revised.target_left.unwrap_or_default().trim().to_string();
            let target_right = revised.target_right.unwrap_or_default().trim().to_string();
            if source_left.is_empty()
                || source_right.is_empty()
                || target_left.is_empty()
                || target_right.is_empty()
            {
                return Ok(ReviewedSplit::Reject);
            }
            Ok(ReviewedSplit::Revise {
                source_left,
                source_right,
                target_left,
                target_right,
                confidence,
            })
        }
        _ => Ok(ReviewedSplit::Reject),
    }
}

fn compute_ratio_delta(
    source_left: &str,
    source_right: &str,
    target_left: &str,
    target_right: &str,
) -> Option<f64> {
    let source_ratio = split_ratio_from_text(source_left, source_right, true)?;
    let target_ratio = split_ratio_from_text(target_left, target_right, false)?;
    Some((source_ratio - target_ratio).abs())
}

fn build_split_review_system_prompt() -> String {
    "You are a subtitle split reviewer. Decide whether a proposed split is good. Return JSON only."
        .to_string()
}

fn build_split_review_user_prompt(
    source_text: &str,
    translated_text: &str,
    proposal: &SplitProposal,
) -> String {
    json!({
        "task": "subtitle_split_review",
        "goal": "Review the proposed split and decide pass/revise/reject.",
        "reviewRules": [
            "Prefer semantically complete chunks",
            "Source and translated split should stay proportionally aligned",
            "Do not rewrite meaning",
            "If revise, only adjust split boundaries and return both sides for source/translation"
        ],
        "original": {
            "sourceText": source_text,
            "translatedText": translated_text
        },
        "candidate": {
            "sourceLeft": proposal.source_left,
            "sourceRight": proposal.source_right,
            "targetLeft": proposal.target_left,
            "targetRight": proposal.target_right
        },
        "output": {
            "json_only": true,
            "schema": {
                "action": "pass|revise|reject",
                "reason": "string",
                "confidence": "number(0..1)",
                "revised": {
                    "sourceLeft": "string",
                    "sourceRight": "string",
                    "targetLeft": "string",
                    "targetRight": "string"
                }
            }
        }
    })
    .to_string()
}

fn find_best_split_pair(
    source_text: &str,
    translated_text: &str,
) -> Option<(String, String, String, String, f64)> {
    let source_options = split_options(source_text, true);
    let target_options = split_options(translated_text, false);
    if source_options.is_empty() || target_options.is_empty() {
        return None;
    }
    let mut best: Option<(String, String, String, String, f64, f64)> = None;
    for (s1, s2, s_ratio) in &source_options {
        for (t1, t2, t_ratio) in &target_options {
            if !is_split_alignment_balanced(s1.as_str(), s2.as_str(), t1.as_str(), t2.as_str()) {
                continue;
            }
            let ratio_delta = (s_ratio - t_ratio).abs();
            let center_penalty = (0.5 - *s_ratio).abs() + (0.5 - *t_ratio).abs();
            match &best {
                Some((_, _, _, _, best_delta, best_penalty))
                    if (*best_delta < ratio_delta)
                        || ((*best_delta - ratio_delta).abs() < 1e-6
                            && *best_penalty <= center_penalty) => {}
                _ => {
                    best = Some((
                        s1.clone(),
                        s2.clone(),
                        t1.clone(),
                        t2.clone(),
                        ratio_delta,
                        center_penalty,
                    ));
                }
            }
        }
    }
    best.map(|(s1, s2, t1, t2, ratio_delta, _)| (s1, s2, t1, t2, ratio_delta))
}

fn collect_layout_candidates(
    segments: &[TranslateSegment],
    source_word_limit: usize,
    target_char_limit: usize,
) -> Vec<LayoutCandidate> {
    segments
        .iter()
        .enumerate()
        .filter_map(|(idx, seg)| {
            let source_words = source_word_count_metric(&seg.source_text);
            let target_chars = target_char_count_metric(&seg.translated_text);
            if source_words > source_word_limit || target_chars > target_char_limit {
                Some(LayoutCandidate { index: idx })
            } else {
                None
            }
        })
        .collect()
}

fn split_options(text: &str, is_source: bool) -> Vec<(String, String, f64)> {
    let mut out: Vec<(String, String, f64)> = Vec::new();
    if let Some((l, r)) = split_text_near_middle(text) {
        if let Some(ratio) = split_ratio_from_text(&l, &r, is_source) {
            out.push((l, r, ratio));
        }
    }
    if let Some((l, r)) = split_text_by_words_near_middle(text) {
        if let Some(ratio) = split_ratio_from_text(&l, &r, is_source) {
            if !out.iter().any(|(ol, or, _)| ol == &l && or == &r) {
                out.push((l, r, ratio));
            }
        }
    }
    if out.is_empty() {
        let words = text
            .split_whitespace()
            .filter(|w| !w.trim().is_empty())
            .collect::<Vec<_>>();
        if words.len() >= 8 {
            let split_idx = words.len() / 2;
            let left = words[..split_idx].join(" ").trim().to_string();
            let right = words[split_idx..].join(" ").trim().to_string();
            if let Some(ratio) = split_ratio_from_text(&left, &right, is_source) {
                out.push((left, right, ratio));
            }
        }
    }
    out
}

fn split_ratio_from_text(left: &str, right: &str, is_source: bool) -> Option<f64> {
    let left_units = if is_source {
        source_word_count_metric(left)
    } else {
        target_char_count_metric(left)
    };
    let right_units = if is_source {
        source_word_count_metric(right)
    } else {
        target_char_count_metric(right)
    };
    split_ratio(left_units, right_units)
}

fn source_word_count_metric(text: &str) -> usize {
    text.split_whitespace()
        .map(trim_token_punctuation)
        .filter(|t| !t.is_empty())
        .filter(|t| t.chars().any(|ch| ch.is_alphabetic()))
        .count()
}

fn split_unit_count(text: &str) -> usize {
    let source_words = source_word_count_metric(text);
    if source_words > 0 {
        source_words
    } else {
        target_char_count_metric(text)
    }
}

fn split_ratio(left_units: usize, right_units: usize) -> Option<f64> {
    let total = left_units + right_units;
    if total == 0 {
        return None;
    }
    Some(left_units as f64 / total as f64)
}

fn is_split_alignment_balanced(
    source_left: &str,
    source_right: &str,
    target_left: &str,
    target_right: &str,
) -> bool {
    let source_left_units = split_unit_count(source_left);
    let source_right_units = split_unit_count(source_right);
    let target_left_units = target_char_count_metric(target_left);
    let target_right_units = target_char_count_metric(target_right);

    if source_left_units < MIN_SOURCE_SIDE_UNITS
        || source_right_units < MIN_SOURCE_SIDE_UNITS
        || target_left_units < MIN_TARGET_SIDE_UNITS
        || target_right_units < MIN_TARGET_SIDE_UNITS
    {
        return false;
    }

    let source_ratio = match split_ratio(source_left_units, source_right_units) {
        Some(v) => v,
        None => return false,
    };
    let target_ratio = match split_ratio(target_left_units, target_right_units) {
        Some(v) => v,
        None => return false,
    };

    if source_ratio < MIN_SPLIT_SIDE_RATIO || source_ratio > (1.0 - MIN_SPLIT_SIDE_RATIO) {
        return false;
    }
    if target_ratio < MIN_SPLIT_SIDE_RATIO || target_ratio > (1.0 - MIN_SPLIT_SIDE_RATIO) {
        return false;
    }

    (source_ratio - target_ratio).abs() <= MAX_SPLIT_RATIO_DELTA
}

fn target_char_count_metric(text: &str) -> usize {
    text.chars()
        .filter(|ch| !ch.is_whitespace())
        .filter(|ch| ch.is_alphabetic() || is_cjk(*ch))
        .count()
}

fn trim_token_punctuation(token: &str) -> &str {
    token.trim_matches(|c: char| !c.is_alphanumeric())
}

fn is_cjk(ch: char) -> bool {
    matches!(ch as u32, 0x4E00..=0x9FFF | 0x3400..=0x4DBF | 0xF900..=0xFAFF)
}

fn split_text_near_middle(text: &str) -> Option<(String, String)> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    if chars.len() < 8 {
        return None;
    }
    let mid = chars.len() / 2;
    let mut candidates: Vec<usize> = Vec::new();
    for (idx, (_, ch)) in chars.iter().enumerate() {
        if matches!(*ch, '，' | ',' | '。' | '.' | '；' | ';' | '：' | ':' | '！' | '!' | '？' | '?')
        {
            candidates.push(idx);
        }
    }
    if candidates.is_empty() {
        return None;
    }
    let best = candidates.into_iter().min_by_key(|idx| {
        let delta = if *idx > mid { *idx - mid } else { mid - *idx };
        (delta, *idx)
    })?;
    let split_byte = chars
        .get(best)
        .map(|(byte_idx, ch)| byte_idx + ch.len_utf8())
        .unwrap_or(text.len());
    let left = text[..split_byte].trim().to_string();
    let right = text[split_byte..].trim().to_string();
    if left.is_empty() || right.is_empty() {
        None
    } else {
        Some((left, right))
    }
}

fn split_text_by_words_near_middle(text: &str) -> Option<(String, String)> {
    let words = text
        .split_whitespace()
        .filter(|w| !w.trim().is_empty())
        .map(|w| w.trim().to_string())
        .collect::<Vec<_>>();
    if words.len() < 8 {
        return None;
    }
    let mid = words.len() / 2;
    let min_left = 3usize;
    let min_right = 3usize;
    let mut best_index: Option<(usize, usize)> = None;
    for idx in min_left..(words.len().saturating_sub(min_right)) {
        let left_last = normalize_word_for_boundary(&words[idx - 1]);
        let right_first = normalize_word_for_boundary(&words[idx]);
        if !is_safe_word_boundary(left_last.as_str(), right_first.as_str()) {
            continue;
        }
        let delta = if idx > mid { idx - mid } else { mid - idx };
        match best_index {
            Some((best_delta, best_idx)) if best_delta < delta || (best_delta == delta && best_idx <= idx) => {}
            _ => best_index = Some((delta, idx)),
        }
    }
    let split_idx = best_index.map(|(_, idx)| idx)?;
    let left = words[..split_idx].join(" ").trim().to_string();
    let right = words[split_idx..].join(" ").trim().to_string();
    if left.is_empty() || right.is_empty() {
        None
    } else {
        Some((left, right))
    }
}

fn normalize_word_for_boundary(token: &str) -> String {
    token
        .trim_matches(|c: char| !c.is_alphabetic())
        .to_ascii_lowercase()
}

fn is_safe_word_boundary(left_last: &str, right_first: &str) -> bool {
    if left_last.is_empty() || right_first.is_empty() {
        return false;
    }
    !is_weak_boundary_word(left_last) && !is_weak_boundary_word(right_first)
}

fn is_weak_boundary_word(word: &str) -> bool {
    matches!(
        word,
        "a"
            | "an"
            | "the"
            | "to"
            | "of"
            | "in"
            | "on"
            | "at"
            | "for"
            | "with"
            | "and"
            | "or"
            | "but"
            | "so"
            | "because"
            | "if"
            | "then"
            | "that"
            | "this"
            | "these"
            | "those"
            | "is"
            | "are"
            | "was"
            | "were"
            | "be"
            | "been"
            | "being"
            | "do"
            | "does"
            | "did"
            | "have"
            | "has"
            | "had"
            | "will"
            | "would"
            | "can"
            | "could"
            | "should"
            | "may"
            | "might"
            | "must"
    )
}

fn build_srt(segments: &[TranslateSegment], translated: bool) -> String {
    let cues = segments
        .iter()
        .enumerate()
        .map(|(idx, segment)| SrtCue {
            index: idx + 1,
            start_ms: segment.start_ms,
            end_ms: segment.end_ms.max(segment.start_ms),
            text: if translated {
                segment.translated_text.trim().to_string()
            } else {
                segment.source_text.trim().to_string()
            },
        })
        .collect::<Vec<_>>();
    to_srt_from_cues(&cues)
}

fn build_bilingual_srt(segments: &[TranslateSegment], source_first: bool) -> String {
    let cues = segments
        .iter()
        .enumerate()
        .map(|(idx, segment)| {
            let first = if source_first {
                segment.source_text.trim()
            } else {
                segment.translated_text.trim()
            };
            let second = if source_first {
                segment.translated_text.trim()
            } else {
                segment.source_text.trim()
            };
            SrtCue {
                index: idx + 1,
                start_ms: segment.start_ms,
                end_ms: segment.end_ms.max(segment.start_ms),
                text: format!("{first}\n{second}"),
            }
        })
        .collect::<Vec<_>>();
    to_srt_from_cues(&cues)
}
