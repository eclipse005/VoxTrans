use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use voxtrans_core::subtitle::srt::{SrtCue, to_srt_from_cues};

use crate::services::task_log::TaskLogger;
use crate::services::translate::adapters::rig_node::{
    JsonResponseValidator, RigNodeClient, RigNodeConfig, RigNodeJsonTask,
};
use crate::services::translate::types::{TranslateSegment, TranslateTerminologyEntry};

const QUALITY_BATCH_SIZE: usize = 24;
const LAYOUT_BATCH_SIZE: usize = 20;
const MIN_QUALITY_CONFIDENCE: f64 = 0.72;
const MIN_LAYOUT_SPLIT_CONFIDENCE: f64 = 0.6;
const MAX_SPLIT_RATIO_DELTA: f64 = 0.35;

#[derive(Debug, Clone)]
pub struct QaAgentRequest {
    pub task_id: String,
    pub media_path: String,
    pub source_lang: String,
    pub target_lang: String,
    pub api_key: String,
    pub base_url: String,
    pub model: String,
    pub llm_concurrency: u32,
    pub source_max_words_per_segment: u32,
    pub target_reference_len: u32,
    pub terminology_entries: Vec<TranslateTerminologyEntry>,
    pub segments: Vec<TranslateSegment>,
    pub style_guidance: String,
    pub pass: String,
    pub prior_report: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct QaAgentResponse {
    pub segments: Vec<TranslateSegment>,
    pub source_srt: String,
    pub target_srt: String,
    pub bilingual_srt_source_first: String,
    pub bilingual_srt_target_first: String,
    pub report: Value,
    pub applied_changes: Vec<QaAppliedChange>,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct QaAppliedChange {
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
    pub before_segments: Vec<QaTextPair>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub after_segments: Vec<QaTextPair>,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct QaTextPair {
    pub source_text: String,
    pub translated_text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QaPassKind {
    Segment,
    Quality,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct QualityExtraction {
    #[serde(default)]
    items: Vec<QualityItem>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct QualityItem {
    index: usize,
    new_text: String,
    #[serde(default)]
    reason: String,
    #[serde(default)]
    confidence: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct LayoutDecisionExtraction {
    #[serde(default)]
    items: Vec<LayoutDecisionItem>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct LayoutDecisionItem {
    index: usize,
    #[serde(default)]
    should_split: bool,
    #[serde(default)]
    confidence: f64,
    #[serde(default)]
    reason: String,
}

#[derive(Debug, Clone)]
struct LayoutCandidate {
    index: usize,
    source_words: usize,
    target_chars: usize,
}

pub async fn run_qa_simple(request: QaAgentRequest) -> Result<QaAgentResponse, String> {
    if request.segments.is_empty() {
        return Err("qa empty segments".to_string());
    }

    let pass = normalize_pass(&request.pass);
    let mut segments = request.segments.clone();
    let mut applied_changes: Vec<QaAppliedChange> = Vec::new();

    match pass {
        QaPassKind::Segment => {
            optimize_layout(
                &request,
                &mut segments,
                request.source_max_words_per_segment,
                request.target_reference_len,
                &mut applied_changes,
            )
            .await?;
        }
        QaPassKind::Quality => {
            run_quality_refine(&request, &mut segments, &mut applied_changes).await?;
        }
    }

    let source_srt = build_srt(&segments, false);
    let target_srt = build_srt(&segments, true);
    let bilingual_srt_source_first = build_bilingual_srt(&segments, true);
    let bilingual_srt_target_first = build_bilingual_srt(&segments, false);

    let mut report = json!({
        "mode": "non_agent_two_pass",
        "pass": request.pass,
        "finalized": true,
        "appliedChangeTotal": applied_changes.len(),
        "segmentTotal": segments.len(),
    });
    if let Some(prior) = &request.prior_report {
        report["priorPassReport"] = prior.clone();
    }

    let logger = TaskLogger::main_with_media(request.task_id.clone(), request.media_path.clone());
    logger.event(
        "qa.completed",
        Some(&json!({
            "finalized": true,
            "finishReason": "finalize_qa",
            "segmentTotal": segments.len(),
            "appliedChangeTotal": applied_changes.len(),
            "pass": request.pass,
        })),
    );
    logger.event(
        "qa.effect",
        Some(&json!({
            "segmentTotalBefore": request.segments.len(),
            "segmentTotalAfter": segments.len(),
            "appliedChangeTotal": applied_changes.len(),
            "appliedChanges": applied_changes.clone(),
            "pass": request.pass,
        })),
    );

    Ok(QaAgentResponse {
        segments,
        source_srt,
        target_srt,
        bilingual_srt_source_first,
        bilingual_srt_target_first,
        report,
        applied_changes,
    })
}

fn normalize_pass(raw: &str) -> QaPassKind {
    let lower = raw.trim().to_lowercase();
    if lower == "segment" || lower == "pass1_segment" || lower == "segmentation" {
        QaPassKind::Segment
    } else {
        QaPassKind::Quality
    }
}

async fn optimize_layout(
    request: &QaAgentRequest,
    segments: &mut Vec<TranslateSegment>,
    source_max_words_per_segment: u32,
    target_reference_len: u32,
    applied_changes: &mut Vec<QaAppliedChange>,
) -> Result<(), String> {
    let candidates = collect_layout_candidates(
        segments,
        source_max_words_per_segment as usize,
        target_reference_len as usize,
    );
    if candidates.is_empty() {
        return Ok(());
    }

    let decisions = decide_layout_splits(request, segments, &candidates).await?;
    let candidate_indexes: HashSet<usize> = candidates.iter().map(|c| c.index).collect();
    let mut best_by_index: HashMap<usize, LayoutDecisionItem> = HashMap::new();
    for decision in decisions {
        if !decision.should_split || decision.confidence < MIN_LAYOUT_SPLIT_CONFIDENCE {
            continue;
        }
        if !candidate_indexes.contains(&decision.index) {
            continue;
        }
        match best_by_index.get(&decision.index) {
            Some(existing) if existing.confidence >= decision.confidence => {}
            _ => {
                best_by_index.insert(decision.index, decision);
            }
        }
    }
    let mut approved = best_by_index.into_values().collect::<Vec<_>>();
    approved.sort_by(|a, b| b.index.cmp(&a.index));

    for decision in approved {
        if decision.index >= segments.len() {
            continue;
        }
        let source_parts = split_text_near_middle(&segments[decision.index].source_text)
            .or_else(|| split_text_by_words_near_middle(&segments[decision.index].source_text));
        let target_parts = split_text_near_middle(&segments[decision.index].translated_text);
        if let (Some((s1, s2)), Some((t1, t2))) = (source_parts, target_parts) {
            if !s1.is_empty() && !s2.is_empty() && !t1.is_empty() && !t2.is_empty() {
                if !is_split_alignment_balanced(s1.as_str(), s2.as_str(), t1.as_str(), t2.as_str()) {
                    continue;
                }
                let mid = (segments[decision.index].start_ms + segments[decision.index].end_ms) / 2;
                let first = TranslateSegment {
                    start_ms: segments[decision.index].start_ms,
                    end_ms: mid.max(segments[decision.index].start_ms + 1),
                    source_text: s1.clone(),
                    translated_text: t1.clone(),
                };
                let second = TranslateSegment {
                    start_ms: first.end_ms,
                    end_ms: segments[decision.index].end_ms,
                    source_text: s2.clone(),
                    translated_text: t2.clone(),
                };
                let before_segments = vec![QaTextPair {
                    source_text: segments[decision.index].source_text.clone(),
                    translated_text: segments[decision.index].translated_text.clone(),
                }];
                let after_segments = vec![
                    QaTextPair {
                        source_text: first.source_text.clone(),
                        translated_text: first.translated_text.clone(),
                    },
                    QaTextPair {
                        source_text: second.source_text.clone(),
                        translated_text: second.translated_text.clone(),
                    },
                ];
                segments[decision.index] = first;
                segments.insert(decision.index + 1, second);
                applied_changes.push(QaAppliedChange {
                    kind: "split".to_string(),
                    index: decision.index,
                    source_text: String::new(),
                    before_translated_text: String::new(),
                    after_translated_text: String::new(),
                    reason: normalize_layout_reason(decision.reason.as_str()),
                    before_segments,
                    after_segments,
                });
            }
        }
    }
    Ok(())
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
                Some(LayoutCandidate {
                    index: idx,
                    source_words,
                    target_chars,
                })
            } else {
                None
            }
        })
        .collect()
}

async fn decide_layout_splits(
    request: &QaAgentRequest,
    segments: &[TranslateSegment],
    candidates: &[LayoutCandidate],
) -> Result<Vec<LayoutDecisionItem>, String> {
    if request.api_key.trim().is_empty()
        || request.base_url.trim().is_empty()
        || request.model.trim().is_empty()
    {
        return Ok(Vec::new());
    }
    let client = RigNodeClient::new(RigNodeConfig::new(
        request.base_url.clone(),
        request.api_key.clone(),
        request.model.clone(),
    ))?;
    let validator = JsonResponseValidator::with_required_keys(&["items"]);
    let system_prompt = build_layout_system_prompt();
    let mut tasks: Vec<RigNodeJsonTask> = Vec::new();
    let mut local_to_global_by_task: Vec<Vec<usize>> = Vec::new();
    let mut start = 0usize;
    let mut task_idx = 0usize;
    while start < candidates.len() {
        let end = (start + LAYOUT_BATCH_SIZE).min(candidates.len());
        let batch = &candidates[start..end];
        let user_prompt = build_layout_user_prompt(
            request.source_lang.as_str(),
            request.target_lang.as_str(),
            segments,
            batch,
            request.source_max_words_per_segment as usize,
            request.target_reference_len as usize,
        );
        tasks.push(RigNodeJsonTask {
            id: task_idx,
            system_prompt: system_prompt.clone(),
            user_prompt,
            response_validator: Some(validator.clone()),
        });
        local_to_global_by_task.push(batch.iter().map(|cand| cand.index).collect());
        task_idx += 1;
        start = end;
    }
    let concurrency = request.llm_concurrency.clamp(1, 16) as usize;
    let results = client
        .call_batch(
            &request.task_id,
            Some(&request.media_path),
            "qa_layout",
            tasks,
            concurrency,
        )
        .await;
    let mut out: Vec<LayoutDecisionItem> = Vec::new();
    for (task_id, result) in results {
        let json = match result {
            Ok(ok) => ok.json,
            Err(err) => return Err(format!("qa layout call failed: {}", err.message)),
        };
        let extracted = serde_json::from_value::<LayoutDecisionExtraction>(json)
            .map_err(|err| format!("qa layout parse failed: {err}"))?;
        let local_to_global = local_to_global_by_task
            .get(task_id)
            .ok_or_else(|| format!("qa layout returned unknown task id {task_id}"))?;
        for mut item in extracted.items {
            if item.index == 0 || item.index > local_to_global.len() {
                return Err(format!(
                    "qa layout returned invalid local index {} for task {}",
                    item.index,
                    task_id + 1
                ));
            }
            item.index = local_to_global[item.index - 1];
            out.push(item);
        }
    }
    Ok(out)
}

fn build_layout_system_prompt() -> String {
    "You are a subtitle layout reviewer. \
For each candidate item, decide whether it should be split into two subtitle segments. \
Split only when it clearly improves readability without changing meaning. \
Do not rewrite text and do not merge segments. \
Reason must focus on semantic/readability judgment, not numeric thresholds. \
Return strict JSON only: {\"items\":[{\"index\":1,\"shouldSplit\":true,\"confidence\":0.0,\"reason\":\"...\"}]}. \
Include every input candidate index in output items."
        .to_string()
}

fn build_layout_user_prompt(
    source_lang: &str,
    target_lang: &str,
    segments: &[TranslateSegment],
    candidates: &[LayoutCandidate],
    source_word_limit: usize,
    target_char_limit: usize,
) -> String {
    let items = candidates
        .iter()
        .enumerate()
        .map(|cand| {
            let local_index = cand.0 + 1;
            let cand = cand.1;
            let seg = &segments[cand.index];
            json!({
                "index": local_index,
                "sourceText": seg.source_text,
                "translatedText": seg.translated_text,
                "overLimitSignals": {
                    "sourceWordOverflow": cand.source_words > source_word_limit,
                    "targetCharOverflow": cand.target_chars > target_char_limit
                }
            })
        })
        .collect::<Vec<_>>();
    json!({
        "task": "subtitle_layout_split_decision",
        "sourceLang": source_lang,
        "targetLang": target_lang,
        "candidates": items,
        "requirements": [
            "Use local candidate index only (1..N in current batch), not global subtitle index",
            "Decide only split-or-keep for each candidate",
            "Prefer keep unless split benefit is clear",
            "Do not consider merge operations",
            "Do not rewrite text; this step is layout decision only",
            "Reason must describe semantic/readability motivation only",
            "Do not mention counts, limits, or formulas in reason",
            "confidence must be in [0,1]"
        ],
        "output": {
            "jsonOnly": true,
            "schema": {
                "items": [
                    {
                        "index": "number",
                        "shouldSplit": "boolean",
                        "confidence": "number(0..1)",
                        "reason": "string"
                    }
                ]
            }
        }
    }).to_string()
}

async fn run_quality_refine(
    request: &QaAgentRequest,
    segments: &mut [TranslateSegment],
    applied_changes: &mut Vec<QaAppliedChange>,
) -> Result<(), String> {
    if request.api_key.trim().is_empty()
        || request.base_url.trim().is_empty()
        || request.model.trim().is_empty()
    {
        return Ok(());
    }

    let client = RigNodeClient::new(RigNodeConfig::new(
        request.base_url.clone(),
        request.api_key.clone(),
        request.model.clone(),
    ))?;
    let validator = JsonResponseValidator::with_required_keys(&["items"]);
    let system_prompt = build_quality_system_prompt();
    let mut tasks: Vec<RigNodeJsonTask> = Vec::new();
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    let mut start = 0usize;
    let mut task_idx = 0usize;
    while start < segments.len() {
        let end = (start + QUALITY_BATCH_SIZE).min(segments.len());
        let batch = &segments[start..end];
        let user_prompt = build_quality_user_prompt(
            request.source_lang.as_str(),
            request.target_lang.as_str(),
            segments,
            batch,
            start,
            request.style_guidance.as_str(),
            &request.terminology_entries,
        );
        tasks.push(RigNodeJsonTask {
            id: task_idx,
            system_prompt: system_prompt.clone(),
            user_prompt,
            response_validator: Some(validator.clone()),
        });
        ranges.push((start, end));
        task_idx += 1;
        start = end;
    }

    let concurrency = request.llm_concurrency.clamp(1, 16) as usize;
    let results = client
        .call_batch(
            &request.task_id,
            Some(&request.media_path),
            "qa_quality",
            tasks,
            concurrency,
        )
        .await;

    for (batch_id, result) in results {
        let (start, end) = match ranges.get(batch_id).copied() {
            Some(v) => v,
            None => continue,
        };
        let batch_len = end.saturating_sub(start);
        let json = match result {
            Ok(ok) => ok.json,
            Err(err) => return Err(format!("qa quality call failed: {}", err.message)),
        };
        let extracted = serde_json::from_value::<QualityExtraction>(json)
            .map_err(|err| format!("qa quality parse failed: {err}"))?;

        for mut item in extracted.items {
            if item.index == 0 || item.index > batch_len {
                return Err(format!(
                    "qa quality returned invalid local index {} at batch {}",
                    item.index,
                    batch_id + 1
                ));
            }
            item.index = start + (item.index - 1);
            if item.confidence < MIN_QUALITY_CONFIDENCE {
                continue;
            }
            let new_text = item.new_text.trim();
            if new_text.is_empty() {
                continue;
            }
            let seg = &mut segments[item.index];
            let old_text = seg.translated_text.trim().to_string();
            if old_text == new_text {
                continue;
            }
            seg.translated_text = new_text.to_string();
            applied_changes.push(QaAppliedChange {
                kind: "update_translation".to_string(),
                index: item.index,
                source_text: seg.source_text.clone(),
                before_translated_text: old_text.clone(),
                after_translated_text: seg.translated_text.clone(),
                reason: if item.reason.trim().is_empty() {
                    "quality_refine".to_string()
                } else {
                    item.reason.trim().to_string()
                },
                before_segments: Vec::new(),
                after_segments: Vec::new(),
            });
        }
    }
    Ok(())
}

fn build_quality_system_prompt() -> String {
    "You are a subtitle quality refiner. \
Priority order: (1) faithfulness, (2) clarity, (3) style fit. \
Keep meaning, facts, logic, stance, and intent exactly faithful to source. \
Use natural target-language subtitle phrasing; avoid literal word-for-word calque. \
Keep output concise, readable, and audience-friendly; avoid over-literary wording. \
Do not change segmentation or index mapping. \
Preserve numbers, symbols, named entities, and terminology mappings. \
Only perform minimal necessary edits and do not modify lines that are already good enough. \
For each changed item, ensure: faithful_to_source=true, idiomatic_target_language=true, terminology_consistent=true, subtitle_concise=true. \
Return strict JSON only: {\"items\":[{\"index\":1,\"newText\":\"...\",\"reason\":\"semantic_or_fluency_refine\",\"confidence\":0.0}]}. \
Only include changed items. Reason must be short and generic; do not include counts or formulas."
        .to_string()
}

fn build_quality_user_prompt(
    source_lang: &str,
    target_lang: &str,
    all_segments: &[TranslateSegment],
    batch: &[TranslateSegment],
    start_index: usize,
    style_guidance: &str,
    terminology_entries: &[TranslateTerminologyEntry],
) -> String {
    let items = batch
        .iter()
        .enumerate()
        .map(|(offset, segment)| {
            let idx = start_index + offset;
            let local_index = offset + 1;
            let previous = if idx > 0 {
                all_segments
                    .get(idx - 1)
                    .map(|s| s.translated_text.as_str())
                    .unwrap_or("")
            } else {
                ""
            };
            let next = all_segments
                .get(idx + 1)
                .map(|s| s.translated_text.as_str())
                .unwrap_or("");
            json!({
                "index": local_index,
                "sourceText": segment.source_text,
                "translatedText": segment.translated_text,
                "context": {
                    "previous": previous,
                    "next": next
                }
            })
        })
        .collect::<Vec<_>>();

    let terms = terminology_entries
        .iter()
        .map(|entry| {
            json!({
                "source": entry.source,
                "target": entry.target,
                "note": entry.note
            })
        })
        .collect::<Vec<_>>();

    json!({
        "task": "subtitle_quality_refine",
        "sourceLang": source_lang,
        "targetLang": target_lang,
        "styleGuidance": style_guidance,
        "items": items,
        "terminology": terms,
        "requirements": [
            "Use local item index only (1..N in current batch), not global subtitle index",
            "Do not change segmentation structure or item index mapping",
            "Priority order: faithfulness > clarity > style fit",
            "Keep meaning, facts, logic, stance, and intent fully faithful to sourceText",
            "Use previous/next context only to avoid mistranslation",
            "Preserve numbers, symbols, and named entities",
            "Follow styleGuidance while keeping subtitle concise and readable",
            "Avoid literal word-for-word calques; prefer idiomatic domain phrasing",
            "Use provided terminology consistently",
            "Only do minimal necessary edits; keep good lines unchanged",
            "Reason must be short and generic; do not include counts/formulas",
            "Only return items that truly need fixes",
            "confidence must be in [0,1]"
        ],
        "output": {
            "jsonOnly": true,
            "schema": {
                "items": [
                    {
                        "index": "number",
                        "newText": "string",
                        "reason": "string",
                        "confidence": "number(0..1)"
                    }
                ]
            }
        }
    })
    .to_string()
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

fn is_split_alignment_balanced(source_left: &str, source_right: &str, target_left: &str, target_right: &str) -> bool {
    let source_left_units = split_unit_count(source_left);
    let source_right_units = split_unit_count(source_right);
    let target_left_units = target_char_count_metric(target_left);
    let target_right_units = target_char_count_metric(target_right);

    if source_left_units == 0
        || source_right_units == 0
        || target_left_units == 0
        || target_right_units == 0
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
    matches!(
        ch as u32,
        0x4E00..=0x9FFF
            | 0x3400..=0x4DBF
            | 0xF900..=0xFAFF
            | 0x3040..=0x309F
            | 0x30A0..=0x30FF
            | 0xAC00..=0xD7AF
    )
}

fn split_text_near_middle(text: &str) -> Option<(String, String)> {
    let chars = text.chars().collect::<Vec<_>>();
    if chars.len() < 16 {
        return None;
    }
    let delimiters = ['，', ',', '。', ';', '；', '!', '！', '?', '？', ':'];
    let mid = chars.len() / 2;
    let mut best_idx: Option<usize> = None;
    let mut best_dist = usize::MAX;
    for (idx, ch) in chars.iter().enumerate() {
        if !delimiters.contains(ch) {
            continue;
        }
        if idx < 4 || idx + 4 >= chars.len() {
            continue;
        }
        let dist = idx.abs_diff(mid);
        if dist < best_dist {
            best_dist = dist;
            best_idx = Some(idx + 1);
        }
    }
    let split_idx = best_idx?;
    let first = chars[..split_idx].iter().collect::<String>().trim().to_string();
    let second = chars[split_idx..].iter().collect::<String>().trim().to_string();
    if first.is_empty() || second.is_empty() {
        None
    } else {
        Some((first, second))
    }
}

fn split_text_by_words_near_middle(text: &str) -> Option<(String, String)> {
    let tokens = text
        .split_whitespace()
        .filter(|t| !t.trim().is_empty())
        .collect::<Vec<_>>();
    if tokens.len() < 8 {
        return None;
    }
    let mid = tokens.len() / 2;
    let min_side_tokens = 3usize;
    let start = min_side_tokens.max(mid.saturating_sub(3));
    let end = (tokens.len() - min_side_tokens).min(mid + 3);
    if start > end {
        return None;
    }

    let mut best_cut: Option<usize> = None;
    let mut best_score: usize = usize::MAX;
    for cut in start..=end {
        let left_last = normalize_word_for_boundary(tokens[cut - 1]);
        let right_first = normalize_word_for_boundary(tokens[cut]);
        if !is_safe_word_boundary(left_last.as_str(), right_first.as_str()) {
            continue;
        }
        let dist = cut.abs_diff(mid);
        if dist < best_score {
            best_score = dist;
            best_cut = Some(cut);
        }
    }
    let cut = best_cut?;

    let left = tokens[..cut].join(" ").trim().to_string();
    let right = tokens[cut..].join(" ").trim().to_string();
    if left.is_empty() || right.is_empty() {
        None
    } else {
        Some((left, right))
    }
}

fn normalize_word_for_boundary(token: &str) -> String {
    token
        .trim_matches(|c: char| !c.is_alphanumeric())
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
            | "and"
            | "are"
            | "as"
            | "at"
            | "be"
            | "been"
            | "being"
            | "but"
            | "by"
            | "for"
            | "from"
            | "if"
            | "in"
            | "is"
            | "of"
            | "on"
            | "or"
            | "so"
            | "that"
            | "the"
            | "to"
            | "was"
            | "were"
            | "with"
            | "without"
    )
}

fn normalize_layout_reason(raw: &str) -> String {
    let reason = raw.trim();
    if reason.is_empty() {
        return "semantic_readability_split".to_string();
    }
    let lower = reason.to_lowercase();
    let has_numeric_noise = lower.contains("超限")
        || lower.contains("字符")
        || lower.contains("字数")
        || lower.contains("词数")
        || lower.contains("limit")
        || lower.contains("count")
        || lower.contains('/')
        || lower.contains("ratio");
    if has_numeric_noise {
        "semantic_readability_split".to_string()
    } else {
        reason.to_string()
    }
}

fn build_srt(segments: &[TranslateSegment], translated: bool) -> String {
    let cues = segments
        .iter()
        .enumerate()
        .map(|(idx, segment)| SrtCue {
            index: idx + 1,
            start_ms: segment.start_ms,
            end_ms: segment.end_ms,
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
            let source = segment.source_text.trim();
            let target = segment.translated_text.trim();
            let text = if source_first {
                format!("{source}\n{target}")
            } else {
                format!("{target}\n{source}")
            };
            SrtCue {
                index: idx + 1,
                start_ms: segment.start_ms,
                end_ms: segment.end_ms,
                text,
            }
        })
        .collect::<Vec<_>>();
    to_srt_from_cues(&cues)
}
