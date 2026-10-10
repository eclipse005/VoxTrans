//! Run report: the machine-readable result of one verb invocation.
//!
//! Every run writes `<out>/run.json` — the same document `--json` prints on
//! stdout. It is the artifact a script (or a model iterating on the pipeline)
//! reads to answer "what did this change do": per-stage timings, artifact
//! paths, LLM call/token totals broken down by phase, and warnings.
//!
//! Progress and diagnostics go to stderr, never stdout, so `--json` output can
//! be piped straight into a parser.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Instant;

use serde::{Deserialize, Serialize};

use super::CliResult;
use super::context::CliContext;

/// Schema id for the report document. Bump when a consumer-visible field
/// changes shape, so a script can refuse a report it does not understand.
pub const REPORT_SCHEMA: &str = "voxtrans.cli/1";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PhaseUsage {
    pub calls: usize,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UsageSum {
    pub calls: usize,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    /// Per-phase breakdown keyed by the `phase` field of `llm.call` log lines
    /// (`translate`, `terminology`, …).
    #[serde(default)]
    pub by_phase: BTreeMap<String, PhaseUsage>,
}

impl UsageSum {
    fn add(&mut self, phase: &str, prompt: u64, completion: u64, total: u64) {
        self.calls += 1;
        self.prompt_tokens += prompt;
        self.completion_tokens += completion;
        self.total_tokens += total;
        let entry = self.by_phase.entry(phase.to_string()).or_default();
        entry.calls += 1;
        entry.prompt_tokens += prompt;
        entry.completion_tokens += completion;
        entry.total_tokens += total;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StageReport {
    pub name: String,
    pub elapsed_sec: f64,
    /// `computed` (ran now), `cache` (resumed from persisted unit state) or
    /// `skipped` (not applicable to this run).
    pub source: String,
    #[serde(default)]
    pub detail: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunReport {
    pub schema: String,
    pub verb: String,
    pub task_id: String,
    pub media_path: String,
    pub source_lang: String,
    pub target_lang: String,
    /// `db:<path>` or `defaults` — where the settings came from.
    pub settings_source: String,
    /// Registered as a `--task` run (database writeback enabled).
    pub task_registered: bool,
    pub out_dir: String,
    pub ok: bool,
    pub elapsed_sec: f64,
    pub stages: Vec<StageReport>,
    pub artifacts: Vec<String>,
    /// Aggregated from this run's `llm.log`; absent when no LLM call was made.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub llm: Option<UsageSum>,
    pub warnings: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Times one stage and records it into the report.
pub struct StageTimer {
    name: &'static str,
    started: Instant,
}

impl StageTimer {
    pub fn start(name: &'static str) -> Self {
        Self {
            name,
            started: Instant::now(),
        }
    }

    pub fn elapsed_sec(&self) -> f64 {
        round2(self.started.elapsed().as_secs_f64())
    }

    pub fn done(self, report: &mut RunReport, detail: serde_json::Value) {
        let elapsed_sec = self.elapsed_sec();
        report.stages.push(StageReport {
            name: self.name.to_string(),
            elapsed_sec,
            source: "computed".to_string(),
            detail,
        });
    }

    pub fn done_with_source(self, report: &mut RunReport, source: &str, detail: serde_json::Value) {
        let elapsed_sec = self.elapsed_sec();
        report.stages.push(StageReport {
            name: self.name.to_string(),
            elapsed_sec,
            source: source.to_string(),
            detail,
        });
    }
}

impl RunReport {
    pub fn new(ctx: &CliContext) -> Self {
        Self {
            schema: REPORT_SCHEMA.to_string(),
            verb: ctx.verb.to_string(),
            task_id: ctx.task_id.clone(),
            media_path: ctx.input_path_str.clone(),
            source_lang: ctx.source_lang.clone(),
            target_lang: ctx.target_lang.clone(),
            settings_source: ctx.settings_source.clone(),
            task_registered: ctx.task_registered,
            out_dir: ctx.out_dir.display().to_string(),
            ok: true,
            elapsed_sec: 0.0,
            stages: Vec::new(),
            artifacts: Vec::new(),
            llm: None,
            warnings: Vec::new(),
            error: None,
        }
    }

    pub fn artifact(&mut self, path: impl AsRef<Path>) {
        let path = path.as_ref().display().to_string();
        if !self.artifacts.contains(&path) {
            self.artifacts.push(path);
        }
    }

    pub fn warn(&mut self, message: impl Into<String>) {
        self.warnings.push(message.into());
    }

    /// Finalise, persist `run.json`, print the result, and return the outcome so
    /// the process exit code reflects it. The report is written on failure too —
    /// a failed run is exactly when its stage timings and warnings matter.
    pub fn finish(mut self, ctx: &CliContext, outcome: CliResult<()>) -> CliResult<()> {
        self.elapsed_sec = round2(ctx.started.elapsed().as_secs_f64());
        match &outcome {
            Ok(()) => self.ok = true,
            Err(err) => {
                self.ok = false;
                self.error = Some(err.clone());
            }
        }
        self.llm = collect_llm_usage(ctx);

        let path = ctx.report_path();
        match serde_json::to_string_pretty(&self) {
            Ok(json) => {
                if let Err(err) = std::fs::write(&path, &json) {
                    ctx.warn(&format!("failed to write {}: {err}", path.display()));
                }
            }
            Err(err) => ctx.warn(&format!("failed to serialize run report: {err}")),
        }

        if ctx.json {
            match serde_json::to_string_pretty(&self) {
                Ok(json) => println!("{json}"),
                Err(err) => ctx.warn(&format!("failed to serialize run report: {err}")),
            }
        } else {
            print_summary(&self, &path);
        }

        outcome
    }
}

fn print_summary(report: &RunReport, report_path: &Path) {
    let status = if report.ok { "ok" } else { "FAILED" };
    println!(
        "voxtrans-cli {} {} — task={} {:.1}s",
        report.verb, status, report.task_id, report.elapsed_sec
    );
    for stage in &report.stages {
        let detail = summarize_detail(&stage.detail);
        let suffix = if detail.is_empty() {
            String::new()
        } else {
            format!(" {detail}")
        };
        println!(
            "  {:<12} {:>7.1}s  {}{}",
            stage.name, stage.elapsed_sec, stage.source, suffix
        );
    }
    for artifact in &report.artifacts {
        println!("  -> {artifact}");
    }
    if let Some(llm) = &report.llm {
        println!(
            "  llm: {} calls, {} prompt + {} completion = {} tokens",
            llm.calls, llm.prompt_tokens, llm.completion_tokens, llm.total_tokens
        );
    }
    for warning in &report.warnings {
        println!("  warning: {warning}");
    }
    if let Some(err) = &report.error {
        println!("  error: {err}");
    }
    println!("  report: {}", report_path.display());
}

/// One-line rendering of a stage's detail object: `key=value` pairs, scalars
/// only — nested objects would make the summary unreadable.
fn summarize_detail(detail: &serde_json::Value) -> String {
    let Some(map) = detail.as_object() else {
        return String::new();
    };
    map.iter()
        .filter_map(|(key, value)| match value {
            serde_json::Value::String(text) if !text.is_empty() => Some(format!("{key}={text}")),
            serde_json::Value::String(_) => None,
            serde_json::Value::Number(number) => Some(format!("{key}={number}")),
            serde_json::Value::Bool(flag) => Some(format!("{key}={flag}")),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Read this run's `llm.log` and sum the `llm.call` entries.
///
/// The log is written by the LLM client itself, so it covers every phase
/// (terminology windows, translation batches, retries) without the CLI having
/// to thread usage counters through the pipeline. Missing log or unparseable
/// entries are not errors — an absent log simply means the run made no LLM call.
fn collect_llm_usage(ctx: &CliContext) -> Option<UsageSum> {
    let path = ctx.log_dir().join("llm.log");
    let content = std::fs::read_to_string(&path).ok()?;
    let usage = parse_llm_log_usage(&content);
    if usage.calls == 0 { None } else { Some(usage) }
}

/// Parse `llm.log` (`"[<timestamp>] event.type\n<pretty json>"` entries) and sum
/// `llm.call` rows by phase. Only successful calls with usage count.
pub fn parse_llm_log_usage(content: &str) -> UsageSum {
    let mut sum = UsageSum::default();
    let mut event = String::new();
    let mut payload = String::new();

    let flush = |event: &str, payload: &str, sum: &mut UsageSum| {
        if event != "llm.call" {
            return;
        }
        let Ok(value) = serde_json::from_str::<serde_json::Value>(payload) else {
            return;
        };
        if value
            .get("status")
            .and_then(|status| status.as_str())
            .is_some_and(|status| status != "ok")
        {
            return;
        }
        let phase = value
            .get("phase")
            .and_then(|phase| phase.as_str())
            .unwrap_or("unknown");
        let usage = value.get("usage");
        let get = |key: &str| {
            usage
                .and_then(|usage| usage.get(key))
                .and_then(|value| value.as_u64())
                .unwrap_or(0)
        };
        let prompt = get("promptTokens");
        let completion = get("completionTokens");
        let total = {
            let total = get("totalTokens");
            if total > 0 { total } else { prompt + completion }
        };
        sum.add(phase, prompt, completion, total);
    };

    for line in content.lines() {
        let is_entry_head =
            line.starts_with('[') && line.len() > 22 && line.as_bytes().get(1).is_some_and(u8::is_ascii_digit);
        if is_entry_head {
            if !event.is_empty() {
                flush(&event, &payload, &mut sum);
            }
            event = line
                .split_once("] ")
                .map(|(_, event)| event.trim().to_string())
                .unwrap_or_default();
            payload.clear();
        } else {
            payload.push_str(line);
            payload.push('\n');
        }
    }
    if !event.is_empty() {
        flush(&event, &payload, &mut sum);
    }

    sum
}

pub fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Usage lines are pretty-printed JSON spanning several lines: the parser
    /// must attribute them to the entry head above, not to line boundaries.
    #[test]
    fn parses_multi_line_usage_entries_by_phase() {
        let log = "\
[2026-10-10 10:00:00] llm.call
{
  \"phase\": \"terminology\",
  \"status\": \"ok\",
  \"usage\": { \"promptTokens\": 100, \"completionTokens\": 10, \"totalTokens\": 110 }
}
[2026-10-10 10:00:01] llm.call
{
  \"phase\": \"translate\",
  \"status\": \"ok\",
  \"usage\": { \"promptTokens\": 200, \"completionTokens\": 20 }
}
[2026-10-10 10:00:02] llm.call
{
  \"phase\": \"translate\",
  \"status\": \"failed\",
  \"usage\": { \"promptTokens\": 999, \"completionTokens\": 999 }
}
";
        let usage = parse_llm_log_usage(log);
        assert_eq!(usage.calls, 2, "failed calls must not count");
        assert_eq!(usage.prompt_tokens, 300);
        assert_eq!(usage.completion_tokens, 30);
        // totalTokens absent on the second entry: fall back to prompt+completion.
        assert_eq!(usage.total_tokens, 110 + 220);
        assert_eq!(usage.by_phase["terminology"].calls, 1);
        assert_eq!(usage.by_phase["translate"].prompt_tokens, 200);
    }

    #[test]
    fn empty_log_yields_zero_usage() {
        let usage = parse_llm_log_usage("");
        assert_eq!(usage.calls, 0);
        assert_eq!(usage.total_tokens, 0);
    }
}
