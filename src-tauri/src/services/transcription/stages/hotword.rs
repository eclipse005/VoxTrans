use serde_json::{Value, json};
use sqlx::SqlitePool;
use std::collections::{HashMap, HashSet};

use crate::prompt_builder::{BuildHotwordCorrectionPromptsRequest, HotwordPromptTerm};
use crate::services::llm::{
    LlmCallEnvelope, LlmMessageInput, LlmRuntimeContext, LlmStage, LlmToolCall, LlmToolResult,
};
use crate::services::preferences::{HotwordCorrection, LlmSettings};
use crate::services::transcribe::WordTokenDto;

use crate::services::transcription::domain::{
    HotwordStats, ReplacementStat, StageResult, TimedHotwordSegment,
};

const DEFAULT_WINDOW_SIZE: usize = 80;
const FIRST_PASS_MAX_AGENT_ROUNDS: usize = 20;
const SECOND_PASS_MAX_AGENT_ROUNDS: usize = 10;
const NO_TOOL_RETRY: usize = 2;
const ACTION_MAX_ROUNDS: usize = 12;
const NO_IMPROVE_PATIENCE: usize = 2;
const FOCUS_RESCAN_PADDING: usize = 10;
const HOTWORD_MAX_CHAR_GROWTH_RATIO: f64 = 2.5;
const HOTWORD_MIN_CHAR_GROWTH_RATIO: f64 = 0.4;
const HOTWORD_MAX_TOKEN_DELTA: usize = 3;
const HOTWORD_NUMERIC_JUMP_THRESHOLD: f64 = 0.2;

pub fn should_run_hotword_correction(config: &HotwordCorrection, llm: &LlmSettings) -> bool {
    if !config.enabled || llm.api_key.trim().is_empty() || llm.api_model.trim().is_empty() {
        return false;
    }
    let Some(active_group) = config
        .groups
        .iter()
        .find(|g| g.id == config.active_group_id)
        .or_else(|| config.groups.first())
    else {
        return false;
    };
    !parse_hotword_terms(&active_group.keyterms).is_empty()
}

pub async fn run_stage(
    segments: &mut [TimedHotwordSegment],
    config: &HotwordCorrection,
    llm: &LlmSettings,
    pool: &SqlitePool,
    task_id: &str,
    media_path: &str,
) -> Result<StageResult<HotwordStats>, String> {
    if !should_run_hotword_correction(config, llm) {
        return Ok(StageResult::skipped());
    }
    let active_group = config
        .groups
        .iter()
        .find(|g| g.id == config.active_group_id)
        .or_else(|| config.groups.first());
    let Some(active_group) = active_group else {
        return Ok(StageResult::skipped());
    };
    let original_texts = segments
        .iter()
        .map(|s| s.source_text.clone())
        .collect::<Vec<_>>();
    let terms = parse_hotword_terms(&active_group.keyterms);
    if terms.is_empty() {
        return Ok(StageResult::skipped());
    }

    let prompt_bundle = crate::prompt_builder::build_hotword_correction_prompts(
        BuildHotwordCorrectionPromptsRequest {
            terms: terms
                .iter()
                .map(|(hotword, note)| HotwordPromptTerm {
                    hotword: hotword.clone(),
                    note: note.clone(),
                })
                .collect(),
            total: segments.len(),
            asr_language: Some("English".to_string()),
        },
    )?;

    let mut state = AgentRuntimeState::default();
    run_hotword_agent_session(
        segments,
        &prompt_bundle.system_prompt,
        &prompt_bundle.tools,
        &prompt_bundle.initial_task,
        FIRST_PASS_MAX_AGENT_ROUNDS,
        &mut state,
        llm,
        pool,
        task_id,
        media_path,
    )
    .await?;

    if !state.changes.is_empty() {
        let changed_indexes = state.changed_indexes.iter().copied().collect::<Vec<_>>();
        let focus_ranges =
            build_focus_rescan_ranges(&changed_indexes, segments.len(), FOCUS_RESCAN_PADDING);
        if !focus_ranges.is_empty() {
            let focus_task = build_focus_rescan_task(&terms, &focus_ranges, segments.len());
            run_hotword_agent_session(
                segments,
                &prompt_bundle.system_prompt,
                &prompt_bundle.tools,
                &focus_task,
                SECOND_PASS_MAX_AGENT_ROUNDS,
                &mut state,
                llm,
                pool,
                task_id,
                media_path,
            )
            .await?;
        }
    }

    if !state.changes.is_empty() {
        rebuild_words_from_corrections(segments, &original_texts, &state.changes);
    }
    let changed_count = state.changes.len();
    let replacement_stats = summarize_replacement_stats(&state.changes);
    let summary = if !state.finished_summary.trim().is_empty() {
        state.finished_summary.trim().to_string()
    } else if changed_count > 0 {
        format!("已修改 {changed_count} 处")
    } else {
        "未发现需要矫正的项".to_string()
    };

    Ok(StageResult::executed(HotwordStats {
        changed_count,
        summary,
        replacement_stats,
    }))
}

#[derive(Debug, Default)]
struct AgentRuntimeState {
    action_round: usize,
    no_improve_streak: usize,
    changes: Vec<CorrectionRecord>,
    changed_indexes: HashSet<usize>,
    finished_summary: String,
}

#[derive(Debug, Clone)]
struct CorrectionRecord {
    segment_idx: usize,
    start_idx: usize,
    end_idx: usize,
    old_text: String,
    new_text: String,
}

#[derive(Debug, Clone)]
struct ReplacementRequest {
    old_text: String,
    new_text: String,
}

async fn run_hotword_agent_session(
    segments: &mut [TimedHotwordSegment],
    system_prompt: &str,
    tools: &[crate::services::llm::LlmTool],
    task_prompt: &str,
    max_rounds: usize,
    state: &mut AgentRuntimeState,
    llm: &LlmSettings,
    pool: &SqlitePool,
    task_id: &str,
    media_path: &str,
) -> Result<(), String> {
    let mut messages: Vec<LlmMessageInput> = Vec::new();
    let mut pending_tool_results: Option<Vec<LlmToolResult>> = None;
    let mut no_tool_streak = 0usize;

    for _ in 0..max_rounds {
        let response = crate::services::llm::call(LlmCallEnvelope {
            api_key: llm.api_key.clone(),
            model: llm.api_model.clone(),
            base_url: if llm.api_base.trim().is_empty() {
                None
            } else {
                Some(llm.api_base.clone())
            },
            system_prompt: Some(system_prompt.to_string()),
            prompt: if messages.is_empty() {
                Some(task_prompt.to_string())
            } else {
                None
            },
            messages: if messages.is_empty() {
                None
            } else {
                Some(messages.clone())
            },
            mode: Some("tool".to_string()),
            tools: Some(tools.to_vec()),
            tool_results: pending_tool_results.take(),
            tool_choice: Some(json!("auto")),
            temperature: None,
            max_tokens: None,
            timeout_secs: Some(120),
            max_retries: Some(2),
            context: Some(LlmRuntimeContext {
                task_id: Some(task_id.to_string()),
                media_path: Some(media_path.to_string()),
                stage: Some(LlmStage::Hotword),
            }),
            validation: None,
            usage_pool: Some(pool.clone()),
        })
        .await?;

        if response.status != "requires_tool" || response.tool_calls.is_empty() {
            no_tool_streak += 1;
            if no_tool_streak <= NO_TOOL_RETRY {
                messages.push(LlmMessageInput {
                    role: "assistant".to_string(),
                    content: Some(response.message.unwrap_or_default()),
                    tool_call_id: None,
                    tool_calls: None,
                });
                messages.push(LlmMessageInput {
                    role: "user".to_string(),
                    content: Some(
                        "请不要输出解释文字，必须继续调用工具完成任务；若已完成请调用 finish。"
                            .to_string(),
                    ),
                    tool_call_id: None,
                    tool_calls: None,
                });
                continue;
            }
            break;
        }
        no_tool_streak = 0;

        messages.push(LlmMessageInput {
            role: "assistant".to_string(),
            content: response.message.clone(),
            tool_call_id: None,
            tool_calls: Some(response.tool_calls.clone()),
        });

        let mut results = Vec::new();
        for call in response.tool_calls {
            let parsed = execute_hotword_tool_call(segments, state, &call);
            results.push(LlmToolResult {
                tool_call_id: call.id.clone(),
                content: serde_json::to_string(&parsed).unwrap_or_else(|_| "{}".to_string()),
            });
            let status = parsed
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_ascii_lowercase();
            if call.function.name == "finish" || status == "finished" || status == "stopped" {
                return Ok(());
            }
        }
        pending_tool_results = Some(results);
    }
    Ok(())
}

fn execute_hotword_tool_call(
    segments: &mut [TimedHotwordSegment],
    state: &mut AgentRuntimeState,
    call: &LlmToolCall,
) -> Value {
    let args: Value = serde_json::from_str(&call.function.arguments).unwrap_or_else(|_| json!({}));
    if call.function.name == "read_sentences" {
        let start_raw = args.get("start_idx").and_then(|v| v.as_i64()).unwrap_or(0);
        let end_raw = args
            .get("end_idx")
            .and_then(|v| v.as_i64())
            .unwrap_or(start_raw + DEFAULT_WINDOW_SIZE as i64);
        let total = segments.len() as i64;
        let start = start_raw.clamp(0, total) as usize;
        let end = end_raw.clamp(start as i64, total) as usize;
        let message = segments[start..end]
            .iter()
            .enumerate()
            .map(|(offset, seg)| {
                format!(
                    "第{}句 [{:.1}s-{:.1}s]: {}",
                    start + offset + 1,
                    seg.start_ms as f64 / 1000.0,
                    seg.end_ms as f64 / 1000.0,
                    seg.source_text
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        return json!({
            "status": "ok",
            "start_idx": start,
            "end_idx": end,
            "total": segments.len(),
            "message": message
        });
    }

    if call.function.name == "batch_replace" {
        if state.action_round >= ACTION_MAX_ROUNDS {
            return json!({
              "status": "stopped",
              "message": format!("达到最大修改轮次 {}，停止继续替换", ACTION_MAX_ROUNDS),
              "stop_reason": "max_rounds"
            });
        }
        state.action_round += 1;
        let replacements = args
            .get("replacements")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let requested_replacement_count = replacements.len();
        let mut replacement_requests = Vec::<ReplacementRequest>::new();
        let mut skipped_replacements = Vec::<Value>::new();
        for rep in replacements {
            let old_text = rep
                .get("old_text")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .trim()
                .to_string();
            let new_text = rep
                .get("new_text")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .trim()
                .to_string();
            if old_text.is_empty() || new_text.is_empty() || old_text == new_text {
                continue;
            }
            if let Some(reason) = replacement_guard_reason(&old_text, &new_text) {
                skipped_replacements.push(json!({
                    "oldText": old_text,
                    "newText": new_text,
                    "reason": reason,
                }));
                continue;
            }
            replacement_requests.push(ReplacementRequest { old_text, new_text });
        }
        let mut replacement_terms = HashSet::new();
        for rep in &replacement_requests {
            replacement_terms.insert(rep.old_text.clone());
        }
        let before = replacement_terms
            .iter()
            .map(|term| count_matches_for_text(segments, term))
            .sum::<usize>();
        let mut changes_count = 0usize;

        for rep in replacement_requests.iter() {
            for (idx, segment) in segments.iter_mut().enumerate() {
                let (next_text, matches) =
                    replace_in_text(&segment.source_text, &rep.old_text, &rep.new_text);
                if matches.is_empty() {
                    continue;
                }
                segment.source_text = next_text;
                changes_count += matches.len();
                state.changed_indexes.insert(idx);
                for m in matches {
                    state.changes.push(CorrectionRecord {
                        segment_idx: idx,
                        start_idx: m.start,
                        end_idx: m.end,
                        old_text: m.matched_text,
                        new_text: rep.new_text.clone(),
                    });
                }
            }
        }

        let after = replacement_terms
            .iter()
            .map(|term| count_matches_for_text(segments, term))
            .sum::<usize>();
        let mut status = "ok";
        let mut stop_reason = "";
        if after < before {
            state.no_improve_streak = 0;
        } else {
            state.no_improve_streak += 1;
            if state.no_improve_streak >= NO_IMPROVE_PATIENCE {
                status = "stopped";
                stop_reason = "no_improvement";
            }
        }
        return json!({
          "status": status,
          "changes_count": changes_count,
          "requestedReplacementCount": requested_replacement_count,
          "acceptedReplacementCount": replacement_requests.len(),
          "skippedReplacementCount": skipped_replacements.len(),
          "skippedReplacements": skipped_replacements,
          "metrics": {
             "before": before,
             "after": after,
             "delta": after as i64 - before as i64,
             "no_improve_streak": state.no_improve_streak,
             "round": state.action_round
          },
          "stop_reason": stop_reason
        });
    }

    if call.function.name == "finish" {
        state.finished_summary = args
            .get("summary")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .trim()
            .to_string();
        return json!({
          "status": "finished",
          "summary": state.finished_summary
        });
    }

    json!({ "status": "error", "message": format!("unknown tool: {}", call.function.name) })
}

#[derive(Debug, Clone)]
struct SurfaceMatch {
    start: usize,
    end: usize,
    matched_text: String,
}

fn replace_in_text(text: &str, old_text: &str, new_text: &str) -> (String, Vec<SurfaceMatch>) {
    let matches = find_surface_matches(text, old_text);
    if matches.is_empty() {
        return (text.to_string(), Vec::new());
    }
    let mut out = text.to_string();
    for m in matches.iter().rev() {
        out.replace_range(m.start..m.end, new_text);
    }
    (out, matches)
}

fn find_surface_matches(text: &str, source: &str) -> Vec<SurfaceMatch> {
    let mut raw = Vec::new();
    for variant in build_surface_variants(source) {
        if variant.is_empty() {
            continue;
        }
        let mut from = 0usize;
        while from + variant.len() <= text.len() {
            let Some(relative) = text[from..].find(&variant) else {
                break;
            };
            let start = from + relative;
            let end = start + variant.len();
            if is_word_boundary(text, start, end) {
                raw.push(SurfaceMatch {
                    start,
                    end,
                    matched_text: text[start..end].to_string(),
                });
            }
            from = start + 1;
        }
    }
    raw.sort_by_key(|m| m.start);
    let mut out = Vec::new();
    let mut occupied_end = 0usize;
    for item in raw {
        if item.start < occupied_end {
            continue;
        }
        occupied_end = item.end;
        out.push(item);
    }
    out
}

fn build_surface_variants(text: &str) -> Vec<String> {
    let base = text.trim();
    if base.is_empty() {
        return Vec::new();
    }
    if !(base.contains(' ') || base.contains('-') || base.contains('_')) {
        return vec![base.to_string()];
    }
    let tokens = base
        .split([' ', '-', '_'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>();
    if tokens.len() < 2 {
        return vec![base.to_string()];
    }
    vec![
        base.to_string(),
        tokens.join(" "),
        tokens.join("-"),
        tokens.join(""),
    ]
}

fn is_word_boundary(text: &str, start: usize, end: usize) -> bool {
    if start > 0
        && text[..start]
            .chars()
            .next_back()
            .map(|c| c.is_alphabetic())
            .unwrap_or(false)
    {
        return false;
    }
    if end < text.len()
        && text[end..]
            .chars()
            .next()
            .map(|c| c.is_alphabetic())
            .unwrap_or(false)
    {
        return false;
    }
    true
}

fn count_matches_for_text(segments: &[TimedHotwordSegment], target: &str) -> usize {
    segments
        .iter()
        .map(|s| find_surface_matches(&s.source_text, target).len())
        .sum()
}

fn replacement_guard_reason(old_text: &str, new_text: &str) -> Option<&'static str> {
    if !is_shape_change_safe(old_text, new_text) {
        return Some("shape_delta_too_large");
    }
    if has_time_unit_numeric_conflict(old_text, new_text) {
        return Some("timeframe_number_shift");
    }
    if has_suspicious_numeric_jump(old_text, new_text) {
        return Some("suspicious_numeric_jump");
    }
    None
}

fn is_shape_change_safe(old_text: &str, new_text: &str) -> bool {
    let old_len = old_text.chars().filter(|c| !c.is_whitespace()).count();
    let new_len = new_text.chars().filter(|c| !c.is_whitespace()).count();
    if old_len == 0 || new_len == 0 {
        return false;
    }
    let ratio = new_len as f64 / old_len as f64;
    if !(HOTWORD_MIN_CHAR_GROWTH_RATIO..=HOTWORD_MAX_CHAR_GROWTH_RATIO).contains(&ratio) {
        return false;
    }
    let old_tokens = token_count_for_guard(old_text);
    let new_tokens = token_count_for_guard(new_text);
    old_tokens.abs_diff(new_tokens) <= HOTWORD_MAX_TOKEN_DELTA
}

fn token_count_for_guard(text: &str) -> usize {
    text.split(|c: char| c.is_whitespace() || c == '-' || c == '_' || c == '/')
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .count()
}

fn has_time_unit_numeric_conflict(old_text: &str, new_text: &str) -> bool {
    let Some(old_unit) = detect_time_unit(old_text) else {
        return false;
    };
    let Some(new_unit) = detect_time_unit(new_text) else {
        return false;
    };
    if old_unit != new_unit {
        return false;
    }
    let Some(old_number) = extract_primary_numeric_value(old_text) else {
        return false;
    };
    let Some(new_number) = extract_primary_numeric_value(new_text) else {
        return false;
    };
    (old_number - new_number).abs() > f64::EPSILON
}

fn has_suspicious_numeric_jump(old_text: &str, new_text: &str) -> bool {
    let Some(old_number) = extract_primary_numeric_value(old_text) else {
        return false;
    };
    let Some(new_number) = extract_primary_numeric_value(new_text) else {
        return false;
    };
    if (old_number - new_number).abs() <= f64::EPSILON {
        return false;
    }
    let old_signed_or_decimal = has_signed_or_decimal_number(old_text);
    let new_signed_or_decimal = has_signed_or_decimal_number(new_text);
    !old_signed_or_decimal
        && new_signed_or_decimal
        && (old_number - new_number).abs() > HOTWORD_NUMERIC_JUMP_THRESHOLD
}

fn detect_time_unit(text: &str) -> Option<&'static str> {
    let words = alpha_words(text);
    let has_any = |candidates: &[&str]| -> bool {
        candidates
            .iter()
            .any(|candidate| words.iter().any(|word| word == candidate))
    };
    if has_any(&["sec", "secs", "second", "seconds"]) {
        return Some("second");
    }
    if has_any(&["min", "mins", "minute", "minutes"]) {
        return Some("minute");
    }
    if has_any(&["hr", "hrs", "hour", "hours"]) {
        return Some("hour");
    }
    if has_any(&["day", "days"]) {
        return Some("day");
    }
    if has_any(&["week", "weeks"]) {
        return Some("week");
    }
    if has_any(&["month", "months"]) {
        return Some("month");
    }
    if has_any(&["year", "years"]) {
        return Some("year");
    }
    None
}

fn alpha_words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_ascii_alphabetic())
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(|token| token.to_ascii_lowercase())
        .collect()
}

fn extract_primary_numeric_value(text: &str) -> Option<f64> {
    extract_numeric_values(text).into_iter().next()
}

fn extract_numeric_values(text: &str) -> Vec<f64> {
    let mut out = extract_numeric_lexemes(text)
        .into_iter()
        .filter_map(|lexeme| lexeme.parse::<f64>().ok())
        .collect::<Vec<_>>();
    out.extend(
        alpha_words(text)
            .into_iter()
            .filter_map(|word| number_word_to_value(&word)),
    );
    out
}

fn extract_numeric_lexemes(text: &str) -> Vec<String> {
    let chars = text.chars().collect::<Vec<_>>();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < chars.len() {
        let ch = chars[i];
        let is_sign = ch == '+' || ch == '-';
        let starts_number = ch.is_ascii_digit()
            || (is_sign && i + 1 < chars.len() && chars[i + 1].is_ascii_digit());
        if !starts_number {
            i += 1;
            continue;
        }
        let start = i;
        i += 1;
        while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
            i += 1;
        }
        let lexeme = chars[start..i].iter().collect::<String>();
        if lexeme.chars().any(|c| c.is_ascii_digit()) {
            out.push(lexeme);
        }
    }
    out
}

fn has_signed_or_decimal_number(text: &str) -> bool {
    extract_numeric_lexemes(text).iter().any(|lexeme| {
        lexeme.contains('.')
            || lexeme
                .chars()
                .next()
                .map(|ch| ch == '+' || ch == '-')
                .unwrap_or(false)
    })
}

fn number_word_to_value(word: &str) -> Option<f64> {
    match word {
        "zero" => Some(0.0),
        "one" => Some(1.0),
        "two" => Some(2.0),
        "three" => Some(3.0),
        "four" => Some(4.0),
        "five" => Some(5.0),
        "six" => Some(6.0),
        "seven" => Some(7.0),
        "eight" => Some(8.0),
        "nine" => Some(9.0),
        "ten" => Some(10.0),
        "eleven" => Some(11.0),
        "twelve" => Some(12.0),
        "thirteen" => Some(13.0),
        "fourteen" => Some(14.0),
        "fifteen" => Some(15.0),
        "sixteen" => Some(16.0),
        "seventeen" => Some(17.0),
        "eighteen" => Some(18.0),
        "nineteen" => Some(19.0),
        "twenty" => Some(20.0),
        "thirty" => Some(30.0),
        "forty" => Some(40.0),
        "fifty" => Some(50.0),
        "sixty" => Some(60.0),
        "seventy" => Some(70.0),
        "eighty" => Some(80.0),
        "ninety" => Some(90.0),
        _ => None,
    }
}

fn parse_hotword_terms(raw_terms: &[String]) -> Vec<(String, Option<String>)> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for raw in raw_terms {
        let value = raw.trim();
        if value.is_empty() {
            continue;
        }
        let (name, meaning) = split_hotword_term(value);
        let name = name.trim().to_string();
        if name.is_empty() {
            continue;
        }
        let key = name.to_ascii_lowercase();
        if seen.contains(&key) {
            continue;
        }
        seen.insert(key);
        out.push((name, meaning));
    }
    out
}

fn build_focus_rescan_ranges(
    changed_indexes: &[usize],
    total: usize,
    padding: usize,
) -> Vec<(usize, usize)> {
    let mut ranges = changed_indexes
        .iter()
        .map(|idx| {
            let start = idx.saturating_sub(padding);
            let end = (idx + padding + 1).min(total);
            (start, end)
        })
        .collect::<Vec<_>>();
    if ranges.is_empty() {
        return ranges;
    }
    ranges.sort_by_key(|(start, _)| *start);
    let mut merged = vec![ranges[0]];
    for (start, end) in ranges.into_iter().skip(1) {
        let last = merged.last_mut().expect("merged non-empty");
        if start <= last.1 {
            last.1 = last.1.max(end);
        } else {
            merged.push((start, end));
        }
    }
    merged
}

fn build_focus_rescan_task(
    terms: &[(String, Option<String>)],
    focus_ranges: &[(usize, usize)],
    total_sentences: usize,
) -> String {
    let term_names = terms
        .iter()
        .map(|(name, _)| name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "请对这些重点窗口做第二轮复扫，继续检查是否还有遗漏的术语识别错误：{term_names}\n\n重点复扫窗口：{}\n请优先检查这些窗口，不要重新从头浏览全部 {total_sentences} 句。",
        format_ranges_brief(focus_ranges, 20)
    )
}

fn format_ranges_brief(ranges: &[(usize, usize)], max_show: usize) -> String {
    if ranges.is_empty() {
        return "[]".to_string();
    }
    let mut out = ranges
        .iter()
        .take(max_show)
        .map(|(start, end)| format!("[{}-{}]", start, end.saturating_sub(1).max(*start)))
        .collect::<Vec<_>>();
    if ranges.len() > max_show {
        out.push("...".to_string());
    }
    out.join(" ")
}

fn split_hotword_term(raw: &str) -> (String, Option<String>) {
    for sep in [" : ", ": ", "：", ":"] {
        if let Some(pos) = raw.find(sep) {
            let left = raw[..pos].trim();
            let right = raw[pos + sep.len()..].trim();
            if !left.is_empty() && !right.is_empty() {
                return (left.to_string(), Some(right.to_string()));
            }
        }
    }
    (raw.to_string(), None)
}

fn summarize_replacement_stats(changes: &[CorrectionRecord]) -> Vec<ReplacementStat> {
    let mut map: HashMap<(String, String), usize> = HashMap::new();
    for change in changes {
        let old_text = change.old_text.trim().to_string();
        let new_text = change.new_text.trim().to_string();
        if old_text.is_empty() || new_text.is_empty() {
            continue;
        }
        *map.entry((old_text, new_text)).or_insert(0) += 1;
    }
    map.into_iter()
        .map(|((old_text, new_text), count)| ReplacementStat {
            old_text,
            new_text,
            count,
        })
        .collect()
}

fn rebuild_words_from_corrections(
    segments: &mut [TimedHotwordSegment],
    original_texts: &[String],
    changes: &[CorrectionRecord],
) {
    for (idx, segment) in segments.iter_mut().enumerate() {
        let mut seg_changes = changes
            .iter()
            .filter(|c| c.segment_idx == idx)
            .cloned()
            .collect::<Vec<_>>();
        seg_changes.sort_by(|a, b| b.start_idx.cmp(&a.start_idx));
        if seg_changes.is_empty() || segment.words.is_empty() {
            continue;
        }

        let chunk_map = build_chunk_map(
            original_texts
                .get(idx)
                .map(String::as_str)
                .unwrap_or_default(),
            &segment.words,
        );
        if chunk_map.is_empty() {
            continue;
        }

        let mut final_words: Vec<WordTokenDto> = Vec::new();
        let mut skip_until = 0usize;
        for item in &chunk_map {
            if item.start_idx < skip_until {
                continue;
            }

            let correction = seg_changes
                .iter()
                .find(|corr| !(item.end_idx <= corr.start_idx || item.start_idx >= corr.end_idx));
            let Some(correction) = correction else {
                final_words.push(item.word.clone());
                continue;
            };

            let affected = chunk_map
                .iter()
                .filter(|entry| {
                    !(entry.end_idx <= correction.start_idx
                        || entry.start_idx >= correction.end_idx)
                })
                .collect::<Vec<_>>();
            if affected.is_empty() {
                final_words.push(item.word.clone());
                continue;
            }

            let fixed_start = affected[0].word.start;
            let fixed_end = affected.last().map(|v| v.word.end).unwrap_or(fixed_start);
            final_words.extend(split_text_into_words_with_timing(
                &correction.new_text,
                fixed_start,
                fixed_end,
            ));
            skip_until = correction.end_idx;
        }

        if final_words.is_empty() {
            continue;
        }
        segment.words = final_words;
        segment.start_ms = ((segment.words[0].start * 1000.0).round() as i64).max(0);
        segment.end_ms = ((segment.words.last().map(|w| w.end).unwrap_or(0.0) * 1000.0).round()
            as i64)
            .max(segment.start_ms);
    }
}

#[derive(Debug, Clone)]
struct ChunkMapItem {
    word: WordTokenDto,
    start_idx: usize,
    end_idx: usize,
}

fn build_chunk_map(text: &str, words: &[WordTokenDto]) -> Vec<ChunkMapItem> {
    let mut map = Vec::new();
    let mut cursor = 0usize;
    for word in words {
        let w = word.word.as_str();
        if w.is_empty() {
            continue;
        }
        let start = text[cursor..]
            .find(w)
            .map(|v| cursor + v)
            .unwrap_or(cursor.min(text.len()));
        let end = (start + w.len()).min(text.len());
        map.push(ChunkMapItem {
            word: word.clone(),
            start_idx: start,
            end_idx: end,
        });
        cursor = end;
    }
    map
}

fn split_text_into_words_with_timing(text: &str, start: f64, end: f64) -> Vec<WordTokenDto> {
    let parts = text
        .split_whitespace()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>();
    if parts.is_empty() {
        return Vec::new();
    }
    let duration = (end - start).max(0.0);
    let chunk = duration / parts.len() as f64;
    let mut cursor = start;
    parts
        .iter()
        .enumerate()
        .map(|(idx, part)| {
            let word_start = cursor;
            let word_end = if idx == parts.len() - 1 {
                end
            } else {
                cursor + chunk
            };
            cursor = word_end;
            WordTokenDto {
                start: word_start,
                end: word_end,
                word: (*part).to_string(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guard_blocks_timeframe_number_shift() {
        assert_eq!(
            replacement_guard_reason("50-minute", "15 minute"),
            Some("timeframe_number_shift")
        );
    }

    #[test]
    fn guard_blocks_suspicious_numeric_jump() {
        assert_eq!(
            replacement_guard_reason("zero level", "-0.27 level"),
            Some("suspicious_numeric_jump")
        );
    }

    #[test]
    fn guard_allows_equivalent_numeric_normalization() {
        assert_eq!(replacement_guard_reason("one level", "1 level"), None);
        assert_eq!(replacement_guard_reason("15 minutes", "15 minute"), None);
    }
}
