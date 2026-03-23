use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use serde_json::{Value, json};
use voxtrans_core::subtitle::srt::{SrtCue, to_srt_from_cues};

use crate::services::llm::client::OpenAiCompatLlmClient;
use crate::services::llm::json_guard::JsonResponseValidator;
use crate::services::llm::port::{LlmCallContext, LlmConfig, LlmJsonTask, LlmPort, next_llm_request_id};
use crate::services::task_log::TaskLogger;
use crate::services::translate::prompt::{
    SegmentOptimizePromptCandidateInput, SegmentOptimizePromptInput,
    SegmentOptimizePromptSegmentInput,
    build_segment_optimize_system_prompt, build_segment_optimize_user_prompt,
};
use crate::services::translate::types::TranslateSegment;

const MAX_SPLIT_OPTIONS_PER_SIDE: usize = 4;
const MAX_PROPOSALS_PER_MODE: usize = 3;
const MAX_PROMPT_CANDIDATES: usize = 2;
const MAX_SPLIT_RATIO_DELTA: f64 = 0.28;
const MIN_SPLIT_SIDE_RATIO: f64 = 0.22;
const MIN_THREE_WAY_SEGMENT_RATIO: f64 = 0.16;
const MIN_SOURCE_SIDE_UNITS: usize = 3;
const MIN_TARGET_SIDE_UNITS: usize = 6;
pub const SEGMENT_OPTIMIZE_LAYOUT_VERSION: u32 = 3;

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
    source_segments: Vec<String>,
    target_segments: Vec<String>,
    confidence: f64,
    reason: String,
    segment_ratios: Option<Vec<f64>>,
}

#[derive(Debug, Clone)]
struct SplitDecisionGroup {
    index: usize,
    preferred_segment_count: usize,
    proposals: Vec<SplitProposal>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
struct SegmentOptimizeExtraction {
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
    #[serde(rename = "origin", alias = "sourceText")]
    #[serde(default)]
    source_text: String,
    #[serde(rename = "translation", alias = "translatedText")]
    #[serde(default)]
    translated_text: String,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
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
        "layoutVersion": SEGMENT_OPTIMIZE_LAYOUT_VERSION,
        "appliedChangeTotal": applied_changes.len(),
        "segmentTotal": segments.len(),
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
    let candidates = collect_layout_candidates(
        segments,
        source_max_words_per_segment as usize,
        target_reference_len as usize,
    );
    if candidates.is_empty() {
        return Ok(());
    }

    let mut decision_groups: Vec<SplitDecisionGroup> = Vec::new();
    for candidate in candidates {
        let index = candidate.index;
        if index >= segments.len() {
            continue;
        }
        let proposals = collect_split_proposals(index, &segments[index], candidate);
        if proposals.is_empty() {
            continue;
        }
        let preferred_segment_count = proposals[0].source_segments.len();
        decision_groups.push(SplitDecisionGroup {
            index,
            preferred_segment_count,
            proposals,
        });
    }
    if decision_groups.is_empty() {
        return Ok(());
    }

    let mut proposals =
        review_split_proposals(_request, llm_client.as_ref(), segments, decision_groups).await?;
    if proposals.is_empty() {
        return Ok(());
    }

    proposals.sort_by(|a, b| b.index.cmp(&a.index));
    for proposal in proposals {
        let index = proposal.index;
        if index >= segments.len() {
            continue;
        }
        let timings = split_segment_timing_multi(
            segments[index].start_ms,
            segments[index].end_ms,
            proposal.source_segments.len(),
            proposal.segment_ratios.as_deref(),
            proposal.timing_strategy,
        );
        let before_segments = vec![SegmentTextPair {
            source_text: segments[index].source_text.clone(),
            translated_text: segments[index].translated_text.clone(),
        }];
        let new_segments = proposal
            .source_segments
            .iter()
            .zip(proposal.target_segments.iter())
            .zip(timings.into_iter())
            .map(|((source_text, translated_text), (start_ms, end_ms))| TranslateSegment {
                start_ms,
                end_ms,
                source_text: source_text.clone(),
                translated_text: translated_text.clone(),
            })
            .collect::<Vec<_>>();
        let after_segments = new_segments
            .iter()
            .map(|segment| SegmentTextPair {
                source_text: segment.source_text.clone(),
                translated_text: segment.translated_text.clone(),
            })
            .collect::<Vec<_>>();
        segments.splice(index..=index, new_segments);
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

    let validator =
        JsonResponseValidator::with_required_keys(&["segments", "reason", "confidence"]);
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
                preferred_segment_count: group.preferred_segment_count,
                source_text: current.source_text,
                translated_text: current.translated_text,
                reference_candidates: group
                    .proposals
                    .iter()
                    .map(|proposal| SegmentOptimizePromptCandidateInput {
                        segments: proposal
                            .source_segments
                            .iter()
                            .zip(proposal.target_segments.iter())
                            .map(|(source_text, translated_text)| {
                                SegmentOptimizePromptSegmentInput {
                                    source_text: source_text.clone(),
                                    translated_text: translated_text.clone(),
                                }
                            })
                            .collect(),
                    })
                    .collect(),
            });
            LlmJsonTask {
                id: task_id,
                request_id: next_llm_request_id(),
                system_prompt: build_segment_optimize_system_prompt(),
                user_prompt,
                response_validator: Some(validator.clone()),
            }
        })
        .collect::<Vec<_>>();
    let llm_ids = tasks
        .iter()
        .map(|task| task.request_id.clone())
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
                Err(err) => {
                    return Err(format!(
                        "segment optimize llm decision invalid for index {} (llmId={}): {err}",
                        group.index,
                        ok.request_id
                    ));
                }
            },
            Err(err) => {
                let llm_id = llm_ids
                    .get(task_id)
                    .cloned()
                    .unwrap_or_else(next_llm_request_id);
                return Err(format!(
                    "segment optimize llm call failed for index {} (llmId={}): {}",
                    group.index,
                    llm_id,
                    err.message
                ));
            }
        }
    }
    Ok(reviewed)
}

fn parse_segment_optimize_decision(
    group: &SplitDecisionGroup,
    value: Value,
) -> Result<Option<SplitProposal>, String> {
    let extracted = serde_json::from_value::<SegmentOptimizeExtraction>(value)
        .map_err(|err| format!("segment optimize parse failed: {err}"))?;
    let segment_count = extracted.segments.len();
    let (source_segments, target_segments) = if matches!(segment_count, 1 | 2 | 3) {
        let source_segments = extracted
            .segments
            .iter()
            .map(|segment| segment.source_text.trim().to_string())
            .collect::<Vec<_>>();
        let target_segments = extracted
            .segments
            .iter()
            .map(|segment| segment.translated_text.trim().to_string())
            .collect::<Vec<_>>();
        if source_segments.iter().any(|segment| segment.is_empty())
            || target_segments.iter().any(|segment| segment.is_empty())
        {
            return Err("segment optimize returned empty source/target segment".to_string());
        } else {
            (source_segments, target_segments)
        }
    } else {
        return Err("segment optimize must return 1, 2, or 3 segments".to_string());
    };

    if segment_count == 1 {
        return Ok(None);
    }

    let inferred_mode = infer_layout_mode(&source_segments, &target_segments)?;

    validate_mode_output(inferred_mode, &source_segments, &target_segments)?;

    let recomputed_split_ratio = recompute_split_ratio(
        inferred_mode,
        infer_timing_strategy(inferred_mode),
        &source_segments,
        &target_segments,
    );

    Ok(Some(SplitProposal {
        candidate_id: format!("llm_{}", inferred_mode.as_str()),
        index: group.index,
        mode: inferred_mode,
        timing_strategy: infer_timing_strategy(inferred_mode),
        source_segments,
        target_segments,
        confidence: extracted.confidence.clamp(0.0, 1.0),
        reason: if extracted.reason.trim().is_empty() {
            format!("llm_result.{}", inferred_mode.as_str())
        } else {
            format!("llm_result.{}", normalize_reason_token(&extracted.reason))
        },
        segment_ratios: recomputed_split_ratio,
    }))
}

fn recompute_split_ratio(
    mode: LayoutMode,
    timing_strategy: TimingStrategy,
    source_segments: &[String],
    target_segments: &[String],
) -> Option<Vec<f64>> {
    match timing_strategy {
        TimingStrategy::ProportionalDuration => segment_ratios_from_texts(target_segments, false),
        TimingStrategy::SourceWordAlign => match mode {
            LayoutMode::DualSplit | LayoutMode::SourceOnlySplit => segment_ratios_from_texts(source_segments, true),
            LayoutMode::TargetOnlyAdjust => segment_ratios_from_texts(target_segments, false),
        },
    }
}

fn infer_layout_mode(
    source_segments: &[String],
    target_segments: &[String],
) -> Result<LayoutMode, String> {
    if source_segments.len() != target_segments.len() || !matches!(source_segments.len(), 2 | 3) {
        return Err("segment count must be 2 or 3 and source/target counts must match".to_string());
    }
    let source_split = !all_same(source_segments);
    let target_split = !all_same(target_segments);
    match (source_split, target_split) {
        (true, true) => Ok(LayoutMode::DualSplit),
        (true, false) => Ok(LayoutMode::SourceOnlySplit),
        (false, true) => Ok(LayoutMode::TargetOnlyAdjust),
        (false, false) => Err("at least one side must actually split".to_string()),
    }
}

fn infer_timing_strategy(mode: LayoutMode) -> TimingStrategy {
    match mode {
        LayoutMode::DualSplit | LayoutMode::SourceOnlySplit => TimingStrategy::SourceWordAlign,
        LayoutMode::TargetOnlyAdjust => TimingStrategy::ProportionalDuration,
    }
}

fn validate_mode_output(
    mode: LayoutMode,
    source_segments: &[String],
    target_segments: &[String],
) -> Result<(), String> {
    if source_segments.len() != target_segments.len() || !matches!(source_segments.len(), 2 | 3) {
        return Err("segment count must be 2 or 3 and source/target counts must match".to_string());
    }
    match mode {
        LayoutMode::DualSplit => {
            if all_same(source_segments) || all_same(target_segments) {
                return Err("dual_split requires both source and target to actually split".to_string());
            }
        }
        LayoutMode::SourceOnlySplit => {
            if all_same(source_segments) {
                return Err("source_only_split requires source to actually split".to_string());
            }
        }
        LayoutMode::TargetOnlyAdjust => {
            if all_same(target_segments) {
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
    let target_segment_count = decide_target_segment_count(
        segment,
        &candidate,
    );
    let source_options = if target_segment_count == 3 {
        let triple = split_options_three_way(segment.source_text.as_str(), true);
        if triple.is_empty() {
            split_options(segment.source_text.as_str(), true)
                .into_iter()
                .map(|(left, right, ratio)| (vec![left, right], vec![ratio, 1.0 - ratio]))
                .collect()
        } else {
            triple
        }
    } else {
        split_options(segment.source_text.as_str(), true)
            .into_iter()
            .map(|(left, right, ratio)| (vec![left, right], vec![ratio, 1.0 - ratio]))
            .collect()
    };
    let target_options = if target_segment_count == 3 {
        let triple = split_options_three_way(segment.translated_text.as_str(), false);
        if triple.is_empty() {
            split_options(segment.translated_text.as_str(), false)
                .into_iter()
                .map(|(left, right, ratio)| (vec![left, right], vec![ratio, 1.0 - ratio]))
                .collect()
        } else {
            triple
        }
    } else {
        split_options(segment.translated_text.as_str(), false)
            .into_iter()
            .map(|(left, right, ratio)| (vec![left, right], vec![ratio, 1.0 - ratio]))
            .collect()
    };

    if candidate.source_overlong {
        for (rank, (source_segments, target_segments, ratio_delta, segment_ratios)) in
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
                source_segments,
                target_segments,
                confidence: score_confidence(ratio_delta, true),
                reason: "source_overlong.dual_split".to_string(),
                segment_ratios: Some(segment_ratios),
            });
        }

        let translated = segment.translated_text.trim().to_string();
        if !translated.is_empty() {
            for (rank, (source_segments, source_ratios)) in source_options
                .iter()
                .cloned()
                .take(MAX_PROPOSALS_PER_MODE)
                .enumerate()
            {
                let target_segments = vec![translated.clone(); target_segment_count];
                proposals.push(SplitProposal {
                    candidate_id: format!("source_only_split_{rank}"),
                    index,
                    mode: LayoutMode::SourceOnlySplit,
                    timing_strategy: TimingStrategy::SourceWordAlign,
                    source_segments,
                    target_segments,
                    confidence: score_confidence(ratio_center_penalty(&source_ratios), false),
                    reason: "source_overlong.source_only_split".to_string(),
                    segment_ratios: Some(source_ratios),
                });
            }
        }
    }

    if candidate.target_overlong {
        for (rank, (source_segments, target_segments, ratio_delta, segment_ratios)) in
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
                source_segments,
                target_segments,
                confidence: score_confidence(ratio_delta, true),
                reason: "target_overlong.target_promoted_dual_split".to_string(),
                segment_ratios: Some(segment_ratios),
            });
        }

        let original_source = segment.source_text.trim().to_string();
        if !original_source.is_empty() {
            for (rank, (target_segments, target_ratios)) in target_options
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
                    source_segments: vec![original_source.clone(); target_segment_count],
                    target_segments,
                    confidence: score_confidence(ratio_center_penalty(&target_ratios), false),
                    reason: "target_overlong.target_only_adjust".to_string(),
                    segment_ratios: Some(target_ratios),
                });
            }
        }
    }

    select_prompt_candidates(dedupe_split_proposals(proposals))
}

fn dedupe_split_proposals(proposals: Vec<SplitProposal>) -> Vec<SplitProposal> {
    let mut out: Vec<SplitProposal> = Vec::new();
    for proposal in proposals {
        if out.iter().any(|existing| {
            existing.mode.as_str() == proposal.mode.as_str()
                && existing.source_segments == proposal.source_segments
                && existing.target_segments == proposal.target_segments
        }) {
            continue;
        }
        out.push(proposal);
    }
    out
}

fn select_prompt_candidates(mut proposals: Vec<SplitProposal>) -> Vec<SplitProposal> {
    if proposals.len() <= MAX_PROMPT_CANDIDATES {
        return proposals;
    }
    proposals.sort_by(|a, b| {
        b.confidence
            .partial_cmp(&a.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.candidate_id.cmp(&b.candidate_id))
    });
    proposals.truncate(MAX_PROMPT_CANDIDATES);
    proposals
}

fn decide_target_segment_count(
    segment: &TranslateSegment,
    candidate: &LayoutCandidate,
) -> usize {
    let source_units = source_word_count_metric(&segment.source_text);
    let target_units = target_char_count_metric(&segment.translated_text);
    let severe_source = candidate.source_overlong && source_units >= 28;
    let severe_target = candidate.target_overlong && target_units >= 42;
    if severe_source || severe_target {
        3
    } else {
        2
    }
}

fn all_same(segments: &[String]) -> bool {
    match segments.split_first() {
        None => true,
        Some((first, rest)) => rest.iter().all(|segment| segment == first),
    }
}

fn ratio_center_penalty(ratios: &[f64]) -> f64 {
    if ratios.is_empty() {
        return 0.5;
    }
    let target = 1.0 / ratios.len() as f64;
    ratios.iter().map(|ratio| (ratio - target).abs()).sum::<f64>()
}

fn segment_ratios_from_texts(segments: &[String], is_source: bool) -> Option<Vec<f64>> {
    if segments.is_empty() {
        return None;
    }
    let units = segments
        .iter()
        .map(|segment| {
            if is_source {
                source_word_count_metric(segment)
            } else {
                target_char_count_metric(segment)
            }
        })
        .collect::<Vec<_>>();
    let total = units.iter().sum::<usize>();
    if total == 0 {
        return None;
    }
    Some(
        units.into_iter()
            .map(|unit| unit as f64 / total as f64)
            .collect(),
    )
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
    source_options: &[(Vec<String>, Vec<f64>)],
    target_options: &[(Vec<String>, Vec<f64>)],
) -> Vec<(Vec<String>, Vec<String>, f64, Vec<f64>)> {
    rank_split_pairs(source_options, target_options, false)
}

fn rank_target_driven_dual_split_pairs(
    source_options: &[(Vec<String>, Vec<f64>)],
    target_options: &[(Vec<String>, Vec<f64>)],
) -> Vec<(Vec<String>, Vec<String>, f64, Vec<f64>)> {
    rank_split_pairs(source_options, target_options, true)
}

fn rank_split_pairs(
    source_options: &[(Vec<String>, Vec<f64>)],
    target_options: &[(Vec<String>, Vec<f64>)],
    allow_target_driven: bool,
) -> Vec<(Vec<String>, Vec<String>, f64, Vec<f64>)> {
    if source_options.is_empty() || target_options.is_empty() {
        return Vec::new();
    }
    let mut ranked: Vec<(Vec<String>, Vec<String>, f64, Vec<f64>, f64)> = Vec::new();
    for (source_segments, source_ratios) in source_options {
        for (target_segments, target_ratios) in target_options {
            if source_segments.len() != target_segments.len() {
                continue;
            }
            let balanced = if allow_target_driven {
                is_target_driven_split_balanced(source_segments, target_segments)
            } else {
                is_split_alignment_balanced(source_segments, target_segments)
            };
            if !balanced {
                continue;
            }
            let ratio_delta = source_ratios
                .iter()
                .zip(target_ratios.iter())
                .map(|(source_ratio, target_ratio)| (source_ratio - target_ratio).abs())
                .sum::<f64>();
            let center_penalty = ratio_center_penalty(source_ratios) + ratio_center_penalty(target_ratios);
            ranked.push((
                source_segments.clone(),
                target_segments.clone(),
                ratio_delta,
                source_ratios.clone(),
                center_penalty,
            ));
        }
    }
    ranked.sort_by(|a, b| {
        a.4.partial_cmp(&b.4)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal))
    });
    ranked
        .into_iter()
        .map(|(source_segments, target_segments, ratio_delta, source_ratios, _)| {
            (source_segments, target_segments, ratio_delta, source_ratios)
        })
        .collect()
}

fn is_target_driven_split_balanced(source_segments: &[String], target_segments: &[String]) -> bool {
    if source_segments.len() != target_segments.len() || !matches!(source_segments.len(), 2 | 3) {
        return false;
    }
    let source_units = source_segments
        .iter()
        .map(|segment| split_unit_count(segment))
        .collect::<Vec<_>>();
    let target_units = target_segments
        .iter()
        .map(|segment| target_char_count_metric(segment))
        .collect::<Vec<_>>();

    if source_units.iter().any(|unit| *unit < MIN_SOURCE_SIDE_UNITS)
        || target_units.iter().any(|unit| *unit < MIN_TARGET_SIDE_UNITS)
    {
        return false;
    }
    let total = source_units.iter().sum::<usize>() as f64;
    if total <= 0.0 {
        return false;
    }
    let min_ratio = if source_segments.len() == 3 {
        MIN_THREE_WAY_SEGMENT_RATIO
    } else {
        MIN_SPLIT_SIDE_RATIO
    };
    source_units
        .iter()
        .all(|unit| (*unit as f64 / total) >= min_ratio)
}

fn score_confidence(ratio_delta: f64, balanced: bool) -> f64 {
    let base = if balanced { 0.92 } else { 0.78 };
    (base - ratio_delta.min(0.5) * 0.6).clamp(0.0, 0.99)
}

fn split_segment_timing_multi(
    start_ms: u64,
    end_ms: u64,
    segment_count: usize,
    segment_ratios: Option<&[f64]>,
    timing_strategy: TimingStrategy,
) -> Vec<(u64, u64)> {
    let duration = end_ms.saturating_sub(start_ms).max(2);
    if segment_count <= 1 {
        return vec![(start_ms, end_ms)];
    }
    let ratios = match timing_strategy {
        TimingStrategy::SourceWordAlign => {
            segment_ratios
                .map(|ratios| normalize_segment_ratios(ratios))
                .unwrap_or_else(|| vec![1.0 / segment_count as f64; segment_count])
        }
        TimingStrategy::ProportionalDuration => segment_ratios
            .map(|ratios| normalize_segment_ratios(ratios))
            .unwrap_or_else(|| vec![1.0 / segment_count as f64; segment_count]),
    };
    let mut cuts = Vec::new();
    let mut acc = 0.0;
    for ratio in ratios.iter().take(segment_count.saturating_sub(1)) {
        acc += *ratio;
        let offset = ((duration as f64) * acc).round() as u64;
        cuts.push(start_ms + offset);
    }
    let mut windows = Vec::with_capacity(segment_count);
    let mut cursor = start_ms;
    for (idx, cut) in cuts.into_iter().enumerate() {
        let min_end = cursor + 1;
        let max_end = end_ms.saturating_sub((segment_count - idx - 1) as u64).max(min_end);
        let bounded_cut = cut.clamp(min_end, max_end);
        windows.push((cursor, bounded_cut));
        cursor = bounded_cut;
    }
    windows.push((cursor, end_ms.max(cursor + 1)));
    windows
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

fn split_options_three_way(text: &str, is_source: bool) -> Vec<(Vec<String>, Vec<f64>)> {
    let mut out: Vec<(Vec<String>, Vec<f64>)> = Vec::new();
    for (first_left, first_right, _) in split_options(text, is_source) {
        for (second_left, second_right, _) in split_options(&first_right, is_source) {
            let segments = vec![first_left.clone(), second_left, second_right];
            push_three_way_option(&mut out, segments, is_source);
        }
    }
    out
}

fn push_three_way_option(
    out: &mut Vec<(Vec<String>, Vec<f64>)>,
    segments: Vec<String>,
    is_source: bool,
) {
    if segments.len() != 3 || segments.iter().any(|segment| segment.trim().is_empty()) {
        return;
    }
    let Some(ratios) = segment_ratios_from_texts(&segments, is_source) else {
        return;
    };
    if ratios.iter().any(|ratio| *ratio < MIN_THREE_WAY_SEGMENT_RATIO) {
        return;
    }
    if out.iter().any(|(existing_segments, _)| *existing_segments == segments) {
        return;
    }
    out.push((segments, ratios));
}

fn normalize_segment_ratios(ratios: &[f64]) -> Vec<f64> {
    if ratios.is_empty() {
        return Vec::new();
    }
    let total = ratios.iter().sum::<f64>();
    if total <= 0.0 {
        return vec![1.0 / ratios.len() as f64; ratios.len()];
    }
    ratios.iter().map(|ratio| ratio / total).collect()
}

fn units_to_ratios(units: &[usize]) -> Option<Vec<f64>> {
    let total = units.iter().sum::<usize>();
    if total == 0 {
        return None;
    }
    Some(
        units.iter()
            .map(|unit| *unit as f64 / total as f64)
            .collect(),
    )
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

fn is_split_alignment_balanced(source_segments: &[String], target_segments: &[String]) -> bool {
    if source_segments.len() != target_segments.len() || !matches!(source_segments.len(), 2 | 3) {
        return false;
    }

    let source_units = source_segments
        .iter()
        .map(|segment| split_unit_count(segment))
        .collect::<Vec<_>>();
    let target_units = target_segments
        .iter()
        .map(|segment| target_char_count_metric(segment))
        .collect::<Vec<_>>();

    if source_units.iter().any(|unit| *unit < MIN_SOURCE_SIDE_UNITS)
        || target_units.iter().any(|unit| *unit < MIN_TARGET_SIDE_UNITS)
    {
        return false;
    }

    let Some(source_ratios) = units_to_ratios(&source_units) else {
        return false;
    };
    let Some(target_ratios) = units_to_ratios(&target_units) else {
        return false;
    };
    let min_ratio = if source_segments.len() == 3 {
        MIN_THREE_WAY_SEGMENT_RATIO
    } else {
        MIN_SPLIT_SIDE_RATIO
    };
    if source_ratios.iter().any(|ratio| *ratio < min_ratio)
        || target_ratios.iter().any(|ratio| *ratio < min_ratio)
    {
        return false;
    }

    source_ratios
        .iter()
        .zip(target_ratios.iter())
        .map(|(source_ratio, target_ratio)| (source_ratio - target_ratio).abs())
        .sum::<f64>()
        <= MAX_SPLIT_RATIO_DELTA
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
