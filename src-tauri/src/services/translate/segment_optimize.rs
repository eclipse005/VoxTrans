use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use serde_json::{Value, json};
use voxtrans_core::subtitle::srt::{SrtCue, to_srt_from_cues};

use crate::services::llm::client::OpenAiCompatLlmClient;
use crate::services::llm::json_guard::JsonResponseValidator;
use crate::services::llm::port::{LlmCallContext, LlmConfig, LlmJsonTask, LlmPort};
use crate::services::task_log::TaskLogger;
use crate::services::translate::prompt::{
    SegmentOptimizePromptCandidateInput, SegmentOptimizePromptConstraints,
    SegmentOptimizePromptInput, SegmentOptimizePromptSegmentInput,
    build_segment_optimize_system_prompt, build_segment_optimize_user_prompt,
};
use crate::services::translate::types::TranslateSegment;

const MAX_LAYOUT_ROUNDS: usize = 3;
const MAX_SPLIT_OPTIONS_PER_SIDE: usize = 4;
const MAX_PROPOSALS_PER_MODE: usize = 3;
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
    pub mode: String,
    pub timing_strategy: String,
    pub confidence: f64,
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
    source_overlong: bool,
    target_overlong: bool,
}

#[derive(Debug, Clone)]
struct SplitProposal {
    candidate_id: String,
    index: usize,
    mode: LayoutMode,
    timing_strategy: TimingStrategy,
    source_left: String,
    source_right: String,
    target_left: String,
    target_right: String,
    confidence: f64,
    reason: String,
    split_ratio: Option<f64>,
}

#[derive(Debug, Clone)]
struct SplitDecisionGroup {
    index: usize,
    preferred_mode: LayoutMode,
    proposals: Vec<SplitProposal>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
struct SegmentOptimizeExtraction {
    #[serde(default)]
    action: String,
    #[serde(default)]
    candidate_id: String,
    #[serde(default)]
    segments: Vec<SegmentOptimizeExtractionSegment>,
    #[serde(default)]
    reason: String,
    #[serde(default)]
    confidence: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
struct SegmentOptimizeExtractionSegment {
    #[serde(default)]
    source_text: String,
    #[serde(default)]
    translated_text: String,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum LayoutMode {
    DualSplit,
    SourceOnlySplit,
    TargetOnlyAdjust,
}

impl LayoutMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::DualSplit => "dual_split",
            Self::SourceOnlySplit => "source_only_split",
            Self::TargetOnlyAdjust => "target_only_adjust",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum TimingStrategy {
    SourceWordAlign,
    ProportionalDuration,
}

impl TimingStrategy {
    fn as_str(self) -> &'static str {
        match self {
            Self::SourceWordAlign => "source_word_align",
            Self::ProportionalDuration => "proportional_duration",
        }
    }
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
        "skipWordRealign": applied_changes.iter().any(|change| change.timing_strategy == "proportional_duration"),
        "appliedChanges": applied_changes,
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
    _request: &SegmentOptimizeRequest,
    segments: &mut Vec<TranslateSegment>,
    source_max_words_per_segment: u32,
    target_reference_len: u32,
    applied_changes: &mut Vec<SegmentOptimizeChange>,
) -> Result<(), String> {
    let llm_client = build_llm_client(_request).ok();
    for _round in 0..MAX_LAYOUT_ROUNDS {
        let candidates = collect_layout_candidates(
            segments,
            source_max_words_per_segment as usize,
            target_reference_len as usize,
        );
        if candidates.is_empty() {
            break;
        }

        let mut decision_groups: Vec<SplitDecisionGroup> = Vec::new();
        for candidate in candidates {
            let index = candidate.index;
            if index >= segments.len() {
                continue;
            }
            let proposals = collect_split_proposals(
                index,
                &segments[index],
                candidate,
            );
            if proposals.is_empty() {
                continue;
            }
            let preferred_mode = proposals[0].mode;
            decision_groups.push(SplitDecisionGroup {
                index,
                preferred_mode,
                proposals,
            });
        }
        if decision_groups.is_empty() {
            break;
        }

        let mut proposals =
            review_split_proposals(_request, llm_client.as_ref(), segments, decision_groups).await?;
        if proposals.is_empty() {
            break;
        }

        let mut applied_any = false;
        proposals.sort_by(|a, b| b.index.cmp(&a.index));
        for proposal in proposals {
            let index = proposal.index;
            if index >= segments.len() {
                continue;
            }
            let s1 = proposal.source_left.clone();
            let s2 = proposal.source_right.clone();
            let t1 = proposal.target_left.clone();
            let t2 = proposal.target_right.clone();
            let _ratio_delta = compute_ratio_delta(
                s1.as_str(),
                s2.as_str(),
                t1.as_str(),
                t2.as_str(),
            )
            .unwrap_or_else(|| proposal.split_ratio.map(|ratio| (ratio - 0.5).abs()).unwrap_or(0.99));
            let (first_end_ms, second_start_ms) = split_segment_timing(
                segments[index].start_ms,
                segments[index].end_ms,
                proposal.split_ratio,
                proposal.timing_strategy,
            );
            let first = TranslateSegment {
                start_ms: segments[index].start_ms,
                end_ms: first_end_ms,
                source_text: s1.clone(),
                translated_text: t1.clone(),
            };
            let second = TranslateSegment {
                start_ms: second_start_ms,
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
                mode: proposal.mode.as_str().to_string(),
                timing_strategy: proposal.timing_strategy.as_str().to_string(),
                confidence: proposal.confidence,
                source_text: String::new(),
                before_translated_text: String::new(),
                after_translated_text: String::new(),
                reason: proposal.reason.clone(),
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
    if request.translate_api_key.trim().is_empty()
        || request.translate_base_url.trim().is_empty()
        || request.translate_model.trim().is_empty()
    {
        return Err("segment optimize llm config missing".to_string());
    }
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
    decision_groups: Vec<SplitDecisionGroup>,
) -> Result<Vec<SplitProposal>, String> {
    let Some(client) = llm_client else {
        return Ok(decision_groups
            .into_iter()
            .filter_map(|group| group.proposals.into_iter().next())
            .collect());
    };
    if decision_groups.is_empty() {
        return Ok(Vec::new());
    }

    let validator = JsonResponseValidator::with_required_keys(&[
        "action",
        "candidateId",
        "segments",
        "reason",
        "confidence",
    ]);
    let tasks = decision_groups
        .iter()
        .enumerate()
        .map(|(task_id, group)| {
            let current = segments
                .get(group.index)
                .cloned()
                .unwrap_or_else(|| TranslateSegment {
                    start_ms: 0,
                    end_ms: 0,
                    source_text: String::new(),
                    translated_text: String::new(),
                });
            let user_prompt = build_segment_optimize_user_prompt(&SegmentOptimizePromptInput {
                preferred_mode: group.preferred_mode.as_str().to_string(),
                source_text: current.source_text,
                translated_text: current.translated_text,
                candidate_actions: group
                    .proposals
                    .iter()
                    .map(|proposal| SegmentOptimizePromptCandidateInput {
                        candidate_id: proposal.candidate_id.clone(),
                        action: proposal.mode.as_str().to_string(),
                        timing_strategy: proposal.timing_strategy.as_str().to_string(),
                        segments: vec![
                            SegmentOptimizePromptSegmentInput {
                                source_text: proposal.source_left.clone(),
                                translated_text: proposal.target_left.clone(),
                            },
                            SegmentOptimizePromptSegmentInput {
                                source_text: proposal.source_right.clone(),
                                translated_text: proposal.target_right.clone(),
                            },
                        ],
                        constraints: prompt_constraints_for_mode(
                            proposal.mode,
                            proposal.timing_strategy,
                        ),
                    })
                    .collect(),
            });
            LlmJsonTask {
                id: task_id,
                system_prompt: build_segment_optimize_system_prompt(),
                user_prompt,
                response_validator: Some(validator.clone()),
            }
        })
        .collect::<Vec<_>>();

    let context = LlmCallContext {
        task_id: request.task_id.clone(),
        media_path: Some(request.media_path.clone()),
        phase: "segment_optimize".to_string(),
    };
    let concurrency = request.llm_concurrency.clamp(1, 8) as usize;
    let results = client.call_batch_json(&context, tasks, concurrency).await;

    let mut reviewed = Vec::with_capacity(decision_groups.len());
    for (task_id, result) in results {
        let Some(group) = decision_groups.get(task_id) else {
            continue;
        };
        match result {
            Ok(ok) => match parse_segment_optimize_decision(group, ok.json) {
                Ok(Some(parsed)) => reviewed.push(parsed),
                Ok(None) => {}
                Err(_) => {
                    if let Some(fallback) = group.proposals.first() {
                        reviewed.push(fallback.clone());
                    }
                }
            },
            Err(_) => {
                if let Some(fallback) = group.proposals.first() {
                    reviewed.push(fallback.clone());
                }
            }
        }
    }
    Ok(reviewed)
}

fn prompt_constraints_for_mode(
    mode: LayoutMode,
    timing_strategy: TimingStrategy,
) -> SegmentOptimizePromptConstraints {
    match mode {
        LayoutMode::DualSplit => SegmentOptimizePromptConstraints {
            allow_split_source: true,
            allow_split_target: true,
            allow_reuse_target: false,
            allow_proportional_timing: false,
        },
        LayoutMode::SourceOnlySplit => SegmentOptimizePromptConstraints {
            allow_split_source: true,
            allow_split_target: false,
            allow_reuse_target: true,
            allow_proportional_timing: false,
        },
        LayoutMode::TargetOnlyAdjust => SegmentOptimizePromptConstraints {
            allow_split_source: timing_strategy == TimingStrategy::SourceWordAlign,
            allow_split_target: true,
            allow_reuse_target: false,
            allow_proportional_timing: timing_strategy == TimingStrategy::ProportionalDuration,
        },
    }
}

fn parse_segment_optimize_decision(
    group: &SplitDecisionGroup,
    value: Value,
) -> Result<Option<SplitProposal>, String> {
    let extracted = serde_json::from_value::<SegmentOptimizeExtraction>(value)
        .map_err(|err| format!("segment optimize parse failed: {err}"))?;
    let action = extracted.action.trim().to_ascii_lowercase();
    if matches!(action.as_str(), "no_change" | "reject" | "skip") {
        return Ok(None);
    }
    let candidate_id = extracted.candidate_id.trim();
    let Some(selected) = group
        .proposals
        .iter()
        .find(|proposal| proposal.candidate_id == candidate_id && proposal.mode.as_str() == action)
        .or_else(|| group.proposals.iter().find(|proposal| proposal.mode.as_str() == action))
    else {
        return Err(format!("unsupported segment optimize action: {}", extracted.action));
    };

    let parsed_segments = if extracted.segments.len() == 2 {
        let first = &extracted.segments[0];
        let second = &extracted.segments[1];
        let source_left = first.source_text.trim().to_string();
        let source_right = second.source_text.trim().to_string();
        let target_left = first.translated_text.trim().to_string();
        let target_right = second.translated_text.trim().to_string();
        if source_left.is_empty()
            || source_right.is_empty()
            || target_left.is_empty()
            || target_right.is_empty()
        {
            None
        } else {
            Some((source_left, source_right, target_left, target_right))
        }
    } else {
        None
    };

    let (source_left, source_right, target_left, target_right) = parsed_segments.unwrap_or_else(|| {
        (
            selected.source_left.clone(),
            selected.source_right.clone(),
            selected.target_left.clone(),
            selected.target_right.clone(),
        )
    });

    validate_mode_output(
        selected.mode,
        &source_left,
        &source_right,
        &target_left,
        &target_right,
    )?;

    let recomputed_split_ratio = recompute_split_ratio(
        selected.mode,
        selected.timing_strategy,
        &source_left,
        &source_right,
        &target_left,
        &target_right,
    )
    .or(selected.split_ratio);

    Ok(Some(SplitProposal {
        candidate_id: selected.candidate_id.clone(),
        index: selected.index,
        mode: selected.mode,
        timing_strategy: selected.timing_strategy,
        source_left,
        source_right,
        target_left,
        target_right,
        confidence: extracted.confidence.clamp(0.0, 1.0),
        reason: if extracted.reason.trim().is_empty() {
            selected.reason.clone()
        } else {
            format!("{}.llm_{}", selected.reason, normalize_reason_token(&extracted.reason))
        },
        split_ratio: recomputed_split_ratio,
    }))
}

fn recompute_split_ratio(
    mode: LayoutMode,
    timing_strategy: TimingStrategy,
    source_left: &str,
    source_right: &str,
    target_left: &str,
    target_right: &str,
) -> Option<f64> {
    match timing_strategy {
        TimingStrategy::ProportionalDuration => {
            split_ratio_from_text(target_left, target_right, false)
        }
        TimingStrategy::SourceWordAlign => match mode {
            LayoutMode::DualSplit | LayoutMode::SourceOnlySplit => {
                split_ratio_from_text(source_left, source_right, true)
            }
            LayoutMode::TargetOnlyAdjust => split_ratio_from_text(target_left, target_right, false),
        },
    }
}

fn validate_mode_output(
    mode: LayoutMode,
    source_left: &str,
    source_right: &str,
    target_left: &str,
    target_right: &str,
) -> Result<(), String> {
    match mode {
        LayoutMode::DualSplit => {
            if source_left == source_right || target_left == target_right {
                return Err("dual_split requires both source and target to actually split".to_string());
            }
        }
        LayoutMode::SourceOnlySplit => {
            if source_left == source_right {
                return Err("source_only_split requires source to actually split".to_string());
            }
        }
        LayoutMode::TargetOnlyAdjust => {
            if target_left == target_right {
                return Err("target_only_adjust requires target to actually split".to_string());
            }
        }
    }
    Ok(())
}

fn normalize_reason_token(reason: &str) -> String {
    let normalized = reason
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch.to_ascii_lowercase() } else { '_' })
        .collect::<String>();
    normalized
        .split('_')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("_")
}

fn collect_split_proposals(
    index: usize,
    segment: &TranslateSegment,
    candidate: LayoutCandidate,
) -> Vec<SplitProposal> {
    let mut proposals = Vec::new();
    let source_options = split_options(segment.source_text.as_str(), true);
    let target_options = split_options(segment.translated_text.as_str(), false);

    if candidate.source_overlong {
        for (rank, (source_left, source_right, target_left, target_right, ratio_delta, source_ratio)) in
            rank_dual_split_pairs(&source_options, &target_options)
                .into_iter()
                .take(MAX_PROPOSALS_PER_MODE)
                .enumerate()
        {
            proposals.push(SplitProposal {
                candidate_id: format!("dual_split_{rank}"),
                index,
                mode: LayoutMode::DualSplit,
                timing_strategy: TimingStrategy::SourceWordAlign,
                source_left,
                source_right,
                target_left,
                target_right,
                confidence: score_confidence(ratio_delta, true),
                reason: "source_overlong.dual_split".to_string(),
                split_ratio: Some(source_ratio),
            });
        }

        let translated = segment.translated_text.trim().to_string();
        if !translated.is_empty() {
            for (rank, (source_left, source_right, source_ratio)) in source_options
                .iter()
                .cloned()
                .take(MAX_PROPOSALS_PER_MODE)
                .enumerate()
            {
                proposals.push(SplitProposal {
                    candidate_id: format!("source_only_split_{rank}"),
                    index,
                    mode: LayoutMode::SourceOnlySplit,
                    timing_strategy: TimingStrategy::SourceWordAlign,
                    source_left,
                    source_right,
                    target_left: translated.clone(),
                    target_right: translated.clone(),
                    confidence: score_confidence((source_ratio - 0.5).abs(), false),
                    reason: "source_overlong.source_only_split".to_string(),
                    split_ratio: Some(source_ratio),
                });
            }
        }
    }

    if candidate.target_overlong {
        for (rank, (source_left, source_right, target_left, target_right, ratio_delta, source_ratio)) in
            rank_target_driven_dual_split_pairs(&source_options, &target_options)
                .into_iter()
                .take(MAX_PROPOSALS_PER_MODE)
                .enumerate()
        {
            proposals.push(SplitProposal {
                candidate_id: format!("target_dual_split_{rank}"),
                index,
                mode: LayoutMode::DualSplit,
                timing_strategy: TimingStrategy::SourceWordAlign,
                source_left,
                source_right,
                target_left,
                target_right,
                confidence: score_confidence(ratio_delta, true),
                reason: "target_overlong.target_promoted_dual_split".to_string(),
                split_ratio: Some(source_ratio),
            });
        }

        let original_source = segment.source_text.trim().to_string();
        if !original_source.is_empty() {
            for (rank, (target_left, target_right, target_ratio)) in target_options
                .iter()
                .cloned()
                .take(MAX_PROPOSALS_PER_MODE)
                .enumerate()
            {
                proposals.push(SplitProposal {
                    candidate_id: format!("target_only_adjust_{rank}"),
                    index,
                    mode: LayoutMode::TargetOnlyAdjust,
                    timing_strategy: TimingStrategy::ProportionalDuration,
                    source_left: original_source.clone(),
                    source_right: original_source.clone(),
                    target_left,
                    target_right,
                    confidence: score_confidence((target_ratio - 0.5).abs(), false),
                    reason: "target_overlong.target_only_adjust".to_string(),
                    split_ratio: Some(target_ratio),
                });
            }
        }
    }

    dedupe_split_proposals(proposals)
}

fn dedupe_split_proposals(proposals: Vec<SplitProposal>) -> Vec<SplitProposal> {
    let mut out: Vec<SplitProposal> = Vec::new();
    for proposal in proposals {
        if out.iter().any(|existing| {
            existing.mode.as_str() == proposal.mode.as_str()
                && existing.source_left == proposal.source_left
                && existing.source_right == proposal.source_right
                && existing.target_left == proposal.target_left
                && existing.target_right == proposal.target_right
        }) {
            continue;
        }
        out.push(proposal);
    }
    out
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
            let source_overlong = source_words > source_word_limit;
            let target_overlong = target_chars > target_char_limit;
            if source_overlong || target_overlong {
                Some(LayoutCandidate {
                    index: idx,
                    source_overlong,
                    target_overlong,
                })
            } else {
                None
            }
        })
        .collect()
}

fn rank_dual_split_pairs(
    source_options: &[(String, String, f64)],
    target_options: &[(String, String, f64)],
) -> Vec<(String, String, String, String, f64, f64)> {
    rank_split_pairs(source_options, target_options, false)
}

fn rank_target_driven_dual_split_pairs(
    source_options: &[(String, String, f64)],
    target_options: &[(String, String, f64)],
) -> Vec<(String, String, String, String, f64, f64)> {
    rank_split_pairs(source_options, target_options, true)
}

fn rank_split_pairs(
    source_options: &[(String, String, f64)],
    target_options: &[(String, String, f64)],
    allow_target_driven: bool,
) -> Vec<(String, String, String, String, f64, f64)> {
    if source_options.is_empty() || target_options.is_empty() {
        return Vec::new();
    }
    let mut ranked: Vec<(String, String, String, String, f64, f64, f64)> = Vec::new();
    for (s1, s2, s_ratio) in source_options {
        for (t1, t2, t_ratio) in target_options {
            let balanced = if allow_target_driven {
                is_target_driven_split_balanced(s1.as_str(), s2.as_str(), t1.as_str(), t2.as_str())
            } else {
                is_split_alignment_balanced(s1.as_str(), s2.as_str(), t1.as_str(), t2.as_str())
            };
            if !balanced {
                continue;
            }
            let ratio_delta = (s_ratio - t_ratio).abs();
            let center_penalty = (0.5 - *s_ratio).abs() + (0.5 - *t_ratio).abs();
            ranked.push((
                s1.clone(),
                s2.clone(),
                t1.clone(),
                t2.clone(),
                ratio_delta,
                *s_ratio,
                center_penalty,
            ));
        }
    }
    ranked.sort_by(|a, b| {
        a.4.partial_cmp(&b.4)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.6.partial_cmp(&b.6).unwrap_or(std::cmp::Ordering::Equal))
    });
    ranked
        .into_iter()
        .map(|(s1, s2, t1, t2, ratio_delta, source_ratio, _)| {
            (s1, s2, t1, t2, ratio_delta, source_ratio)
        })
        .collect()
}

fn is_target_driven_split_balanced(
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
    let Some(source_ratio) = split_ratio(source_left_units, source_right_units) else {
        return false;
    };
    source_ratio >= MIN_SPLIT_SIDE_RATIO && source_ratio <= (1.0 - MIN_SPLIT_SIDE_RATIO)
}

fn score_confidence(ratio_delta: f64, balanced: bool) -> f64 {
    let base = if balanced { 0.92 } else { 0.78 };
    (base - ratio_delta.min(0.5) * 0.6).clamp(0.0, 0.99)
}

fn split_segment_timing(
    start_ms: u64,
    end_ms: u64,
    split_ratio: Option<f64>,
    timing_strategy: TimingStrategy,
) -> (u64, u64) {
    let duration = end_ms.saturating_sub(start_ms).max(2);
    let split_at = match timing_strategy {
        TimingStrategy::SourceWordAlign => start_ms + duration / 2,
        TimingStrategy::ProportionalDuration => {
            let ratio = split_ratio.unwrap_or(0.5).clamp(MIN_SPLIT_SIDE_RATIO, 1.0 - MIN_SPLIT_SIDE_RATIO);
            let offset = ((duration as f64) * ratio).round() as u64;
            start_ms + offset.clamp(1, duration.saturating_sub(1))
        }
    };
    let first_end = split_at.max(start_ms + 1).min(end_ms.saturating_sub(1).max(start_ms + 1));
    let second_start = first_end.min(end_ms.saturating_sub(1)).max(start_ms + 1);
    (first_end, second_start)
}

fn split_options(text: &str, is_source: bool) -> Vec<(String, String, f64)> {
    let mut out: Vec<(String, String, f64)> = Vec::new();
    for (left, right) in split_text_by_punctuation_candidates(text) {
        push_split_option(&mut out, left, right, is_source);
    }
    for (left, right) in split_text_by_word_boundary_candidates(text) {
        push_split_option(&mut out, left, right, is_source);
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
            push_split_option(&mut out, left, right, is_source);
        }
    }
    out.sort_by(|a, b| {
        let da = (0.5 - a.2).abs();
        let db = (0.5 - b.2).abs();
        da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
    });
    out.truncate(MAX_SPLIT_OPTIONS_PER_SIDE);
    out
}

fn push_split_option(
    out: &mut Vec<(String, String, f64)>,
    left: String,
    right: String,
    is_source: bool,
) {
    if left.is_empty() || right.is_empty() {
        return;
    }
    let Some(ratio) = split_ratio_from_text(&left, &right, is_source) else {
        return;
    };
    if ratio < MIN_SPLIT_SIDE_RATIO || ratio > (1.0 - MIN_SPLIT_SIDE_RATIO) {
        return;
    }
    if out.iter().any(|(ol, or, _)| ol == &left && or == &right) {
        return;
    }
    out.push((left, right, ratio));
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

fn split_text_by_punctuation_candidates(text: &str) -> Vec<(String, String)> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    if chars.len() < 8 {
        return Vec::new();
    }
    let mid = chars.len() / 2;
    let mut candidates: Vec<usize> = Vec::new();
    for (idx, (_, ch)) in chars.iter().enumerate() {
        if matches!(*ch, '，' | ',' | '。' | '.' | '；' | ';' | '：' | ':' | '！' | '!' | '？' | '?')
        {
            candidates.push(idx);
        }
    }
    candidates.sort_by_key(|idx| {
        let delta = if *idx > mid { *idx - mid } else { mid - *idx };
        (delta, *idx)
    });
    let mut out = Vec::new();
    for idx in candidates.into_iter().take(MAX_SPLIT_OPTIONS_PER_SIDE) {
        let split_byte = chars
            .get(idx)
            .map(|(byte_idx, ch)| byte_idx + ch.len_utf8())
            .unwrap_or(text.len());
        let left = text[..split_byte].trim().to_string();
        let right = text[split_byte..].trim().to_string();
        if !left.is_empty() && !right.is_empty() {
            out.push((left, right));
        }
    }
    out
}

fn split_text_by_word_boundary_candidates(text: &str) -> Vec<(String, String)> {
    let words = text
        .split_whitespace()
        .filter(|w| !w.trim().is_empty())
        .map(|w| w.trim().to_string())
        .collect::<Vec<_>>();
    if words.len() < 8 {
        return Vec::new();
    }
    let mid = words.len() / 2;
    let min_left = 3usize;
    let min_right = 3usize;
    let mut candidates: Vec<(usize, usize)> = Vec::new();
    for idx in min_left..(words.len().saturating_sub(min_right)) {
        let left_last = normalize_word_for_boundary(&words[idx - 1]);
        let right_first = normalize_word_for_boundary(&words[idx]);
        if !is_safe_word_boundary(left_last.as_str(), right_first.as_str()) {
            continue;
        }
        let delta = if idx > mid { idx - mid } else { mid - idx };
        candidates.push((delta, idx));
    }
    candidates.sort_by_key(|(delta, idx)| (*delta, *idx));
    let mut out = Vec::new();
    for (_, split_idx) in candidates.into_iter().take(MAX_SPLIT_OPTIONS_PER_SIDE) {
        let left = words[..split_idx].join(" ").trim().to_string();
        let right = words[split_idx..].join(" ").trim().to_string();
        if !left.is_empty() && !right.is_empty() {
            out.push((left, right));
        }
    }
    out
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
