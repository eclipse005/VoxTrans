use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use voxtrans_core::subtitle::srt::{SrtCue, to_srt_from_cues};

use crate::services::task_log::TaskLogger;

use super::types::{TranslateSegment, TranslateTerminologyEntry};

const MAX_TURNS: usize = 12;
const MAX_TOOL_CALLS_PER_TURN: usize = 4;

#[derive(Debug, Clone)]
pub struct QaAgentRequest {
    pub task_id: String,
    pub media_path: String,
    pub source_lang: String,
    pub target_lang: String,
    pub api_key: String,
    pub base_url: String,
    pub model: String,
    pub target_reference_len: u32,
    pub terminology_entries: Vec<TranslateTerminologyEntry>,
    pub segments: Vec<TranslateSegment>,
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
    pub before_source: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub before_target: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub after_source: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub after_target: String,
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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct OverlengthIssue {
    index: usize,
    source_len: usize,
    target_len: usize,
    reference_len: u32,
    over_by_source: i64,
    over_by_target: i64,
    severity: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ConsistencyIssue {
    issue_id: String,
    issue_type: String,
    severity: String,
    indices: Vec<usize>,
    evidence: Value,
    suggested_fix: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlanSegmentOpsArgs {
    #[serde(default)]
    indices: Vec<usize>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApplySegmentOpsArgs {
    #[serde(default)]
    ops: Vec<SegmentOp>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(tag = "kind")]
enum SegmentOp {
    Split {
        index: usize,
        segments: Vec<SegmentTextPair>,
        #[serde(default)]
        reason: String,
    },
    Merge {
        start_index: usize,
        end_index: usize,
        source_text: String,
        translated_text: String,
        #[serde(default)]
        reason: String,
    },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SegmentTextPair {
    source_text: String,
    translated_text: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateTranslationArgs {
    index: usize,
    new_text: String,
    #[serde(default)]
    reason: String,
}

#[derive(Debug, Clone)]
struct QaState {
    original_segments: Vec<TranslateSegment>,
    working_segments: Vec<TranslateSegment>,
    target_reference_len: u32,
    source_lang: String,
    target_lang: String,
    terminology_entries: Vec<TranslateTerminologyEntry>,
    applied_changes: Vec<QaAppliedChange>,
}

pub async fn run_qa_agent(request: QaAgentRequest) -> Result<QaAgentResponse, String> {
    if request.api_key.trim().is_empty()
        || request.base_url.trim().is_empty()
        || request.model.trim().is_empty()
    {
        return Err("qa missing llm config".to_string());
    }
    if request.segments.is_empty() {
        return Err("qa empty segments".to_string());
    }

    let logger = TaskLogger::main_with_media(request.task_id.clone(), request.media_path.clone());
    let llm_logger = TaskLogger::llm_with_media(request.task_id.clone(), request.media_path.clone());
    let mut state = QaState {
        original_segments: request.segments.clone(),
        working_segments: request.segments.clone(),
        target_reference_len: request.target_reference_len.clamp(8, 80),
        source_lang: request.source_lang.clone(),
        target_lang: request.target_lang.clone(),
        terminology_entries: request.terminology_entries.clone(),
        applied_changes: Vec::new(),
    };

    let base = request.base_url.trim_end_matches('/');
    let url = format!("{base}/chat/completions");
    let client = reqwest::Client::new();

    let pass = normalize_pass(&request.pass);
    let mut messages = vec![
        json!({
            "role": "system",
            "content": build_qa_system_prompt(pass)
        }),
        json!({
            "role": "user",
            "content": build_qa_user_prompt(&request, &state, pass)
        }),
    ];

    let mut finalized = false;
    let mut finish_reason = String::new();
    let tools = build_tools(pass);

    for turn in 1..=MAX_TURNS {
        let payload = json!({
            "model": request.model,
            "temperature": 0.2,
            "messages": messages.clone(),
            "tools": tools.clone(),
            "tool_choice": "auto",
        });
        logger.event(
            "qa.turn.request",
            Some(&json!({
                "turn": turn,
                "model": request.model,
                "pass": request.pass,
            })),
        );
        llm_logger.event(
            "qa.turn.request",
            Some(&json!({
                "turn": turn,
                "url": url,
                "payload": payload,
                "pass": request.pass,
            })),
        );

        let response = client
            .post(&url)
            .bearer_auth(&request.api_key)
            .json(&payload)
            .send()
            .await
            .map_err(|err| format!("qa chat request failed: {err}"))?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let body = response.text().await.unwrap_or_default();
            return Err(format!("qa chat http {status}: {body}"));
        }
        let value = response
            .json::<Value>()
            .await
            .map_err(|err| format!("qa chat parse failed: {err}"))?;
        let choice = value
            .get("choices")
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .cloned()
            .ok_or_else(|| "qa missing choices".to_string())?;
        let message = choice
            .get("message")
            .cloned()
            .ok_or_else(|| "qa missing message".to_string())?;
        let tool_calls = message
            .get("tool_calls")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        finish_reason = choice
            .get("finish_reason")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();

        messages.push(message.clone());
        llm_logger.event(
            "qa.turn.response",
            Some(&json!({
                "turn": turn,
                "response": value,
                "finishReason": finish_reason,
            })),
        );

        if tool_calls.is_empty() {
            logger.event(
                "qa.turn.no_tool_calls",
                Some(&json!({
                    "turn": turn,
                    "finishReason": finish_reason,
                })),
            );
            break;
        }

        for tool_call in tool_calls.into_iter().take(MAX_TOOL_CALLS_PER_TURN) {
            let call_id = tool_call
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let name = tool_call
                .get("function")
                .and_then(|v| v.get("name"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let raw_args = tool_call
                .get("function")
                .and_then(|v| v.get("arguments"))
                .and_then(|v| v.as_str())
                .unwrap_or("{}");
            let args = serde_json::from_str::<Value>(raw_args).unwrap_or_else(|_| json!({}));
            llm_logger.event(
                "qa.tool.call",
                Some(&json!({
                    "turn": turn,
                    "tool": name,
                    "callId": call_id,
                    "args": args,
                })),
            );
            let result = execute_tool(&name, args, &mut state, &mut finalized);
            logger.event(
                "qa.tool.executed",
                Some(&json!({
                    "turn": turn,
                    "tool": name,
                    "callId": call_id,
                    "pass": request.pass,
                })),
            );
            if result
                .get("ok")
                .and_then(|v| v.as_bool())
                .is_some_and(|ok| !ok)
            {
                logger.event(
                    "qa.tool.error",
                    Some(&json!({
                        "turn": turn,
                        "tool": name,
                        "callId": call_id,
                        "error": result.get("error").cloned().unwrap_or(Value::Null),
                        "pass": request.pass,
                    })),
                );
            }
            llm_logger.event(
                "qa.tool.result",
                Some(&json!({
                    "turn": turn,
                    "tool": name,
                    "callId": call_id,
                    "result": result,
                    "pass": request.pass,
                })),
            );
            messages.push(json!({
                "role": "tool",
                "tool_call_id": call_id,
                "content": result.to_string()
            }));
        }

        if finalized {
            break;
        }
    }

    let source_srt = build_srt(&state.working_segments, false);
    let target_srt = build_srt(&state.working_segments, true);
    let bilingual_srt_source_first = build_bilingual_srt(&state.working_segments, true);
    let bilingual_srt_target_first = build_bilingual_srt(&state.working_segments, false);
    let report = build_report(&state, finalized, &finish_reason);

    logger.event(
        "qa.completed",
        Some(&json!({
            "finalized": finalized,
            "finishReason": finish_reason,
            "segmentTotal": state.working_segments.len(),
            "appliedChangeTotal": state.applied_changes.len(),
            "pass": request.pass,
        })),
    );
    logger.event(
        "qa.effect",
        Some(&json!({
            "segmentTotalBefore": state.original_segments.len(),
            "segmentTotalAfter": state.working_segments.len(),
            "appliedChangeTotal": state.applied_changes.len(),
            "impact": check_change_impact(&state),
            "appliedChanges": state.applied_changes,
            "pass": request.pass,
        })),
    );
    llm_logger.event(
        "qa.completed",
        Some(&json!({
            "finalized": finalized,
            "finishReason": finish_reason,
            "segmentTotal": state.working_segments.len(),
            "appliedChangeTotal": state.applied_changes.len(),
            "report": report,
            "pass": request.pass,
        })),
    );

    Ok(QaAgentResponse {
        segments: state.working_segments,
        source_srt,
        target_srt,
        bilingual_srt_source_first,
        bilingual_srt_target_first,
        report,
        applied_changes: state.applied_changes,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QaPassKind {
    Segment,
    Quality,
}

fn normalize_pass(raw: &str) -> QaPassKind {
    let lower = raw.trim().to_lowercase();
    if lower == "segment" || lower == "pass1_segment" || lower == "segmentation" {
        QaPassKind::Segment
    } else {
        QaPassKind::Quality
    }
}

fn build_qa_system_prompt(pass: QaPassKind) -> String {
    match pass {
        QaPassKind::Segment => "You are pass1 subtitle segment QA agent. \
Only optimize segmentation and readability boundaries. \
Do not rewrite translated wording unless absolutely required to keep segment alignment. \
Use split conservatively and keep natural semantic boundaries. \
Always call finalize_qa when done."
            .to_string(),
        QaPassKind::Quality => "You are pass2 subtitle quality QA agent. \
Do not change segmentation structure in this pass. \
Focus on terminology consistency, translation accuracy, and natural phrasing. \
Use update_translation for high-confidence fixes and preserve source meaning. \
Always call finalize_qa when done."
            .to_string(),
    }
}

fn build_qa_user_prompt(request: &QaAgentRequest, state: &QaState, pass: QaPassKind) -> String {
    let segments = state
        .working_segments
        .iter()
        .enumerate()
        .map(|(idx, segment)| {
            json!({
                "index": idx,
                "sourceText": segment.source_text,
                "translatedText": segment.translated_text
            })
        })
        .collect::<Vec<_>>();
    let mut payload = json!({
        "task": "subtitle_qa",
        "qaPass": request.pass,
        "sourceLang": request.source_lang,
        "targetLang": request.target_lang,
        "targetReferenceLen": state.target_reference_len,
        "segments": segments,
    });
    if let Some(report) = &request.prior_report {
        payload["priorPassReport"] = report.clone();
    }
    payload["rules"] = match pass {
        QaPassKind::Segment => json!([
            "Only perform segmentation optimization in this pass",
            "Keep split count low and prefer natural sentence boundaries",
            "Do not break semantic units and do not create tiny fragments",
            "Split result must contain exactly 2 aligned segments",
            "Do not call update_translation unless absolutely necessary",
            "Call finalize_qa once segmentation is stable"
        ]),
        QaPassKind::Quality => json!([
            "Do not change segmentation structure in this pass",
            "Run consistency checks and apply translation fixes where beneficial",
            "Prioritize terminology consistency and translation accuracy",
            "If quality is already good, keep text unchanged and finalize",
            "Call finalize_qa once no more translation fixes are needed"
        ]),
    };
    payload.to_string()
}

fn build_tools(pass: QaPassKind) -> Vec<Value> {
    let mut tools = Vec::new();
    if pass == QaPassKind::Segment {
        tools.push(tool_def(
            "list_overlength_segments",
            "List segments whose translated text length is over target reference length.",
            json!({ "type": "object", "properties": {} }),
        ));
        tools.push(tool_def(
            "plan_segment_ops",
            "Propose split/merge operations for given indices.",
            json!({
                "type": "object",
                "properties": {
                    "indices": { "type": "array", "items": { "type": "integer" } }
                },
                "required": ["indices"]
            }),
        ));
        tools.push(tool_def(
            "apply_segment_ops",
            "Apply segment split/merge operations.",
            json!({
                "type": "object",
                "properties": {
                    "ops": { "type": "array" }
                },
                "required": ["ops"]
            }),
        ));
    } else {
        tools.push(tool_def(
            "check_translation_consistency",
            "Check terminology/phrase/number consistency issues.",
            json!({ "type": "object", "properties": {} }),
        ));
        tools.push(tool_def(
            "update_translation",
            "Update translated text at index.",
            json!({
                "type": "object",
                "properties": {
                    "index": { "type": "integer" },
                    "newText": { "type": "string" },
                    "reason": { "type": "string" }
                },
                "required": ["index", "newText"]
            }),
        ));
    }
    tools.push(tool_def(
        "check_change_impact",
        "Summarize changes and risks between original and current subtitles.",
        json!({ "type": "object", "properties": {} }),
    ));
    tools.push(tool_def(
        "finalize_qa",
        "Finish QA and return summary.",
        json!({ "type": "object", "properties": {} }),
    ));
    tools
}

fn tool_def(name: &str, description: &str, parameters: Value) -> Value {
    json!({
        "type": "function",
        "function": {
            "name": name,
            "description": description,
            "parameters": parameters
        }
    })
}

fn execute_tool(
    name: &str,
    args: Value,
    state: &mut QaState,
    finalized: &mut bool,
) -> Value {
    match name {
        "list_overlength_segments" => {
            json!({ "ok": true, "issues": list_overlength_segments(
                &state.working_segments,
                state.target_reference_len,
                &state.source_lang,
                &state.target_lang
            ) })
        }
        "check_translation_consistency" => {
            json!({ "ok": true, "issues": check_translation_consistency(&state.working_segments, &state.terminology_entries) })
        }
        "plan_segment_ops" => {
            match serde_json::from_value::<PlanSegmentOpsArgs>(args) {
                Ok(parsed) => json!({ "ok": true, "recommendedOps": plan_segment_ops(
                    &state.working_segments,
                    parsed.indices,
                    state.target_reference_len,
                    &state.target_lang
                ) }),
                Err(err) => json!({
                    "ok": false,
                    "retryable": true,
                    "error": format!("invalid plan_segment_ops args: {err}")
                }),
            }
        }
        "apply_segment_ops" => {
            match serde_json::from_value::<ApplySegmentOpsArgs>(args) {
                Ok(parsed) => match apply_segment_ops(state, parsed.ops) {
                    Ok(()) => json!({ "ok": true, "segmentTotal": state.working_segments.len() }),
                    Err(err) => json!({
                        "ok": false,
                        "retryable": true,
                        "error": format!("apply_segment_ops rejected: {err}")
                    }),
                },
                Err(err) => json!({
                    "ok": false,
                    "retryable": true,
                    "error": format!("invalid apply_segment_ops args: {err}")
                }),
            }
        }
        "update_translation" => {
            match serde_json::from_value::<UpdateTranslationArgs>(args) {
                Ok(parsed) => match update_translation(state, parsed) {
                    Ok(()) => json!({ "ok": true }),
                    Err(err) => json!({
                        "ok": false,
                        "retryable": true,
                        "error": format!("update_translation rejected: {err}")
                    }),
                },
                Err(err) => json!({
                    "ok": false,
                    "retryable": true,
                    "error": format!("invalid update_translation args: {err}")
                }),
            }
        }
        "check_change_impact" => {
            let mut v = check_change_impact(state);
            if let Some(obj) = v.as_object_mut() {
                obj.insert("ok".to_string(), Value::Bool(true));
            }
            v
        }
        "finalize_qa" => {
            *finalized = true;
            json!({ "ok": true, "summary": "qa finalized" })
        }
        _ => json!({
            "ok": false,
            "retryable": false,
            "error": format!("unknown tool {name}")
        }),
    }
}

fn list_overlength_segments(
    segments: &[TranslateSegment],
    target_reference_len: u32,
    source_lang: &str,
    target_lang: &str,
) -> Vec<OverlengthIssue> {
    segments
        .iter()
        .enumerate()
        .filter_map(|(index, segment)| {
            let source_len = source_length_metric(&segment.source_text, source_lang);
            let target_len = target_length_metric(&segment.translated_text, target_lang);
            if target_len <= target_reference_len as usize {
                return None;
            }
            let over_by_source = 0i64;
            let over_by_target = target_len as i64 - target_reference_len as i64;
            let max_over = over_by_target;
            let severity = if max_over > 20 {
                "high"
            } else if max_over > 8 {
                "medium"
            } else {
                "low"
            };
            Some(OverlengthIssue {
                index,
                source_len,
                target_len,
                reference_len: target_reference_len,
                over_by_source,
                over_by_target,
                severity: severity.to_string(),
            })
        })
        .collect()
}

fn check_translation_consistency(
    segments: &[TranslateSegment],
    terminology_entries: &[TranslateTerminologyEntry],
) -> Vec<ConsistencyIssue> {
    let mut issues: Vec<ConsistencyIssue> = Vec::new();

    for (term_idx, term) in terminology_entries.iter().enumerate() {
        let source = term.source.trim().to_lowercase();
        let target = term.target.trim().to_lowercase();
        if source.is_empty() || target.is_empty() {
            continue;
        }
        let mut source_hit_indices = Vec::new();
        let mut target_miss_indices = Vec::new();
        for (idx, segment) in segments.iter().enumerate() {
            let src = segment.source_text.to_lowercase();
            if src.contains(&source) {
                source_hit_indices.push(idx);
                if !segment.translated_text.to_lowercase().contains(&target) {
                    target_miss_indices.push(idx);
                }
            }
        }
        if !target_miss_indices.is_empty() {
            issues.push(ConsistencyIssue {
                issue_id: format!("term-mismatch-{term_idx}"),
                issue_type: "term_mismatch".to_string(),
                severity: "high".to_string(),
                indices: target_miss_indices.clone(),
                evidence: json!({
                    "term": { "source": term.source, "target": term.target },
                    "sourceHitIndices": source_hit_indices,
                }),
                suggested_fix: format!("Ensure '{}' is translated as '{}'", term.source, term.target),
            });
        }
    }

    let mut source_to_targets: HashMap<String, HashSet<String>> = HashMap::new();
    let mut source_to_indices: HashMap<String, Vec<usize>> = HashMap::new();
    for (idx, segment) in segments.iter().enumerate() {
        let source_key = segment.source_text.trim().to_lowercase();
        let target_key = segment.translated_text.trim().to_lowercase();
        if source_key.is_empty() || target_key.is_empty() {
            continue;
        }
        source_to_targets
            .entry(source_key.clone())
            .or_default()
            .insert(target_key);
        source_to_indices.entry(source_key).or_default().push(idx);
    }
    let mut conflict_seq = 0usize;
    for (source, targets) in source_to_targets {
        if targets.len() < 2 {
            continue;
        }
        conflict_seq += 1;
        issues.push(ConsistencyIssue {
            issue_id: format!("phrase-conflict-{conflict_seq}"),
            issue_type: "phrase_inconsistent".to_string(),
            severity: "medium".to_string(),
            indices: source_to_indices.get(&source).cloned().unwrap_or_default(),
            evidence: json!({
                "source": source,
                "targetVariants": targets.into_iter().collect::<Vec<_>>(),
            }),
            suggested_fix: "Use one consistent translation for repeated source phrase".to_string(),
        });
    }

    let number_inconsistent = detect_numeric_inconsistency(segments);
    issues.extend(number_inconsistent);
    issues
}

fn detect_numeric_inconsistency(segments: &[TranslateSegment]) -> Vec<ConsistencyIssue> {
    let mut issues = Vec::new();
    for (idx, segment) in segments.iter().enumerate() {
        let source_has_number = segment.source_text.chars().any(|ch| ch.is_ascii_digit());
        if !source_has_number {
            continue;
        }
        let target_has_number = segment.translated_text.chars().any(|ch| ch.is_ascii_digit());
        if target_has_number {
            continue;
        }
        issues.push(ConsistencyIssue {
            issue_id: format!("number-missing-{idx}"),
            issue_type: "number_format_inconsistent".to_string(),
            severity: "medium".to_string(),
            indices: vec![idx],
            evidence: json!({
                "sourceText": segment.source_text,
                "translatedText": segment.translated_text,
            }),
            suggested_fix: "Ensure numeric information is preserved in translation".to_string(),
        });
    }
    issues
}

fn plan_segment_ops(
    segments: &[TranslateSegment],
    indices: Vec<usize>,
    target_reference_len: u32,
    target_lang: &str,
) -> Vec<Value> {
    let mut out = Vec::new();
    for index in indices {
        let Some(segment) = segments.get(index) else {
            continue;
        };
        let target_len = target_length_metric(&segment.translated_text, target_lang);
        if target_len <= target_reference_len as usize {
            continue;
        }
        out.push(json!({
            "kind": "split",
            "index": index,
            "reason": "overlength",
            "segments": [
                {
                    "sourceText": "",
                    "translatedText": ""
                },
                {
                    "sourceText": "",
                    "translatedText": ""
                }
            ],
            "instruction": "Fill segments[0..1] with exactly 2 aligned source/translated pairs before apply_segment_ops."
        }));
    }
    out
}

fn apply_segment_ops(state: &mut QaState, ops: Vec<SegmentOp>) -> Result<(), String> {
    let mut ops = ops;
    // Apply from high index to low index to avoid index drift after splice.
    ops.sort_by(|a, b| segment_op_sort_key(b).cmp(&segment_op_sort_key(a)));
    for op in ops {
        match op {
            SegmentOp::Split {
                index,
                segments,
                reason,
            } => {
                if index >= state.working_segments.len() {
                    return Err(format!("split index out of range: {index}"));
                }
                if segments.len() != 2 {
                    return Err("split requires exactly 2 segments".to_string());
                }
                let old = state.working_segments[index].clone();
                let mut new_parts_preview: Vec<TranslateSegment> = Vec::new();
                let mut new_parts: Vec<TranslateSegment> = Vec::new();
                for part in &segments {
                    let source_text = part.source_text.trim().to_string();
                    let translated_text = part.translated_text.trim().to_string();
                    if source_text.is_empty() && translated_text.is_empty() {
                        continue;
                    }
                    new_parts_preview.push(TranslateSegment {
                        start_ms: old.start_ms,
                        end_ms: old.end_ms.max(old.start_ms),
                        source_text,
                        translated_text,
                    });
                }
                if new_parts_preview.len() != 2 {
                    return Err(format!(
                        "split requires exactly 2 non-empty segment pairs at index {index}"
                    ));
                }
                if new_parts_preview
                    .iter()
                    .any(|part| part.source_text.trim().is_empty() || part.translated_text.trim().is_empty())
                {
                    return Err(format!("split produced empty side at index {index}; rejected"));
                }
                if !split_quality_acceptable(
                    &old.source_text,
                    &old.translated_text,
                    &new_parts_preview,
                    &state.source_lang,
                    &state.target_lang,
                ) {
                    return Err(format!(
                        "split quality check failed at index {index}; use natural boundary and aligned chunk proportions"
                    ));
                }
                let spans =
                    split_span_by_parts(old.start_ms, old.end_ms.max(old.start_ms), new_parts_preview.len());
                for (i, part) in new_parts_preview.into_iter().enumerate() {
                    new_parts.push(TranslateSegment {
                        start_ms: spans[i].0,
                        end_ms: spans[i].1.max(spans[i].0),
                        source_text: part.source_text,
                        translated_text: part.translated_text,
                    });
                }
                let old_source_text = old.source_text.clone();
                let old_target_text = old.translated_text.clone();
                state.working_segments.splice(index..=index, new_parts.clone());
                state.applied_changes.push(QaAppliedChange {
                    kind: "split".to_string(),
                    index,
                    before_source: String::new(),
                    before_target: String::new(),
                    after_source: String::new(),
                    after_target: String::new(),
                    reason: if reason.trim().is_empty() {
                        "overlength_split".to_string()
                    } else {
                        reason
                    },
                    before_segments: vec![QaTextPair {
                        source_text: old_source_text,
                        translated_text: old_target_text,
                    }],
                    after_segments: new_parts
                        .iter()
                        .map(|seg| QaTextPair {
                            source_text: seg.source_text.clone(),
                            translated_text: seg.translated_text.clone(),
                        })
                        .collect(),
                });
            }
            SegmentOp::Merge {
                start_index,
                end_index,
                source_text,
                translated_text,
                reason,
            } => {
                if start_index >= state.working_segments.len()
                    || end_index >= state.working_segments.len()
                    || start_index >= end_index
                {
                    return Err("invalid merge range".to_string());
                }
                let merged_start = state.working_segments[start_index].start_ms;
                let merged_end = state.working_segments[end_index].end_ms;
                let before_segments = state.working_segments[start_index..=end_index]
                    .iter()
                    .map(|seg| QaTextPair {
                        source_text: seg.source_text.clone(),
                        translated_text: seg.translated_text.clone(),
                    })
                    .collect::<Vec<_>>();
                let merged = TranslateSegment {
                    start_ms: merged_start,
                    end_ms: merged_end.max(merged_start),
                    source_text: source_text.trim().to_string(),
                    translated_text: translated_text.trim().to_string(),
                };
                let merged_source = merged.source_text.clone();
                let merged_target = merged.translated_text.clone();
                state
                    .working_segments
                    .splice(start_index..=end_index, vec![merged.clone()]);
                state.applied_changes.push(QaAppliedChange {
                    kind: "merge".to_string(),
                    index: start_index,
                    before_source: String::new(),
                    before_target: String::new(),
                    after_source: String::new(),
                    after_target: String::new(),
                    reason: if reason.trim().is_empty() {
                        "merge_segments".to_string()
                    } else {
                        reason
                    },
                    before_segments,
                    after_segments: vec![QaTextPair {
                        source_text: merged_source,
                        translated_text: merged_target,
                    }],
                });
            }
        }
    }
    Ok(())
}

fn segment_op_sort_key(op: &SegmentOp) -> usize {
    match op {
        SegmentOp::Split { index, .. } => *index,
        SegmentOp::Merge { start_index, .. } => *start_index,
    }
}

fn update_translation(state: &mut QaState, args: UpdateTranslationArgs) -> Result<(), String> {
    if args.index >= state.working_segments.len() {
        return Err(format!("update index out of range: {}", args.index));
    }
    let source_text = state.working_segments[args.index].source_text.clone();
    let before = state.working_segments[args.index].translated_text.clone();
    state.working_segments[args.index].translated_text = args.new_text.trim().to_string();
    let after = state.working_segments[args.index].translated_text.clone();
    state.applied_changes.push(QaAppliedChange {
        kind: "update_translation".to_string(),
        index: args.index,
        before_source: String::new(),
        before_target: String::new(),
        after_source: String::new(),
        after_target: String::new(),
        reason: if args.reason.trim().is_empty() {
            "quality_refine".to_string()
        } else {
            args.reason
        },
        before_segments: vec![QaTextPair {
            source_text: source_text.clone(),
            translated_text: before,
        }],
        after_segments: vec![QaTextPair {
            source_text,
            translated_text: after,
        }],
    });
    Ok(())
}

fn check_change_impact(state: &QaState) -> Value {
    let mut changed_indices: HashSet<usize> = HashSet::new();
    let mut high_risk = 0usize;
    for change in &state.applied_changes {
        changed_indices.insert(change.index);
        let before_text = if !change.before_target.trim().is_empty() {
            change.before_target.clone()
        } else {
            change
                .before_segments
                .iter()
                .map(|v| v.translated_text.as_str())
                .collect::<Vec<_>>()
                .join(" ")
        };
        let after_text = if !change.after_target.trim().is_empty() {
            change.after_target.clone()
        } else {
            change
                .after_segments
                .iter()
                .map(|v| v.translated_text.as_str())
                .collect::<Vec<_>>()
                .join(" ")
        };
        let before_len = text_len(&before_text) as i64;
        let after_len = text_len(&after_text) as i64;
        if before_len > 0 && after_len > 0 && (after_len - before_len).abs() > 20 {
            high_risk += 1;
        }
    }
    json!({
        "changedSegmentTotal": changed_indices.len(),
        "appliedChangeTotal": state.applied_changes.len(),
        "highRiskChangeTotal": high_risk,
    })
}

fn build_report(state: &QaState, finalized: bool, finish_reason: &str) -> Value {
    json!({
        "finalized": finalized,
        "finishReason": finish_reason,
        "segmentTotalBefore": state.original_segments.len(),
        "segmentTotalAfter": state.working_segments.len(),
        "appliedChanges": state.applied_changes,
        "impact": check_change_impact(state),
    })
}

fn split_quality_acceptable(
    full_source: &str,
    full_target: &str,
    parts: &[TranslateSegment],
    source_lang: &str,
    target_lang: &str,
) -> bool {
    if parts.len() <= 1 {
        return true;
    }
    if parts.len() > 2 {
        return false;
    }
    let total_source = source_length_metric(full_source, source_lang).max(1) as f64;
    let total_target = target_length_metric(full_target, target_lang).max(1) as f64;
    for idx in 0..parts.len() {
        let src = parts[idx].source_text.trim();
        let tgt = parts[idx].translated_text.trim();
        let src_len = source_length_metric(src, source_lang);
        let tgt_len = target_length_metric(tgt, target_lang);
        if src_len < 3 || tgt_len < 4 {
            return false;
        }
        let src_share = src_len as f64 / total_source;
        let tgt_share = tgt_len as f64 / total_target;
        if (src_share - tgt_share).abs() > 0.40 {
            return false;
        }
        if idx + 1 < parts.len() {
            let next_src = parts[idx + 1].source_text.trim();
            let next_tgt = parts[idx + 1].translated_text.trim();
            if let (Some(left), Some(right)) = (
                src.split_whitespace().last(),
                next_src.split_whitespace().next(),
            ) {
                if is_bad_split_boundary(left, right) {
                    return false;
                }
            }
            if let (Some(left), Some(right)) = (
                tgt.split_whitespace().last(),
                next_tgt.split_whitespace().next(),
            ) {
                if is_bad_split_boundary(left, right) {
                    return false;
                }
            }
        }
    }
    true
}

fn is_bad_split_boundary(left: &str, right: &str) -> bool {
    let l = left.trim_matches(|c: char| !c.is_alphanumeric() && !is_cjk(c));
    let r = right.trim_matches(|c: char| !c.is_alphanumeric() && !is_cjk(c));
    if l.is_empty() || r.is_empty() {
        return false;
    }
    let left_is_num = l.chars().all(|c| c.is_ascii_digit());
    if left_is_num {
        let right_lower = r.to_lowercase();
        if right_lower.starts_with("min")
            || right_lower.starts_with("hour")
            || right_lower.starts_with("minute")
            || right_lower.starts_with("day")
            || right_lower.starts_with("week")
            || right_lower.starts_with("month")
            || right_lower.starts_with("year")
            || right_lower.starts_with("分钟")
            || right_lower.starts_with("小时")
            || right_lower.starts_with("天")
            || right_lower.starts_with("周")
            || right_lower.starts_with("月")
            || right_lower.starts_with("年")
            || right_lower.starts_with('%')
        {
            return true;
        }
    }
    false
}

fn split_span_by_parts(start_ms: u64, end_ms: u64, parts: usize) -> Vec<(u64, u64)> {
    if parts <= 1 {
        return vec![(start_ms, end_ms)];
    }
    let duration = end_ms.saturating_sub(start_ms).max(parts as u64);
    let mut out = Vec::with_capacity(parts);
    let mut cursor = start_ms;
    for idx in 0..parts {
        let mut next = if idx + 1 == parts {
            end_ms
        } else {
            start_ms + (duration * (idx as u64 + 1)) / parts as u64
        };
        if next <= cursor {
            next = cursor + 1;
        }
        out.push((cursor, next));
        cursor = next;
    }
    out
}

fn text_len(text: &str) -> usize {
    text.chars().filter(|ch| !ch.is_whitespace()).count()
}

fn source_length_metric(text: &str, source_lang: &str) -> usize {
    if is_english_lang(source_lang) {
        return text
            .split_whitespace()
            .filter(|token| token.chars().any(|ch| ch.is_ascii_alphabetic()))
            .count();
    }
    text.chars()
        .filter(|ch| ch.is_alphabetic() || is_cjk(*ch))
        .count()
}

fn target_length_metric(text: &str, target_lang: &str) -> usize {
    if is_chinese_lang(target_lang) {
        return text
            .chars()
            .filter(|ch| ch.is_alphabetic() || is_cjk(*ch))
            .count();
    }
    text.chars()
        .filter(|ch| ch.is_alphabetic() || is_cjk(*ch))
        .count()
}

fn is_english_lang(lang: &str) -> bool {
    let lower = lang.trim().to_lowercase();
    lower.starts_with("en") || lower.contains("english")
}

fn is_chinese_lang(lang: &str) -> bool {
    let lower = lang.trim().to_lowercase();
    lower.starts_with("zh") || lower.contains("chinese")
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
