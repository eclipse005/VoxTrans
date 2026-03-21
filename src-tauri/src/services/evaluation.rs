use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::services::preferences::load_user_preferences;
use crate::services::task_context::{TaskContext, TaskContextSeed};
use crate::services::task_log::TaskLogger;
use crate::services::translate::adapters::rig_node::{
    JsonResponseValidator, RigNodeClient, RigNodeConfig, RigNodeJsonTask,
};

const EVAL_BATCH_SIZE: usize = 80;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvaluateTaskRequest {
    pub task_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EvaluateTaskResponse {
    pub id: i64,
    pub task_id: String,
    pub overall_score: f64,
    pub summary: String,
    pub metrics_json: String,
    pub output_path: String,
    pub created_at: i64,
}

#[derive(Debug, sqlx::FromRow)]
struct TaskEvalRow {
    id: String,
    intent: String,
    source_lang: String,
    target_lang: String,
    media_path: String,
    media_kind: String,
    size_bytes: i64,
    created_at: i64,
    settings_snapshot_json: String,
    context_json: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct SubtitleSegment {
    start_ms: i64,
    end_ms: i64,
    #[serde(default)]
    source_text: String,
    #[serde(default)]
    translated_text: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LlmEvaluateBatchResult {
    batch_score: f64,
    summary: String,
    metrics: LlmBatchMetrics,
    #[serde(default)]
    issues: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LlmDimensionMetric {
    score: f64,
    #[serde(default)]
    analysis: String,
    #[serde(default)]
    issues: Vec<String>,
}

impl Default for LlmDimensionMetric {
    fn default() -> Self {
        Self {
            score: 0.0,
            analysis: String::new(),
            issues: Vec::new(),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LlmBatchMetrics {
    accuracy: LlmDimensionMetric,
    fluency: LlmDimensionMetric,
    readability: LlmDimensionMetric,
    #[serde(default)]
    terminology_consistency: LlmDimensionMetric,
}

pub async fn evaluate_task(
    pool: &SqlitePool,
    request: EvaluateTaskRequest,
) -> Result<EvaluateTaskResponse, String> {
    let task_id = request.task_id.trim();
    if task_id.is_empty() {
        return Err("taskId is required".to_string());
    }

    let row = sqlx::query_as::<_, TaskEvalRow>(
        "SELECT id, intent, source_lang, target_lang, media_path, media_kind, size_bytes, created_at, settings_snapshot_json, context_json
         FROM task_runs WHERE id = ?",
    )
    .bind(task_id)
    .fetch_optional(pool)
    .await
    .map_err(|err| err.to_string())?
    .ok_or_else(|| "task not found".to_string())?;
    let logger = TaskLogger::main_with_media(task_id.to_string(), row.media_path.clone());
    logger.event(
        "evaluate.started",
        Some(&json!({
            "segmentTotal": segments_count_preview(&row.context_json),
            "model": "from-settings"
        })),
    );

    let settings_snapshot = serde_json::from_str::<Value>(&row.settings_snapshot_json)
        .unwrap_or_else(|_| json!({}));
    let context = TaskContext::parse_or_new(
        &row.context_json,
        TaskContextSeed {
            task_id: row.id.clone(),
            intent: row.intent.clone(),
            source_lang: row.source_lang.clone(),
            target_lang: row.target_lang.clone(),
            media_path: row.media_path.clone(),
            media_kind: row.media_kind.clone(),
            media_size_bytes: row.size_bytes.max(0) as u64,
            settings_snapshot,
            created_at: row.created_at,
        },
    );
    let segments = parse_segments(&context.projections.editor.subtitle_segments_json);
    if segments.is_empty() {
        return Err("当前任务没有可评估的字幕数据".to_string());
    }

    let prefs = load_user_preferences(pool).await?.settings;
    if prefs.translate_api_key.trim().is_empty() {
        return Err("评估失败：未配置 LLM API Key".to_string());
    }
    if prefs.translate_base_url.trim().is_empty() {
        return Err("评估失败：未配置 LLM Base URL".to_string());
    }
    if prefs.translate_model.trim().is_empty() {
        return Err("评估失败：未配置 LLM 模型".to_string());
    }

    let rig_client = RigNodeClient::new(RigNodeConfig::new(
        prefs.translate_base_url.trim().to_string(),
        prefs.translate_api_key.trim().to_string(),
        prefs.translate_model.trim().to_string(),
    ))?;

    let heuristics = compute_heuristics(&segments);
    let eval_terminology = extract_eval_terminology_entries(&context.input.settings_snapshot);
    let batches = build_eval_batches(&segments, EVAL_BATCH_SIZE);
    let batch_total = batches.len();
    let validator =
        JsonResponseValidator::with_required_keys(&["batchScore", "summary", "metrics", "issues"]);
    let system_prompt = build_eval_batch_system_prompt();
    let tasks = batches
        .iter()
        .enumerate()
        .map(|(batch_idx, batch)| RigNodeJsonTask {
            id: batch_idx,
            system_prompt: system_prompt.clone(),
            user_prompt: build_eval_batch_user_prompt(
                &row.source_lang,
                &row.target_lang,
                &heuristics,
                &eval_terminology,
                batch_idx + 1,
                batch_total,
                batch,
            ),
            response_validator: Some(validator.clone()),
        })
        .collect::<Vec<_>>();

    let llm_results = rig_client
        .call_batch(
            task_id,
            Some(&row.media_path),
            "evaluation",
            tasks,
            prefs.llm_concurrency.clamp(1, 16) as usize,
        )
        .await;

    if llm_results.len() != batch_total {
        return Err(format!(
            "评估 LLM 返回批次数异常: expect {batch_total}, got {}",
            llm_results.len()
        ));
    }

    let mut weighted_score_sum = 0.0f64;
    let mut weighted_count_sum = 0.0f64;
    let mut batch_scores = Vec::with_capacity(batch_total);
    let mut batch_summaries = Vec::with_capacity(batch_total);
    let mut issue_counter: HashMap<String, usize> = HashMap::new();
    let mut dim_score_sum: HashMap<String, f64> = HashMap::new();
    let mut dim_weight_sum: HashMap<String, f64> = HashMap::new();
    let mut dim_analysis_samples: HashMap<String, Vec<String>> = HashMap::new();
    let mut dim_issue_counter: HashMap<String, HashMap<String, usize>> = HashMap::new();

    for (batch_idx, result) in llm_results {
        if batch_idx >= batch_total {
            return Err(format!("评估 LLM 返回非法 batch index: {batch_idx}"));
        }
        let raw = result.map_err(|err| format!("评估 LLM 调用失败: {}", err.message))?;
        let parsed: LlmEvaluateBatchResult = serde_json::from_value(raw.json)
            .map_err(|err| format!("评估结果解析失败(批次 {}): {err}", batch_idx + 1))?;

        let batch_size = batches[batch_idx].len().max(1) as f64;
        let batch_score = parsed.batch_score.clamp(0.0, 100.0);
        weighted_score_sum += batch_score * batch_size;
        weighted_count_sum += batch_size;
        batch_scores.push(json!({
            "batch": batch_idx + 1,
            "segmentCount": batches[batch_idx].len(),
            "score": batch_score,
        }));

        let summary = parsed.summary.trim().to_string();
        if !summary.is_empty() {
            batch_summaries.push(summary);
        }

        for issue in parsed.issues {
            let key = normalize_issue_text(&issue);
            if key.is_empty() {
                continue;
            }
            *issue_counter.entry(key.to_string()).or_insert(0) += 1;
        }

        aggregate_dimension_score(
            &mut dim_score_sum,
            &mut dim_weight_sum,
            &mut dim_analysis_samples,
            &mut dim_issue_counter,
            &parsed.metrics,
            batch_size,
            !eval_terminology.is_empty(),
        );
    }

    let overall = if weighted_count_sum <= 0.0 {
        0.0
    } else {
        ((weighted_score_sum / weighted_count_sum).clamp(0.0, 100.0) * 10.0).round() / 10.0
    };
    let top_issues = top_issues(&issue_counter, 6);
    let summary = build_full_eval_summary(
        &batch_summaries,
        &top_issues,
        segments.len(),
        batch_total,
        overall,
    );
    if summary.is_empty() {
        return Err("评估结果无效：summary 为空".to_string());
    }

    let dimension_scores = finalize_dimension_scores(
        &dim_score_sum,
        &dim_weight_sum,
        &dim_analysis_samples,
        &dim_issue_counter,
    );
    let metrics = json!({
        "mode": "llm_full_batch",
        "segmentTotal": segments.len(),
        "batchTotal": batch_total,
        "terminologyEnabled": !eval_terminology.is_empty(),
        "terminologyCount": eval_terminology.len(),
        "overallScore": overall,
        "dimensions": dimension_scores,
        "issuesTop": top_issues,
        "batchScores": batch_scores,
        "heuristics": heuristics
    });
    let metrics_json = serde_json::to_string(&metrics).map_err(|err| err.to_string())?;
    let now = now_unix();

    let mut tx = pool.begin().await.map_err(|err| err.to_string())?;
    let inserted = sqlx::query(
        "INSERT INTO task_evaluations (task_id, version, overall_score, summary, metrics_json, output_path, created_at, updated_at)
         VALUES (?, 'v1', ?, ?, ?, '', ?, ?)",
    )
    .bind(task_id)
    .bind(overall)
    .bind(&summary)
    .bind(&metrics_json)
    .bind(now)
    .bind(now)
    .execute(tx.as_mut())
    .await
    .map_err(|err| err.to_string())?;
    let eval_id = inserted.last_insert_rowid();

    let output_path = write_evaluation_output(
        &row.media_path,
        task_id,
        eval_id,
        now,
        overall,
        &summary,
        &metrics,
        &heuristics,
    )?;
    sqlx::query("UPDATE task_evaluations SET output_path = ?, updated_at = ? WHERE id = ?")
        .bind(&output_path)
        .bind(now)
        .bind(eval_id)
        .execute(tx.as_mut())
        .await
        .map_err(|err| err.to_string())?;
    tx.commit().await.map_err(|err| err.to_string())?;
    logger.event(
        "evaluate.completed",
        Some(&json!({
            "overallScore": overall,
            "summary": summary,
            "dimensions": metrics.get("dimensions").cloned().unwrap_or(Value::Null),
            "issuesTop": metrics.get("issuesTop").cloned().unwrap_or(Value::Null),
            "outputPath": output_path
        })),
    );

    Ok(EvaluateTaskResponse {
        id: eval_id,
        task_id: task_id.to_string(),
        overall_score: overall,
        summary,
        metrics_json,
        output_path,
        created_at: now,
    })
}

fn parse_segments(raw: &str) -> Vec<SubtitleSegment> {
    serde_json::from_str::<Vec<SubtitleSegment>>(raw).unwrap_or_default()
}

fn build_eval_batches(segments: &[SubtitleSegment], batch_size: usize) -> Vec<Vec<Value>> {
    if segments.is_empty() || batch_size == 0 {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut start = 0usize;
    while start < segments.len() {
        let end = (start + batch_size).min(segments.len());
        let mut batch = Vec::with_capacity(end - start);
        for (idx, segment) in segments.iter().enumerate().take(end).skip(start) {
            batch.push(segment_to_json((idx, segment)));
        }
        out.push(batch);
        start = end;
    }
    out
}

fn segment_to_json((idx, segment): (usize, &SubtitleSegment)) -> Value {
    json!({
        "index": idx,
        "sourceText": segment.source_text,
        "translatedText": segment.translated_text
    })
}

fn compute_heuristics(segments: &[SubtitleSegment]) -> Value {
    let total = segments.len() as i64;
    let mut translated_non_empty = 0_i64;
    let mut empty_source = 0_i64;
    let mut timing_invalid = 0_i64;
    let mut overlap_count = 0_i64;
    let mut cps_violations = 0_i64;
    let mut prev_end = i64::MIN;

    for seg in segments {
        let source = seg.source_text.trim();
        let translated = seg.translated_text.trim();
        if source.is_empty() {
            empty_source += 1;
        }
        if !translated.is_empty() {
            translated_non_empty += 1;
            let duration_ms = (seg.end_ms - seg.start_ms).max(1) as f64;
            let cps = translated.chars().count() as f64 / (duration_ms / 1000.0);
            if cps > 17.0 {
                cps_violations += 1;
            }
        }
        if seg.end_ms <= seg.start_ms {
            timing_invalid += 1;
        }
        if prev_end != i64::MIN && seg.start_ms < prev_end {
            overlap_count += 1;
        }
        prev_end = prev_end.max(seg.end_ms);
    }

    json!({
        "segmentTotal": total,
        "translatedNonEmpty": translated_non_empty,
        "translationCoverage": ratio(translated_non_empty, total),
        "emptySourceCount": empty_source,
        "timingInvalidCount": timing_invalid,
        "overlapCount": overlap_count,
        "cpsViolationCount": cps_violations
    })
}

fn build_eval_batch_system_prompt() -> String {
    "你是字幕质量评估助手。你会评估一个批次的字幕质量。必须只返回 JSON。\
输出格式：{\"batchScore\":0-100数字,\"summary\":\"简短中文总结\",\"metrics\":{\"accuracy\":{\"score\":0-100,\"analysis\":\"...\",\"issues\":[\"...\"]},\"fluency\":{\"score\":0-100,\"analysis\":\"...\",\"issues\":[\"...\"]},\"readability\":{\"score\":0-100,\"analysis\":\"...\",\"issues\":[\"...\"]},\"terminologyConsistency\":{\"score\":0-100,\"analysis\":\"...\",\"issues\":[\"...\"]}},\"issues\":[\"问题1\",\"问题2\"]}。\
禁止输出 markdown 或解释文本。".to_string()
}

fn build_eval_batch_user_prompt(
    source_lang: &str,
    target_lang: &str,
    heuristics: &Value,
    terminology: &[Value],
    batch_index: usize,
    batch_total: usize,
    batch_segments: &[Value],
) -> String {
    let mut requirements = vec![
        "评分维度固定为 accuracy、fluency、readability".to_string(),
        "每个维度必须包含 score、analysis、issues 三个字段".to_string(),
        "batchScore 必须综合各维度并反映观看体验".to_string(),
        "issues 返回本批次最显著问题（2-6条）".to_string(),
    ];
    if !terminology.is_empty() {
        requirements.push(
            "启用术语时必须额外返回 terminologyConsistency 维度，并指出术语一致性问题"
                .to_string(),
        );
    }

    json!({
        "task": "subtitle_quality_evaluation_batch",
        "sourceLang": source_lang,
        "targetLang": target_lang,
        "batchIndex": batch_index,
        "batchTotal": batch_total,
        "segmentCount": batch_segments.len(),
        "heuristicsGlobal": heuristics,
        "terminology": terminology,
        "segments": batch_segments,
        "requirements": requirements,
        "output": {
            "jsonOnly": true
        }
    })
    .to_string()
}

fn aggregate_dimension_score(
    score_sum: &mut HashMap<String, f64>,
    weight_sum: &mut HashMap<String, f64>,
    analysis_samples: &mut HashMap<String, Vec<String>>,
    issue_counter: &mut HashMap<String, HashMap<String, usize>>,
    metrics: &LlmBatchMetrics,
    batch_weight: f64,
    terminology_enabled: bool,
) {
    let mut dims = vec![
        ("accuracy", &metrics.accuracy),
        ("fluency", &metrics.fluency),
        ("readability", &metrics.readability),
    ];
    if terminology_enabled {
        dims.push(("terminologyConsistency", &metrics.terminology_consistency));
    }
    for (key, dim) in dims {
        let score = dim.score.clamp(0.0, 100.0);
        *score_sum.entry(key.to_string()).or_insert(0.0) += score * batch_weight;
        *weight_sum.entry(key.to_string()).or_insert(0.0) += batch_weight;
        let analysis = dim.analysis.trim();
        if !analysis.is_empty() {
            analysis_samples
                .entry(key.to_string())
                .or_default()
                .push(analysis.to_string());
        }
        let counter = issue_counter.entry(key.to_string()).or_default();
        for issue in &dim.issues {
            let issue = normalize_issue_text(issue);
            if issue.is_empty() {
                continue;
            }
            *counter.entry(issue.to_string()).or_insert(0) += 1;
        }
    }
}

fn extract_eval_terminology_entries(settings_snapshot: &Value) -> Vec<Value> {
    let enabled = settings_snapshot
        .get("enableTerminology")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if !enabled {
        return Vec::new();
    }
    let Some(groups) = settings_snapshot
        .get("terminologyGroups")
        .and_then(|v| v.as_array())
    else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for group in groups {
        let group_name = group
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        let Some(terms) = group.get("terms").and_then(|v| v.as_array()) else {
            continue;
        };
        for term in terms {
            let source = term
                .get("origin")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            let target = term
                .get("target")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            if source.is_empty() || target.is_empty() {
                continue;
            }
            let note = term
                .get("note")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            out.push(json!({
                "source": source,
                "target": target,
                "group": group_name,
                "note": note
            }));
            if out.len() >= 300 {
                return out;
            }
        }
    }
    out
}

fn finalize_dimension_scores(
    score_sum: &HashMap<String, f64>,
    weight_sum: &HashMap<String, f64>,
    analysis_samples: &HashMap<String, Vec<String>>,
    issue_counter: &HashMap<String, HashMap<String, usize>>,
) -> Value {
    let mut out = serde_json::Map::new();
    for (key, sum) in score_sum {
        let weight = weight_sum.get(key).copied().unwrap_or(0.0);
        if weight <= 0.0 {
            continue;
        }
        let score = ((sum / weight).clamp(0.0, 100.0) * 10.0).round() / 10.0;
        let analysis = analysis_samples
            .get(key)
            .and_then(|v| v.iter().find(|s| !s.trim().is_empty()))
            .cloned()
            .unwrap_or_default();
        let issues = issue_counter
            .get(key)
            .map(|counter| {
                let mut items = counter
                    .iter()
                    .map(|(issue, count)| (issue.clone(), *count))
                    .collect::<Vec<_>>();
                items.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
                items
                    .into_iter()
                    .take(5)
                    .map(|(issue, count)| format!("{issue} (x{count})"))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        out.insert(
            key.clone(),
            json!({
                "score": score,
                "analysis": analysis,
                "issues": issues
            }),
        );
    }
    Value::Object(out)
}

fn top_issues(counter: &HashMap<String, usize>, limit: usize) -> Vec<String> {
    let mut items = counter
        .iter()
        .filter_map(|(issue, count)| {
            let normalized = normalize_issue_text(issue);
            if normalized.is_empty() {
                None
            } else {
                Some((normalized, *count))
            }
        })
        .collect::<Vec<_>>();
    items.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    items
        .into_iter()
        .take(limit)
        .map(|(issue, count)| format!("{issue} (x{count})"))
        .collect()
}

fn build_full_eval_summary(
    batch_summaries: &[String],
    top_issues: &[String],
    segment_total: usize,
    batch_total: usize,
    overall: f64,
) -> String {
    let issue_text = if top_issues.is_empty() {
        "未发现集中性问题".to_string()
    } else {
        top_issues.join("，")
    };
    let extra = batch_summaries
        .iter()
        .find(|s| !s.trim().is_empty())
        .cloned()
        .unwrap_or_default();
    if extra.is_empty() {
        format!(
            "全量评估完成（{} 段/{} 批），综合评分 {:.1}。主要问题：{}",
            segment_total, batch_total, overall, issue_text
        )
    } else {
        format!(
            "全量评估完成（{} 段/{} 批），综合评分 {:.1}。{}；主要问题：{}",
            segment_total, batch_total, overall, extra, issue_text
        )
    }
}

fn write_evaluation_output(
    media_path: &str,
    task_id: &str,
    eval_id: i64,
    created_at: i64,
    overall: f64,
    summary: &str,
    metrics: &Value,
    heuristics: &Value,
) -> Result<String, String> {
    let output_dir = crate::services::task_path::task_output_dir(task_id, Path::new(media_path));
    std::fs::create_dir_all(&output_dir).map_err(|err| err.to_string())?;
    let output_path = build_eval_output_path(&output_dir, created_at, eval_id);
    let payload = json!({
        "taskId": task_id,
        "evaluationId": eval_id,
        "createdAt": created_at,
        "overallScore": overall,
        "summary": summary,
        "metrics": metrics,
        "heuristics": heuristics
    });
    let bytes = serde_json::to_vec_pretty(&payload).map_err(|err| err.to_string())?;
    std::fs::write(&output_path, bytes).map_err(|err| err.to_string())?;
    Ok(output_path.display().to_string())
}

fn build_eval_output_path(output_dir: &Path, created_at: i64, eval_id: i64) -> PathBuf {
    output_dir.join(format!("evaluation_{created_at}_{eval_id}.json"))
}

fn ratio(numerator: i64, denominator: i64) -> f64 {
    if denominator <= 0 {
        return 0.0;
    }
    (numerator.max(0) as f64) / (denominator as f64)
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn normalize_issue_text(raw: &str) -> String {
    let s = raw.trim();
    if s.is_empty() || is_non_issue_text(s) {
        return String::new();
    }
    // Collapse whitespace and trim trailing punctuation noise for better dedupe.
    let compact = s.split_whitespace().collect::<Vec<_>>().join(" ");
    compact
        .trim_matches(|c: char| c == '。' || c == '；' || c == ';' || c == '，' || c == ',')
        .trim()
        .to_string()
}

fn is_non_issue_text(s: &str) -> bool {
    const PHRASES: [&str; 11] = [
        "无显著",
        "无明显",
        "无重大",
        "无问题",
        "未发现问题",
        "未见问题",
        "整体良好",
        "表现良好",
        "表达自然流畅",
        "内容清晰易懂",
        "术语使用规范统一",
    ];
    PHRASES.iter().any(|p| s.contains(p))
}

fn segments_count_preview(context_json: &str) -> usize {
    let Ok(v) = serde_json::from_str::<Value>(context_json) else {
        return 0;
    };
    v.get("projections")
        .and_then(|v| v.get("editor"))
        .and_then(|v| v.get("subtitleSegmentsJson"))
        .and_then(|v| v.as_str())
        .and_then(|raw| serde_json::from_str::<Vec<Value>>(raw).ok())
        .map(|arr| arr.len())
        .unwrap_or(0)
}
