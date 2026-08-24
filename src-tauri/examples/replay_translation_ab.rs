//! Replay downstream translation A/B for a terminology briefing.
//!
//! Answers the only question that ultimately matters for the terminology
//! agent: does the briefing actually make the final translation more
//! consistent? The same SRT is translated twice through the real translation
//! service:
//!
//!   A (baseline):  user glossary only, no style guide
//!   B (treatment): user glossary + agent glossary (user wins on conflicts),
//!                  plus the agent style guide
//!
//! Metrics are deterministic (substring checks, no judge model):
//!   - Term Target Hit Rate per arm: across source lines where a glossary
//!     source is grounded, how often the translation contains the expected
//!     target. NOTE: this measures glossary enforcement, not glossary
//!     correctness.
//!   - A→B improvements: lines the baseline missed and the treatment hit.
//!   - Treatment misses / review cases: lines the treatment missed — either
//!     translator deviation or a bad glossary row; human review decides which.
//!   - Agent behavior stats parsed from agent.log (rounds, tool calls,
//!     flag_pair usage, submit rejections, salvage, tokens) — only when the
//!     agent was run live by this invocation.
//!
//! Briefing source precedence:
//!   1. --briefing <path>   inject a saved briefing JSON (agent NOT run);
//!      accepts a raw TerminologyBriefing or a replay_terminology dump
//!      ("glossary" + "styleGuide" keys).
//!   2. <media dir>/briefing_replay.json  (written by replay_terminology).
//!   3. Otherwise the terminology agent is run live (agent.log is cleared
//!      first so behavior stats reflect this run).
//!
//! Settings JSON (camelCase), same shape as replay_terminology:
//!   taskId, sourceLang, targetLang, translateApiKey, translateBaseUrl,
//!   translateModel, srtPath, mediaPath, title
//!   optional: userTerms, anysearchApiKey, llmConcurrency, batchSize
//!
//! Cost note: one replay = TWO full translations of the video.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use serde_json::{json, Value};
use voxtrans::services::logs::{self, ClearTaskLogsRequest, ReadTaskLogRequest};
use voxtrans::services::terminology_agent::{
    merge_glossary_user_priority, run_terminology_briefing, source_grounded_in_text,
    EndReason, GlossaryEntry, TerminologyAgentInput, TerminologyBriefing, TranscriptCue,
};
use voxtrans::services::translation::{
    build_translation_layer_with_progress, BuildTranslationLayerRequest, TranslationSegmentInput,
    TranslationTerminologyEntry,
};

const SAMPLE_CAP: usize = 20;

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReplayUserTerm {
    source: String,
    target: String,
    #[serde(default)]
    note: String,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReplaySettings {
    task_id: String,
    source_lang: String,
    target_lang: String,
    translate_api_key: String,
    translate_base_url: String,
    translate_model: String,
    srt_path: String,
    media_path: String,
    title: String,
    #[serde(default)]
    user_terms: Vec<ReplayUserTerm>,
    #[serde(default)]
    anysearch_api_key: String,
    #[serde(default)]
    llm_concurrency: Option<u32>,
    #[serde(default)]
    batch_size: Option<usize>,
}

#[tokio::main]
async fn main() {
    if let Err(err) = run().await {
        eprintln!("replay-ab failed: {err}");
        std::process::exit(1);
    }
}

fn parse_srt(path: &str) -> Result<Vec<TranscriptCue>, String> {
    let content = fs::read_to_string(path).map_err(|e| format!("read srt: {e}"))?;
    let mut cues = Vec::new();
    for block in content.split("\n\n") {
        let lines: Vec<&str> = block.trim().lines().collect();
        if lines.len() < 3 {
            continue;
        }
        let index: usize = lines[0].trim().parse().unwrap_or(cues.len() + 1);
        let start_ms = parse_start_ms(lines[1]).unwrap_or(0);
        cues.push(TranscriptCue {
            index,
            start_ms,
            text: lines[2..].join(" "),
        });
    }
    Ok(cues)
}

fn parse_start_ms(timing: &str) -> Option<u64> {
    let start = timing.split("-->").next()?.trim();
    let (hms, ms) = start.split_once(&[',', '.'][..])?;
    let mut it = hms.split(':');
    let h: u64 = it.next()?.parse().ok()?;
    let m: u64 = it.next()?.parse().ok()?;
    let s: u64 = it.next()?.parse().ok()?;
    let ms: u64 = ms.trim().parse().ok()?;
    Some(((h * 3600 + m * 60 + s) * 1000) + ms)
}

/// Accepts a raw TerminologyBriefing or the replay_terminology dump shape
/// ({ "glossary": [...], "styleGuide": "...", ... }).
fn load_briefing_file(path: &Path) -> Result<TerminologyBriefing, String> {
    let raw = fs::read_to_string(path).map_err(|e| format!("read briefing {}: {e}", path.display()))?;
    if let Ok(b) = serde_json::from_str::<TerminologyBriefing>(&raw) {
        return Ok(b);
    }
    let v: Value = serde_json::from_str(&raw).map_err(|e| format!("parse briefing: {e}"))?;
    let glossary: Vec<GlossaryEntry> = v
        .get("glossary")
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map_err(|e| format!("briefing glossary: {e}"))?
        .unwrap_or_default();
    let style_guide = v
        .get("styleGuide")
        .or_else(|| v.get("style_guide"))
        .and_then(|s| s.as_str())
        .unwrap_or("")
        .to_string();
    Ok(TerminologyBriefing {
        glossary,
        style_guide,
        windows: 0,
        end_reason: EndReason::SubmitOk,
        skipped: false,
        skip_reason: None,
    })
}

fn media_dir(media_path: &str) -> PathBuf {
    PathBuf::from(media_path)
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}

fn to_translate_entries(terms: &[GlossaryEntry]) -> Vec<TranslationTerminologyEntry> {
    terms
        .iter()
        .map(|t| TranslationTerminologyEntry {
            source: t.source.clone(),
            target: t.target.clone(),
            note: t.note.clone(),
        })
        .collect()
}

async fn translate_arm(
    settings: &ReplaySettings,
    cues: &[TranscriptCue],
    label: &str,
    terms: Vec<TranslationTerminologyEntry>,
    style_guide: String,
) -> Result<Vec<String>, String> {
    let segments: Vec<TranslationSegmentInput> = cues
        .iter()
        .map(|c| TranslationSegmentInput {
            segment: c.text.clone(),
            start: c.start_ms as f64 / 1000.0,
            end: c.start_ms as f64 / 1000.0 + 2.0,
            tokens: Vec::new(),
        })
        .collect();
    let request = BuildTranslationLayerRequest {
        task_id: format!("{}-ab-{label}", settings.task_id),
        media_path: settings.media_path.clone(),
        source_lang: settings.source_lang.clone(),
        target_lang: settings.target_lang.clone(),
        segments,
        style_guide,
        terminology_entries: terms,
        translate_api_key: settings.translate_api_key.clone(),
        translate_base_url: settings.translate_base_url.clone(),
        translate_model: settings.translate_model.clone(),
        llm_concurrency: settings.llm_concurrency.unwrap_or(4),
        batch_size: settings.batch_size.unwrap_or(0),
        unit_store: None,
    };
    let last_done = Arc::new(AtomicUsize::new(usize::MAX));
    let label_owned = label.to_string();
    let on_progress = move |p: voxtrans::services::translation::TranslationProgress| {
        if p.done != last_done.swap(p.done, Ordering::Relaxed) {
            eprintln!("arm {label_owned}: batch {}/{}", p.done, p.total);
        }
    };
    let resp = build_translation_layer_with_progress(request, Some(Arc::new(on_progress))).await?;
    let by_id: std::collections::HashMap<usize, String> = resp
        .segments
        .into_iter()
        .map(|s| (s.segment_id, s.translation))
        .collect();
    // Normalized segments keep input order with 1-based ids; empty source
    // lines (none here — cues are non-empty) would shift ids, so fail loudly
    // instead of pairing the wrong translations.
    (1..=cues.len())
        .map(|id| {
            by_id
                .get(&id)
                .cloned()
                .ok_or_else(|| format!("arm {label}: missing translation for segment #{id}"))
        })
        .collect()
}

#[derive(Default)]
struct AgentStats {
    rounds: usize,
    total_tokens: u64,
    tool_calls: BTreeMap<String, usize>,
    flag_pair_calls: usize,
    submit_rejections: usize,
    salvage: usize,
    harness: BTreeMap<String, usize>,
    end_reason: String,
}

/// Parse agent.log entries: "[YYYY-MM-DD HH:MM:SS] event.type\n<pretty json>".
fn parse_agent_log(task_id: &str, media_path: &str) -> Option<AgentStats> {
    let content = logs::read_task_log(ReadTaskLogRequest {
        task_id: task_id.to_string(),
        media_path: Some(media_path.to_string()),
        channel: "agent".to_string(),
    })
    .ok()?;
    if content.trim().is_empty() {
        return None;
    }
    let mut stats = AgentStats::default();
    let mut event = String::new();
    let mut payload = String::new();
    let mut flush = |event: &str, payload: &str, stats: &mut AgentStats| {
        let v: Value = serde_json::from_str(payload).unwrap_or(Value::Null);
        match event {
            "agent.round" => {
                stats.rounds += 1;
                stats.total_tokens += v
                    .pointer("/usage/totalTokens")
                    .and_then(|t| t.as_u64())
                    .unwrap_or(0);
            }
            "agent.tool" => {
                let name = v.get("name").and_then(|n| n.as_str()).unwrap_or("?");
                *stats.tool_calls.entry(name.to_string()).or_default() += 1;
                if name == "flag_pair" {
                    stats.flag_pair_calls += 1;
                }
                if name == "submit_result" {
                    if v.get("ok").and_then(|o| o.as_bool()) == Some(false) {
                        stats.submit_rejections += 1;
                    }
                    if v.get("result")
                        .and_then(|r| r.as_str())
                        .is_some_and(|r| r.contains("SALVAGE"))
                    {
                        stats.salvage += 1;
                    }
                }
            }
            "agent.harness" => {
                let kind = v.get("kind").and_then(|k| k.as_str()).unwrap_or("?");
                *stats.harness.entry(kind.to_string()).or_default() += 1;
            }
            "agent.end" => {
                stats.end_reason = v
                    .get("endReason")
                    .and_then(|r| r.as_str())
                    .unwrap_or("")
                    .to_string();
            }
            _ => {}
        }
    };
    for line in content.lines() {
        let is_entry_head = line.starts_with('[')
            && line.len() > 22
            && line.as_bytes().get(1).is_some_and(u8::is_ascii_digit);
        if is_entry_head {
            if !event.is_empty() {
                flush(&event, &payload, &mut stats);
            }
            event = line
                .split_once("] ")
                .map(|(_, e)| e.trim().to_string())
                .unwrap_or_default();
            payload.clear();
        } else {
            payload.push_str(line);
            payload.push('\n');
        }
    }
    if !event.is_empty() {
        flush(&event, &payload, &mut stats);
    }
    Some(stats)
}

fn contains_ignore_case(hay: &str, needle: &str) -> bool {
    hay.to_lowercase().contains(&needle.to_lowercase())
}

struct TermStat {
    source: String,
    target: String,
    lines: Vec<usize>,
    a_hits: usize,
    b_hits: usize,
}

struct LineCase {
    term: String,
    cue_index: usize,
    source_line: String,
    baseline: String,
    treatment: String,
}

async fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    let settings_path = args
        .get(1)
        .filter(|a| !a.starts_with("--"))
        .cloned()
        .unwrap_or_else(|| r"C:\Users\ADMIN\AppData\Local\Temp\vt-replay-ab.json".into());
    let briefing_flag = args
        .iter()
        .position(|a| a == "--briefing")
        .and_then(|i| args.get(i + 1))
        .cloned();
    let settings: ReplaySettings = serde_json::from_str(
        &fs::read_to_string(&settings_path).map_err(|e| format!("read settings: {e}"))?,
    )
    .map_err(|e| format!("parse settings: {e}"))?;

    let cues = parse_srt(&settings.srt_path)?;
    eprintln!("loaded {} cues from {}", cues.len(), settings.srt_path);
    if cues.is_empty() {
        return Err("no cues parsed from srt".to_string());
    }

    let user_terms: Vec<GlossaryEntry> = settings
        .user_terms
        .iter()
        .map(|t| GlossaryEntry::new(&t.source, &t.target, &t.note))
        .collect();

    // --- Briefing -----------------------------------------------------------
    let cache_path = media_dir(&settings.media_path).join("briefing_replay.json");
    let mut agent_ran_live = false;
    let (briefing, briefing_origin) = if let Some(p) = &briefing_flag {
        (load_briefing_file(Path::new(p))?, format!("injected: {p}"))
    } else if cache_path.exists() {
        (
            load_briefing_file(&cache_path)?,
            format!("cache: {}", cache_path.display()),
        )
    } else {
        // Clear the agent log so behavior stats reflect this run only.
        let _ = logs::clear_task_logs(ClearTaskLogsRequest {
            task_id: settings.task_id.clone(),
            media_path: Some(settings.media_path.clone()),
            channel: Some("agent".to_string()),
        });
        eprintln!("no briefing found — running terminology agent live");
        let briefing = run_terminology_briefing(
            TerminologyAgentInput {
                task_id: settings.task_id.clone(),
                media_path: settings.media_path.clone(),
                title: settings.title.clone(),
                source_lang: settings.source_lang.clone(),
                target_lang: settings.target_lang.clone(),
                cues: cues.clone(),
                user_terms: user_terms.clone(),
                api_key: settings.translate_api_key.clone(),
                base_url: settings.translate_base_url.clone(),
                model: settings.translate_model.clone(),
                anysearch_api_key: settings.anysearch_api_key.clone(),
                store: None,
            },
            |win, nwin, round, max, tool| {
                eprintln!("agent window {win}/{nwin} round {round}/{max} {}", tool.unwrap_or_default());
            },
        )
        .await;
        agent_ran_live = true;
        (briefing, "live agent run".to_string())
    };
    eprintln!(
        "briefing [{}]: skipped={} glossary={} style_chars={}",
        briefing_origin,
        briefing.skipped,
        briefing.glossary.len(),
        briefing.style_guide.chars().count()
    );
    if briefing.skipped || briefing.glossary.is_empty() {
        return Err(
            "briefing is skipped/empty — arms would be identical, nothing to measure".to_string(),
        );
    }

    // Mirror the app pipeline: user terms always kept and win on conflicts.
    let hay: String = cues
        .iter()
        .map(|c| c.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let merged = merge_glossary_user_priority(&user_terms, &briefing.glossary, &hay);

    // --- Translation arms ---------------------------------------------------
    eprintln!("translating arm A (baseline: user terms only)…");
    let arm_a = translate_arm(
        &settings,
        &cues,
        "a",
        to_translate_entries(&user_terms),
        String::new(),
    )
    .await?;
    eprintln!("translating arm B (treatment: merged glossary + style guide)…");
    let arm_b = translate_arm(
        &settings,
        &cues,
        "b",
        to_translate_entries(&merged),
        briefing.style_guide.clone(),
    )
    .await?;

    // --- Metrics ------------------------------------------------------------
    let mut term_stats: Vec<TermStat> = Vec::new();
    let mut improvements: Vec<LineCase> = Vec::new();
    let mut misses: Vec<LineCase> = Vec::new();
    let mut terms_without_lines = 0usize;
    let mut total_lines = 0usize;
    let mut total_a = 0usize;
    let mut total_b = 0usize;

    for term in &merged {
        let lines: Vec<usize> = cues
            .iter()
            .enumerate()
            .filter(|(_, c)| source_grounded_in_text(&term.source, &c.text))
            .map(|(i, _)| i)
            .collect();
        if lines.is_empty() {
            terms_without_lines += 1;
            continue;
        }
        let mut a_hits = 0usize;
        let mut b_hits = 0usize;
        for &i in &lines {
            let a_hit = contains_ignore_case(&arm_a[i], &term.target);
            let b_hit = contains_ignore_case(&arm_b[i], &term.target);
            a_hits += a_hit as usize;
            b_hits += b_hit as usize;
            let case = |term: &GlossaryEntry| LineCase {
                term: term.source.clone(),
                cue_index: cues[i].index,
                source_line: cues[i].text.clone(),
                baseline: arm_a[i].clone(),
                treatment: arm_b[i].clone(),
            };
            if !a_hit && b_hit {
                improvements.push(case(term));
            } else if !b_hit {
                misses.push(case(term));
            }
        }
        total_lines += lines.len();
        total_a += a_hits;
        total_b += b_hits;
        term_stats.push(TermStat {
            source: term.source.clone(),
            target: term.target.clone(),
            lines,
            a_hits,
            b_hits,
        });
    }

    let pct = |hits: usize, lines: usize| {
        if lines == 0 {
            0.0
        } else {
            hits as f64 * 100.0 / lines as f64
        }
    };
    let a_rate = pct(total_a, total_lines);
    let b_rate = pct(total_b, total_lines);
    let agent_stats = if agent_ran_live {
        parse_agent_log(&settings.task_id, &settings.media_path)
    } else {
        None
    };

    // --- Console report -----------------------------------------------------
    println!("\n=== TERMINOLOGY A/B REPORT ===");
    println!("video: {} ({} → {})", settings.title, settings.source_lang, settings.target_lang);
    println!("briefing: {briefing_origin}");
    println!(
        "glossary: {} merged terms ({} grounded, {} without source lines, excluded)",
        merged.len(),
        term_stats.len(),
        terms_without_lines
    );
    println!("style guide: {} chars", briefing.style_guide.chars().count());
    if let Some(s) = &agent_stats {
        println!("\n--- Agent behavior (this run) ---");
        println!("rounds: {}   tokens: {}   end: {}", s.rounds, s.total_tokens, s.end_reason);
        println!("tool calls: {:?}", s.tool_calls);
        println!(
            "flag_pair: {}   submit rejections: {}   salvage: {}",
            s.flag_pair_calls, s.submit_rejections, s.salvage
        );
        if !s.harness.is_empty() {
            println!("harness events: {:?}", s.harness);
        }
    }
    println!("\n--- Term Target Hit Rate (enforcement, not correctness) ---");
    println!("baseline  A: {total_a}/{total_lines} = {a_rate:.1}%");
    println!("treatment B: {total_b}/{total_lines} = {b_rate:.1}%");
    println!("delta: {:+.1} pp", b_rate - a_rate);
    println!("\n--- Per term (lines / A hits / B hits) ---");
    for t in &term_stats {
        println!(
            "  {} → {} : {} / {} / {}",
            t.source,
            t.target,
            t.lines.len(),
            t.a_hits,
            t.b_hits
        );
    }
    println!("\n--- A→B improvements: {} (sample ≤{SAMPLE_CAP}) ---", improvements.len());
    for c in improvements.iter().take(SAMPLE_CAP) {
        println!("  [#{} {}] {}", c.cue_index, c.term, c.source_line);
        println!("      A: {}", c.baseline);
        println!("      B: {}", c.treatment);
    }
    println!("\n--- Treatment misses / review cases: {} (sample ≤{SAMPLE_CAP}) ---", misses.len());
    for c in misses.iter().take(SAMPLE_CAP) {
        println!("  [#{} {}] {}", c.cue_index, c.term, c.source_line);
        println!("      A: {}", c.baseline);
        println!("      B: {}", c.treatment);
    }

    // --- JSON report ----------------------------------------------------------
    let report = json!({
        "video": settings.title,
        "sourceLang": settings.source_lang,
        "targetLang": settings.target_lang,
        "srtPath": settings.srt_path,
        "briefing": {
            "origin": briefing_origin,
            "mergedTerms": merged.len(),
            "groundedTerms": term_stats.len(),
            "termsWithoutSourceLines": terms_without_lines,
            "styleGuideChars": briefing.style_guide.chars().count(),
        },
        "agent": agent_stats.as_ref().map(|s| json!({
            "rounds": s.rounds,
            "totalTokens": s.total_tokens,
            "endReason": s.end_reason,
            "toolCalls": s.tool_calls,
            "flagPairCalls": s.flag_pair_calls,
            "submitRejections": s.submit_rejections,
            "salvage": s.salvage,
            "harness": s.harness,
        })),
        "hitRate": {
            "baseline": { "hits": total_a, "lines": total_lines, "rate": a_rate },
            "treatment": { "hits": total_b, "lines": total_lines, "rate": b_rate },
            "deltaPp": b_rate - a_rate,
        },
        "terms": term_stats.iter().map(|t| json!({
            "source": t.source,
            "target": t.target,
            "lines": t.lines.len(),
            "baselineHits": t.a_hits,
            "treatmentHits": t.b_hits,
        })).collect::<Vec<_>>(),
        "improvements": improvements.iter().map(|c| json!({
            "term": c.term,
            "cueIndex": c.cue_index,
            "sourceLine": c.source_line,
            "baseline": c.baseline,
            "treatment": c.treatment,
        })).collect::<Vec<_>>(),
        "treatmentMisses": misses.iter().map(|c| json!({
            "term": c.term,
            "cueIndex": c.cue_index,
            "sourceLine": c.source_line,
            "baseline": c.baseline,
            "treatment": c.treatment,
        })).collect::<Vec<_>>(),
    });
    let out = media_dir(&settings.media_path).join("translation_ab_report.json");
    fs::write(&out, serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?)
        .map_err(|e| format!("write report: {e}"))?;
    println!("\nwrote {}", out.display());
    Ok(())
}
