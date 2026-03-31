use serde::Serialize;
use serde_json::{Value, json};
use voxtrans_core::subtitle::srt::{SrtCue, to_srt_from_cues};

use crate::services::llm::client::OpenAiCompatLlmClient;
use crate::services::llm::client::LlmSemanticValidationError;
use crate::services::llm::batch::run_indexed_concurrent;
use crate::services::llm::json_guard::JsonResponseValidator;
use crate::services::llm::port::{LlmCallContext, LlmConfig, LlmJsonTask, next_llm_request_id};
use crate::services::task_log::TaskLogger;
use crate::services::translate::prompt::{
    build_align_prompt, build_subtitle_split_prompt,
};
use crate::services::translate::types::TranslateSegment;

pub const SEGMENT_OPTIMIZE_LAYOUT_VERSION: u32 = 3;
const MAX_SPLIT_ROUNDS: usize = 3;

#[derive(Debug, Clone)]
pub struct SegmentOptimizeRequest {
    pub task_id: String,
    pub media_path: String,
    pub source_lang: String,
    pub target_lang: String,
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
    mode: LayoutMode,
    timing_strategy: TimingStrategy,
    preferred_segment_count: usize,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum LayoutMode {
    DualSplitAlign,
    SourceOnlySplit,
    TargetOnlySplit,
}

impl LayoutMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::DualSplitAlign => "dual_split_align",
            Self::SourceOnlySplit => "source_only_split",
            Self::TargetOnlySplit => "target_only_split",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum TimingStrategy {
    SourceWordAlign,
    TargetLengthProportional,
}

impl TimingStrategy {
    fn as_str(self) -> &'static str {
        match self {
            Self::SourceWordAlign => "source_word_align",
            Self::TargetLengthProportional => "target_length_proportional",
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
    for round in 0..MAX_SPLIT_ROUNDS {
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
            let mode = decide_layout_mode(&candidate);
            let preferred_segment_count = decide_target_segment_count(&segments[index], &candidate);
            decision_groups.push(SplitDecisionGroup {
                index,
                mode,
                timing_strategy: infer_timing_strategy(mode),
                preferred_segment_count,
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

        let mut round_applied = 0usize;
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
            // 由于 proposals 按 index 降序排序，从后往前处理可避免索引偏移
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
                reason: format!("{};round={}", proposal.reason, round + 1),
                before_segments,
                after_segments,
            });
            round_applied += 1;
        }

        if round_applied == 0 {
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
        return Ok(Vec::new());
    };
    if decision_groups.is_empty() {
        return Ok(Vec::new());
    }

    let split_validator = JsonResponseValidator::with_required_keys(&["keep_original", "parts"]);
    let align_validator = JsonResponseValidator::with_required_keys(&["align"]);
    let context = LlmCallContext {
        task_id: request.task_id.clone(),
        media_path: Some(request.media_path.clone()),
        phase: "segment_optimize".to_string(),
    };
    let source_lang = request.source_lang.clone();
    let target_lang = request.target_lang.clone();
    let segments_for_eval = segments.to_vec();
    let concurrency = request.llm_concurrency.clamp(1, 8) as usize;
    let tasks = decision_groups
        .iter()
        .enumerate()
        .map(|(task_id, _)| LlmJsonTask {
            id: task_id,
            request_id: next_llm_request_id(),
            user_prompt: String::new(),
            response_validator: None,
        })
        .collect::<Vec<_>>();
    let results = run_indexed_concurrent(
        tasks,
        concurrency,
        {
            let client = client.clone();
            let context = context.clone();
            let decision_groups = decision_groups.clone();
            let source_lang = source_lang.clone();
            let target_lang = target_lang.clone();
            let segments_for_eval = segments_for_eval.clone();
            let split_validator = split_validator.clone();
            let align_validator = align_validator.clone();
            move |task| {
                let client = client.clone();
                let context = context.clone();
                let decision_groups = decision_groups.clone();
                let source_lang = source_lang.clone();
                let target_lang = target_lang.clone();
                let segments_for_eval = segments_for_eval.clone();
                let split_validator = split_validator.clone();
                let align_validator = align_validator.clone();
                async move {
                    let Some(group) = decision_groups.get(task.id).cloned() else {
                        return Err("segment optimize internal error: missing decision group".to_string());
                    };
                    let current = segments_for_eval
                        .get(group.index)
                        .cloned()
                        .unwrap_or_else(|| TranslateSegment {
                            start_ms: 0,
                            end_ms: 0,
                            source_text: String::new(),
                            translated_text: String::new(),
                        });

                    let (split_language, split_text, split_word_limit, max_parts) = match group.mode {
                        LayoutMode::DualSplitAlign | LayoutMode::SourceOnlySplit => (
                            source_lang.as_str(),
                            current.source_text.as_str(),
                            20usize,
                            group.preferred_segment_count.clamp(2, 3),
                        ),
                        LayoutMode::TargetOnlySplit => (
                            target_lang.as_str(),
                            current.translated_text.as_str(),
                            18usize,
                            group.preferred_segment_count.clamp(2, 3),
                        ),
                    };
                    let split_prompt = build_subtitle_split_prompt(
                        split_language,
                        split_text,
                        group.preferred_segment_count,
                        split_word_limit,
                        max_parts,
                    );
                    let split_llm_id = format!("{}-split", task.request_id);
                    let split_result = client
                        .call_json_validated(
                            &context,
                            &split_llm_id,
                            &split_prompt,
                            Some(&split_validator.clone()),
                            |value| {
                                parse_split_parts(value, split_text, max_parts)
                                    .map_err(LlmSemanticValidationError::retryable)
                            },
                        )
                        .await;
                    let split_parts = match split_result {
                        Ok(validated) => validated.value,
                        Err(err) => {
                            return Err(format!(
                                "segment optimize split failed for index {} (llmId={}): {}",
                                group.index,
                                split_llm_id,
                                err.message
                            ))
                        }
                    };

                    if split_parts.len() < 2 {
                        return Ok((task.id, None));
                    }

                    match group.mode {
                        LayoutMode::SourceOnlySplit => {
                            let source_segments = split_parts.clone();
                            let target_segments =
                                vec![current.translated_text.clone(); source_segments.len()];
                            validate_mode_output(group.mode, &source_segments, &target_segments)?;
                            Ok((
                                task.id,
                                Some(SplitProposal {
                                    index: group.index,
                                    mode: group.mode,
                                    timing_strategy: group.timing_strategy,
                                    source_segments: source_segments.clone(),
                                    target_segments: target_segments.clone(),
                                    confidence: 0.72,
                                    reason: format!("llm_result.{}", group.mode.as_str()),
                                    segment_ratios: recompute_split_ratio(
                                        group.mode,
                                        group.timing_strategy,
                                        &source_segments,
                                        &target_segments,
                                    ),
                                }),
                            ))
                        }
                        LayoutMode::TargetOnlySplit => {
                            let source_segments = vec![current.source_text.clone(); split_parts.len()];
                            let target_segments = split_parts.clone();
                            validate_mode_output(group.mode, &source_segments, &target_segments)?;
                            Ok((
                                task.id,
                                Some(SplitProposal {
                                    index: group.index,
                                    mode: group.mode,
                                    timing_strategy: group.timing_strategy,
                                    source_segments: source_segments.clone(),
                                    target_segments: target_segments.clone(),
                                    confidence: 0.72,
                                    reason: format!("llm_result.{}", group.mode.as_str()),
                                    segment_ratios: recompute_split_ratio(
                                        group.mode,
                                        group.timing_strategy,
                                        &source_segments,
                                        &target_segments,
                                    ),
                                }),
                            ))
                        }
                        LayoutMode::DualSplitAlign => {
                            let src_part = split_parts.join("[br]");
                            let align_prompt = build_align_prompt(
                                &source_lang,
                                &target_lang,
                                &current.source_text,
                                &current.translated_text,
                                &src_part,
                                split_parts.len(),
                            );
                            let align_llm_id = format!("{}-align", task.request_id);
                            let align_result = client
                                .call_json_validated(
                                    &context,
                                    &align_llm_id,
                                    &align_prompt,
                                    Some(&align_validator.clone()),
                                    |value| {
                                        parse_align_target_parts(value, split_parts.len())
                                            .map_err(LlmSemanticValidationError::retryable)
                                    },
                                )
                                .await;
                            match align_result {
                                Ok(validated) => {
                                    let target_parts = validated.value;
                                    let source_segments = split_parts.clone();
                                    let target_segments = target_parts;
                                    validate_mode_output(group.mode, &source_segments, &target_segments)?;
                                    Ok((
                                        task.id,
                                        Some(SplitProposal {
                                            index: group.index,
                                            mode: group.mode,
                                            timing_strategy: group.timing_strategy,
                                            source_segments: source_segments.clone(),
                                            target_segments: target_segments.clone(),
                                            confidence: 0.72,
                                            reason: format!("llm_result.{}", group.mode.as_str()),
                                            segment_ratios: recompute_split_ratio(
                                                group.mode,
                                                group.timing_strategy,
                                                &source_segments,
                                                &target_segments,
                                            ),
                                        }),
                                    ))
                                }
                                Err(err) => Err(format!(
                                    "segment optimize align failed for index {} (llmId={}): {}",
                                    group.index,
                                    align_llm_id,
                                    err.message
                                )),
                            }
                        }
                    }
                }
            }
        },
        |message| message,
    )
    .await;

    let mut reviewed = Vec::with_capacity(decision_groups.len());
    for (index, result) in results {
        match result {
            Ok((_, parsed)) => {
                if let Some(parsed) = parsed {
                    reviewed.push(parsed);
                }
            }
            Err(err) => {
                // 单个分段失败不影响其他分段，记录警告后跳过
                eprintln!(
                    "[segment_optimize] skipped index {} due to error: {}",
                    index, err
                );
            }
        }
    }
    Ok(reviewed)
}

fn parse_split_parts(value: Value, original_text: &str, max_parts: usize) -> Result<Vec<String>, String> {
    let keep_original = value
        .get("keep_original")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mut parts = value
        .get("parts")
        .and_then(Value::as_array)
        .ok_or_else(|| "split parse failed: `parts` must be array".to_string())?
        .iter()
        .filter_map(Value::as_str)
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .collect::<Vec<_>>();
    if keep_original {
        return Ok(vec![original_text.trim().to_string()]);
    }
    if parts.is_empty() {
        parts.push(original_text.trim().to_string());
    }
    if parts.len() > max_parts {
        return Err(format!(
            "split parse failed: expected at most {} parts, got {}",
            max_parts,
            parts.len()
        ));
    }
    Ok(parts)
}

fn parse_align_target_parts(value: Value, expected_parts: usize) -> Result<Vec<String>, String> {
    let align = value
        .get("align")
        .and_then(Value::as_array)
        .ok_or_else(|| "align parse failed: `align` must be array".to_string())?;
    if align.len() != expected_parts {
        return Err(format!(
            "align parse failed: expected {} items, got {}",
            expected_parts,
            align.len()
        ));
    }
    let mut out = Vec::with_capacity(expected_parts);
    for i in 0..expected_parts {
        let key = format!("target_part_{}", i + 1);
        let item = align
            .get(i)
            .and_then(Value::as_object)
            .ok_or_else(|| format!("align parse failed: align[{}] must be object", i))?;
        let target = item
            .get(&key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .ok_or_else(|| format!("align parse failed: missing `{key}`"))?;
        out.push(target.to_string());
    }
    Ok(out)
}

fn recompute_split_ratio(
    mode: LayoutMode,
    timing_strategy: TimingStrategy,
    source_segments: &[String],
    target_segments: &[String],
) -> Option<Vec<f64>> {
    match timing_strategy {
        TimingStrategy::SourceWordAlign => match mode {
            LayoutMode::DualSplitAlign | LayoutMode::SourceOnlySplit => {
                segment_ratios_from_texts(source_segments, true)
            }
            LayoutMode::TargetOnlySplit => segment_ratios_from_texts(target_segments, false),
        },
        TimingStrategy::TargetLengthProportional => segment_ratios_from_texts(target_segments, false),
    }
}

fn decide_layout_mode(candidate: &LayoutCandidate) -> LayoutMode {
    match (candidate.source_overlong, candidate.target_overlong) {
        (true, true) => LayoutMode::DualSplitAlign,
        (true, false) => LayoutMode::SourceOnlySplit,
        (false, true) => LayoutMode::TargetOnlySplit,
        (false, false) => LayoutMode::SourceOnlySplit,
    }
}

fn infer_timing_strategy(mode: LayoutMode) -> TimingStrategy {
    match mode {
        LayoutMode::DualSplitAlign | LayoutMode::SourceOnlySplit => TimingStrategy::SourceWordAlign,
        LayoutMode::TargetOnlySplit => TimingStrategy::TargetLengthProportional,
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
        LayoutMode::DualSplitAlign => {
            if all_same(source_segments) || all_same(target_segments) {
                return Err("dual_split_align requires both source and target to actually split".to_string());
            }
        }
        LayoutMode::SourceOnlySplit => {
            if all_same(source_segments) {
                return Err("source_only_split requires source to actually split".to_string());
            }
        }
        LayoutMode::TargetOnlySplit => {
            if all_same(target_segments) {
                return Err("target_only_split requires target to actually split".to_string());
            }
        }
    }
    Ok(())
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

fn split_segment_timing_multi(
    start_ms: u64,
    end_ms: u64,
    segment_count: usize,
    segment_ratios: Option<&[f64]>,
) -> Vec<(u64, u64)> {
    let duration = end_ms.saturating_sub(start_ms).max(2);
    if segment_count <= 1 {
        return vec![(start_ms, end_ms)];
    }
    let ratios = segment_ratios
        .map(|ratios| normalize_segment_ratios(ratios))
        .unwrap_or_else(|| vec![1.0 / segment_count as f64; segment_count]);
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

fn source_word_count_metric(text: &str) -> usize {
    text.split_whitespace()
        .map(trim_token_punctuation)
        .filter(|t| !t.is_empty())
        .filter(|t| t.chars().any(|ch| ch.is_alphabetic()))
        .count()
}

fn target_char_count_metric(text: &str) -> usize {
    let mut count = 0usize;
    let mut in_latin_run = false;

    for ch in text.chars() {
        if ch.is_whitespace() {
            in_latin_run = false;
            continue;
        }
        if is_cjk(ch) {
            count += 1;
            in_latin_run = false;
            continue;
        }
        if ch.is_ascii_alphanumeric() {
            if !in_latin_run {
                count += 1;
                in_latin_run = true;
            }
            continue;
        }
        in_latin_run = false;
    }

    count
}

fn trim_token_punctuation(token: &str) -> &str {
    token.trim_matches(|c: char| !c.is_alphanumeric())
}

fn is_cjk(ch: char) -> bool {
    matches!(ch as u32, 0x4E00..=0x9FFF | 0x3400..=0x4DBF | 0xF900..=0xFAFF)
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
