use serde_json::{json, Value};

use super::types::{normalize_term_key, GlossaryEntry, ToolOutcome, TranscriptCue};

pub const WEB_SEARCH_BUDGET: u32 = 3;
const MAX_READ_CUES: usize = 150;
const MAX_READ_CUES_CHARS: usize = 30_000;
const MAX_DECLARED_PAIRS: usize = 20;
const SEARCH_EXAMPLE_CAP: usize = 3;
const COUNT_TERM_CAP: usize = 24;
const COUNT_SAMPLE_CAP: usize = 3;
const MAX_QUERY_CHARS: usize = 200;
const MIN_QUERY_CHARS: usize = 3;
const MAX_WEB_RESULT_CHARS: usize = 2_000;
const MAX_GLOSSARY_ROWS: usize = 80;
const MAX_TOOL_MESSAGE_CHARS: usize = 4_000;
const SEARCH_CONTEXT_DEFAULT: u64 = 40;
const SEARCH_CONTEXT_MAX: u64 = 80;

pub fn tool_schemas(web_search: bool) -> Vec<Value> {
    let mut schemas = vec![
        function_schema(
            "count_transcript",
            "Count candidate phrases first. Returns Nx plus a few cue ids. Prefer this before search_transcript.",
            json!({
                "type": "object",
                "properties": {
                    "terms": { "type": "array", "items": { "type": "string" } },
                    "ignore_case": { "type": "boolean" }
                },
                "required": ["terms"]
            }),
        ),
        function_schema(
            "search_transcript",
            "Count one phrase and show a few usage examples. Not a transcript dump. Use after count_transcript.",
            json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string" },
                    "ignore_case": { "type": "boolean" },
                    "context_chars": { "type": "integer" }
                },
                "required": ["pattern"]
            }),
        ),
    ];
    schemas.push(function_schema(
        "read_cues",
        "Re-read a verbatim cue range by index (inclusive) — use after the transcript is evicted from context, or to inspect a suspicious region closely. Large ranges are capped; narrow the range if truncated.",
        json!({
            "type": "object",
            "properties": {
                "from_index": { "type": "integer" },
                "to_index": { "type": "integer" }
            },
            "required": ["from_index", "to_index"]
        }),
    ));
    schemas.push(function_schema(
        "flag_pair",
        "Register a suspect pair YOU noticed while reading: two similar surfaces that may be the same term (e.g. one a consistent ASR mishearing of the other), including pairs the ⚠ list missed. Returns verbatim evidence lines for both surfaces. Flagged pairs face the SAME submit-gate rules as pre-flagged ⚠ pairs. The goal is translation consistency, not fixing the transcript.",
        json!({
            "type": "object",
            "properties": {
                "a": { "type": "string" },
                "b": { "type": "string" }
            },
            "required": ["a", "b"]
        }),
    ));
    if web_search {
        schemas.push(function_schema(
            "web_search",
            "Budgeted web lookup for uncertain proper names / abbreviations: conventional target rendering or entity sense. Prefer transcript tools to ground sources first. Short query (not a transcript dump). Always qualify the query with 1–2 domain keywords from THIS video (title/context) — a bare generic phrase gets hits everywhere and proves nothing. Trust encyclopedic/glossary sources over raw transcript pages — those may repeat the same ASR error.",
            json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Surface + this video's domain keywords, e.g. \"<term>\" <domain>. Never a bare generic phrase." },
                    "purpose": {
                        "type": "string",
                        "enum": ["render", "disambiguate", "expand", "other"]
                    }
                },
                "required": ["query"]
            }),
        ));
    }
    schemas.push(function_schema(
            "submit_result",
            "Submit FINAL terminology briefing. Call this tool ALONE in the turn, with no search/count/web_search beside it. glossary: terms needing consistent source→target. style_guide: 2–4 sentences for THIS video. No full translation.",
            json!({
                "type": "object",
                "properties": {
                    "glossary": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "source": { "type": "string" },
                                "target": { "type": "string" },
                                "note": { "type": "string" }
                            },
                            "required": ["source", "target"]
                        }
                    },
                    "style_guide": { "type": "string" }
                },
                "required": ["glossary", "style_guide"]
            }),
        ));
    schemas
}

fn function_schema(name: &str, description: &str, parameters: Value) -> Value {
    json!({
        "type": "function",
        "function": {
            "name": name,
            "description": description,
            "parameters": parameters
        }
    })
}

pub fn verification_tool_names() -> &'static [&'static str] {
    &["count_transcript", "search_transcript", "read_cues", "web_search"]
}

pub fn is_submit_tool(name: &str) -> bool {
    name == "submit_result"
}

pub fn is_probe_tool(name: &str) -> bool {
    matches!(name, "count_transcript" | "search_transcript" | "read_cues" | "web_search")
}

pub fn blocked_probe_message(name: &str) -> String {
    format!(
        "Blocked: '{name}' was skipped because this turn also called submit_result. \
         Call submit_result alone. Probe tools from this turn were not executed."
    )
}

pub fn parse_tool_arguments(raw: &str) -> Result<Value, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(json!({}));
    }
    match serde_json::from_str::<Value>(trimmed) {
        Ok(Value::Null) => Ok(json!({})),
        Ok(value) => Ok(value),
        Err(_) => {
            // Some models wrap JSON in fences.
            let inner = trimmed
                .trim_start_matches("```json")
                .trim_start_matches("```")
                .trim_end_matches("```")
                .trim();
            serde_json::from_str(inner).map_err(|e| {
                format!("could not parse arguments as JSON ({e}). Hint: pass a JSON object.")
            })
        }
    }
}

pub struct AgentToolContext<'a> {
    pub cues: &'a [TranscriptCue],
    pub web_search_count: u32,
    pub web_search_enabled: bool,
    pub web_queries: Vec<String>,
    pub submit_rejects: Vec<String>,
    pub final_glossary: Option<Vec<GlossaryEntry>>,
    pub final_style: Option<String>,
    pub confusable_pairs: &'a [(String, String)],
    /// Pairs the model registered itself via flag_pair — gated identically
    /// to the deterministic confusable_pairs at submit.
    pub declared_pairs: Vec<(String, String)>,
}

pub fn dispatch_tool(name: &str, args: &Value, ctx: &mut AgentToolContext<'_>) -> ToolOutcome {
    let outcome = match name {
        "search_transcript" => tool_search_transcript(args, ctx.cues),
        "count_transcript" => tool_count_transcript(args, ctx.cues),
        "read_cues" => tool_read_cues(args, ctx.cues),
        "flag_pair" => tool_flag_pair(args, ctx),
        "web_search" => {
            if !ctx.web_search_enabled {
                ToolOutcome::err(
                    "Error: web_search is not configured. Ground terms in the transcript.".to_string(),
                )
            } else {
                // Network call is executed by the loop (async). Here we only
                // validate query/budget; the loop replaces this with the real
                // result when validation passes.
                tool_web_search_preflight(args, ctx)
            }
        }
        "submit_result" => tool_submit_result(args, ctx),
        other => {
            let available = if ctx.web_search_enabled {
                "count_transcript, search_transcript, read_cues, flag_pair, web_search, submit_result"
            } else {
                "count_transcript, search_transcript, read_cues, flag_pair, submit_result"
            };
            ToolOutcome::err(format!(
                "Error: unknown tool '{other}'. Available: {available}."
            ))
        }
    };
    ToolOutcome {
        message: truncate_tool_message(&outcome.message),
        ..outcome
    }
}

fn truncate_tool_message(msg: &str) -> String {
    if msg.chars().count() <= MAX_TOOL_MESSAGE_CHARS {
        return msg.to_string();
    }
    let mut out: String = msg.chars().take(MAX_TOOL_MESSAGE_CHARS).collect();
    out.push_str("…[truncated]");
    out
}

fn bool_arg(v: &Value, key: &str, default: bool) -> bool {
    match v.get(key) {
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) => !matches!(s.to_ascii_lowercase().as_str(), "false" | "0" | "no"),
        Some(Value::Number(n)) => n.as_i64().unwrap_or(1) != 0,
        _ => default,
    }
}

fn string_arg(v: &Value, keys: &[&str]) -> String {
    for key in keys {
        if let Some(s) = v.get(*key).and_then(|x| x.as_str()) {
            return s.to_string();
        }
    }
    String::new()
}

pub fn tool_search_transcript(args: &Value, cues: &[TranscriptCue]) -> ToolOutcome {
    let pattern = string_arg(args, &["pattern", "query"]).trim().to_string();
    if pattern.is_empty() {
        return ToolOutcome::err(
            "Error: 'pattern' is required. Hint: {\"pattern\":\"order block\",\"ignore_case\":true}",
        );
    }
    if cues.is_empty() {
        return ToolOutcome::err("Error: no transcript loaded in harness context.");
    }
    let context_chars = args
        .get("context_chars")
        .and_then(|v| v.as_u64())
        .unwrap_or(SEARCH_CONTEXT_DEFAULT)
        .clamp(0, SEARCH_CONTEXT_MAX) as usize;
    let ignore_case = bool_arg(args, "ignore_case", true);
    let needle = if ignore_case {
        pattern.to_lowercase()
    } else {
        pattern.clone()
    };

    let mut hits = Vec::new();
    let mut cue_ids = Vec::new();
    let mut total = 0usize;
    for cue in cues {
        let hay_owned;
        let hay: &str = if ignore_case {
            hay_owned = cue.text.to_lowercase();
            &hay_owned
        } else {
            &cue.text
        };
        let mut start = 0;
        let mut cue_hit = false;
        while let Some(rel) = hay[start..].find(&needle) {
            let idx = start + rel;
            total += 1;
            cue_hit = true;
            let s = idx.saturating_sub(context_chars);
            let ed = (idx + pattern.len() + context_chars).min(cue.text.len());
            let s = cue.text.floor_char_boundary(s);
            let ed = cue.text.ceil_char_boundary(ed.min(cue.text.len()));
            let snippet = &cue.text[s..ed];
            let left = if s > 0 { "[...]" } else { "" };
            let right = if ed < cue.text.len() { "[...]" } else { "" };
            hits.push(format!(
                "[#{} {}] {left}{snippet}{right}",
                cue.index,
                format_mmss(cue.start_ms)
            ));
            let next = idx + needle.len().max(1);
            if next >= hay.len() {
                break;
            }
            start = hay.ceil_char_boundary(next);
            if start >= hay.len() {
                break;
            }
        }
        if cue_hit {
            cue_ids.push(cue.index);
        }
    }
    if total == 0 {
        return ToolOutcome::ok_msg(format!(
            "0x  '{pattern}' ({} segments, ignore_case={ignore_case}).",
            cues.len()
        ));
    }
    let examples = sample_search_hits(&hits, SEARCH_EXAMPLE_CAP);
    ToolOutcome::ok_msg(format!(
        "{total}x  '{pattern}'  across {} cues (ignore_case={ignore_case})\nexamples:\n{}",
        cue_ids.len(),
        examples
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    ))
}

fn sample_search_hits(hits: &[String], cap: usize) -> Vec<&String> {
    if hits.is_empty() || cap == 0 {
        return Vec::new();
    }
    if hits.len() <= cap {
        return hits.iter().collect();
    }
    if cap == 1 {
        return vec![&hits[0]];
    }
    let mut out = Vec::with_capacity(cap);
    out.push(&hits[0]);
    if cap >= 3 {
        out.push(&hits[hits.len() / 2]);
    }
    out.push(&hits[hits.len() - 1]);
    out
}

pub fn tool_count_transcript(args: &Value, cues: &[TranscriptCue]) -> ToolOutcome {
    let terms_val = args.get("terms").or_else(|| args.get("phrases"));
    let mut terms: Vec<String> = match terms_val {
        Some(Value::String(s)) => vec![s.clone()],
        Some(Value::Array(arr)) => arr
            .iter()
            .filter_map(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect(),
        _ => Vec::new(),
    };
    if terms.is_empty() {
        return ToolOutcome::err(
            "Error: 'terms' must be a non-empty list of strings. Hint: {\"terms\":[\"order block\"]}",
        );
    }
    if cues.is_empty() {
        return ToolOutcome::err("Error: no transcript loaded in harness context.");
    }
    let ignore_case = bool_arg(args, "ignore_case", true);
    let truncated = terms.len() > COUNT_TERM_CAP;
    terms.truncate(COUNT_TERM_CAP);

    let mut lines = Vec::new();
    for t in &terms {
        let needle = if ignore_case { t.to_lowercase() } else { t.clone() };
        let mut count = 0usize;
        let mut samples = Vec::new();
        for cue in cues {
            let hay = if ignore_case {
                cue.text.to_lowercase()
            } else {
                cue.text.clone()
            };
            let n = hay.matches(&needle).count();
            if n > 0 {
                count += n;
                if samples.len() < COUNT_SAMPLE_CAP {
                    samples.push(cue.index);
                }
            }
        }
        let sample = if samples.is_empty() {
            String::new()
        } else {
            format!(
                "  e.g. #{}",
                samples
                    .iter()
                    .map(|i| i.to_string())
                    .collect::<Vec<_>>()
                    .join(", #")
            )
        };
        lines.push(format!("  {count}x  '{t}'{sample}"));
    }
    let header = if truncated {
        format!("Transcript term counts (first {COUNT_TERM_CAP} terms only) (ignore_case={ignore_case}):\n")
    } else {
        format!("Transcript term counts (ignore_case={ignore_case}):\n")
    };
    ToolOutcome::ok_msg(format!("{header}{}", lines.join("\n")))
}

fn int_arg(v: &Value, keys: &[&str]) -> Option<i64> {
    for key in keys {
        match v.get(*key) {
            Some(Value::Number(n)) => return n.as_i64(),
            Some(Value::String(s)) => return s.trim().parse().ok(),
            _ => {}
        }
    }
    None
}

/// Verbatim re-read of a cue index range — the Read-equivalent after the
/// transcript is evicted from context. Capped so it cannot become a second
/// full-transcript dump.
pub fn tool_read_cues(args: &Value, cues: &[TranscriptCue]) -> ToolOutcome {
    let from = int_arg(args, &["from_index", "from", "start"]);
    let to = int_arg(args, &["to_index", "to", "end"]);
    let (Some(from), Some(to)) = (from, to) else {
        return ToolOutcome::err(
            "Error: 'from_index' and 'to_index' are required. Hint: {\"from_index\":40,\"to_index\":70}",
        );
    };
    if cues.is_empty() {
        return ToolOutcome::err("Error: no transcript loaded in harness context.");
    }
    let (lo, hi) = if from <= to { (from, to) } else { (to, from) };
    let mut lines = Vec::new();
    let mut chars = 0usize;
    let mut capped = false;
    for cue in cues
        .iter()
        .filter(|c| (c.index as i64) >= lo && (c.index as i64) <= hi)
    {
        let line = format!("[#{} {}] {}", cue.index, format_mmss(cue.start_ms), cue.text);
        let n = line.chars().count() + 1;
        if lines.len() >= MAX_READ_CUES || chars + n > MAX_READ_CUES_CHARS {
            capped = true;
            break;
        }
        chars += n;
        lines.push(line);
    }
    if lines.is_empty() {
        return ToolOutcome::err(format!(
            "Error: no cues in index range {lo}..={hi} (transcript spans #{}..=#{}).",
            cues.first().map(|c| c.index).unwrap_or(0),
            cues.last().map(|c| c.index).unwrap_or(0),
        ));
    }
    let mut out = format!(
        "Cues #{lo}..=#{hi} ({} shown):\n{}",
        lines.len(),
        lines.join("\n")
    );
    if capped {
        out.push_str(&format!(
            "\n…[capped at {MAX_READ_CUES} cues / {MAX_READ_CUES_CHARS} chars — narrow the range]"
        ));
    }
    ToolOutcome::ok_msg(out)
}

/// Model-declared suspect pair. Both surfaces must appear in the transcript
/// (the gate only adjudicates surfaces that can fire on real subtitle lines).
/// Returns verbatim evidence lines so the model can adjudicate immediately.
pub fn tool_flag_pair(args: &Value, ctx: &mut AgentToolContext<'_>) -> ToolOutcome {
    let a = string_arg(args, &["a", "surface_a", "first"]).trim().to_string();
    let b = string_arg(args, &["b", "surface_b", "second"]).trim().to_string();
    if a.is_empty() || b.is_empty() {
        return ToolOutcome::err(
            "Error: 'a' and 'b' are required. Hint: {\"a\":\"hard time frame\",\"b\":\"high time frame\"}",
        );
    }
    let (ka, kb) = (normalize_term_key(&a), normalize_term_key(&b));
    if ka.is_empty() || kb.is_empty() || ka == kb {
        return ToolOutcome::err("Error: the two surfaces must be distinct non-empty strings.");
    }
    let hay = ctx
        .cues
        .iter()
        .map(|c| c.text.to_lowercase())
        .collect::<Vec<_>>()
        .join("\n");
    for surface in [&a, &b] {
        if !hay.contains(&surface.to_lowercase()) {
            return ToolOutcome::err(format!(
                "Error: '{surface}' does not appear in the transcript. flag_pair is for transcript surfaces only."
            ));
        }
    }
    let already = ctx
        .confusable_pairs
        .iter()
        .chain(ctx.declared_pairs.iter())
        .any(|(x, y)| {
            let (kx, ky) = (normalize_term_key(x), normalize_term_key(y));
            (kx == ka && ky == kb) || (kx == kb && ky == ka)
        });
    if already {
        return ToolOutcome::ok_msg(format!(
            "Pair '{a}' vs '{b}' is already tracked — adjudicate it per the ⚠ pair rules before submit_result."
        ));
    }
    if ctx.declared_pairs.len() >= MAX_DECLARED_PAIRS {
        return ToolOutcome::err(format!(
            "Error: declared-pair budget exhausted ({MAX_DECLARED_PAIRS}). Adjudicate the pairs already flagged."
        ));
    }
    ctx.declared_pairs.push((a.clone(), b.clone()));
    let mut out = format!(
        "Flagged suspect pair: '{a}' vs '{b}'. The SAME adjudication rules as ⚠ pairs apply at submit_result \
         (map both to one target / exclude the suspect / cross-named notes / verified quote + web when enabled).\nEvidence:\n"
    );
    for surface in [&a, &b] {
        out.push_str(&format!(
            "'{surface}':\n{}\n",
            super::candidates::surface_example_lines(surface, ctx.cues, 2).join("\n")
        ));
    }
    ToolOutcome::ok_msg(out)
}

pub fn tool_web_search_preflight(args: &Value, ctx: &mut AgentToolContext<'_>) -> ToolOutcome {
    let query = string_arg(args, &["query"]).trim().to_string();
    if query.chars().count() < MIN_QUERY_CHARS {
        return ToolOutcome::err(format!(
            "Error: query too short (min {MIN_QUERY_CHARS} chars). Use a short name/abbreviation from THIS video."
        ));
    }
    if query.chars().count() > MAX_QUERY_CHARS {
        return ToolOutcome::err(format!(
            "Error: query too long (max {MAX_QUERY_CHARS} chars). Do not dump the transcript."
        ));
    }
    if ctx.web_search_count >= WEB_SEARCH_BUDGET {
        return ToolOutcome::err(format!(
            "Error: web_search budget exhausted ({WEB_SEARCH_BUDGET}). Reason from transcript + title + knowledge instead."
        ));
    }
    // Log the query: the submit gate requires distinct-concept claims on ⚠
    // pairs to be backed by a domain-qualified lookup.
    ctx.web_queries.push(query.to_lowercase());
    ToolOutcome::ok_msg(format!("WEB_PENDING\t{query}"))
}

pub fn truncate_web_result(text: &str) -> String {
    if text.chars().count() <= MAX_WEB_RESULT_CHARS {
        return text.to_string();
    }
    text.chars().take(MAX_WEB_RESULT_CHARS).collect::<String>() + "…"
}

/// Web search provider selection. AnySearch (api.anysearch.com, needs
/// ANYSEARCH_API_KEY) wins when configured; otherwise Parallel Search's
/// hosted MCP endpoint, which answers unauthenticated (rate-limited) —
/// PARALLEL_API_KEY attaches a Bearer token for higher limits.
/// VOXTRANS_WEB_SEARCH=off disables the tool entirely (privacy: queries
/// contain a few words from the video).
#[derive(Debug, Clone)]
pub enum WebSearchProvider {
    Parallel { key: Option<String> },
    AnySearch { key: String },
}

pub fn web_search_provider(settings_anysearch_key: &str) -> Option<WebSearchProvider> {
    if std::env::var("VOXTRANS_WEB_SEARCH")
        .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "off" | "0" | "false"))
        .unwrap_or(false)
    {
        return None;
    }
    let env_val = |name: &str| {
        std::env::var(name)
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    };
    // Priority: settings UI key > env key > Parallel free endpoint.
    let key = if settings_anysearch_key.trim().is_empty() {
        env_val("ANYSEARCH_API_KEY")
    } else {
        Some(settings_anysearch_key.trim().to_string())
    };
    if let Some(key) = key {
        return Some(WebSearchProvider::AnySearch { key });
    }
    Some(WebSearchProvider::Parallel {
        key: env_val("PARALLEL_API_KEY"),
    })
}

fn http_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())
}

const SEARCH_FALLBACK_HINT: &str = "Reason from transcript + title + knowledge instead.";

pub async fn execute_web_search(query: &str, session_id: &str, provider: &WebSearchProvider) -> String {
    let result = match provider {
        WebSearchProvider::Parallel { key } => {
            parallel_search(query, session_id, key.as_deref()).await
        }
        WebSearchProvider::AnySearch { key } => anysearch_search(query, key).await,
    };
    match result {
        Ok(text) if !text.trim().is_empty() => format!(
            "[web_search '{query}']\n{}",
            truncate_web_result(&text)
        ),
        Ok(_) => format!("[web_search: no results]. {SEARCH_FALLBACK_HINT}"),
        Err(e) => format!("[web_search failed: {e}]. {SEARCH_FALLBACK_HINT}"),
    }
}

async fn parallel_search(
    query: &str,
    session_id: &str,
    api_key: Option<&str>,
) -> Result<String, String> {
    let url = "https://search.parallel.ai/mcp";
    // Parallel best practice: objective carries the research goal plus source
    // guidance; search_queries stay short keywords (one is the accepted minimum).
    let objective = format!(
        "Verify the conventional meaning of '{query}' as used in a video transcript. \
         Prefer encyclopedic or glossary sources over transcript/forum pages, which may repeat a transcription error."
    );
    let body = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {
            "name": "web_search",
            "arguments": {
                "objective": objective,
                "search_queries": [query],
                "session_id": session_id,
            }
        }
    });
    let client = http_client()?;
    let mut req = client
        .post(url)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .header("User-Agent", "VoxTrans/1.4.3")
        .json(&body);
    if let Some(key) = api_key {
        req = req.header("Authorization", format!("Bearer {key}"));
    }
    let raw = req
        .send()
        .await
        .map_err(|e| e.to_string())?
        .text()
        .await
        .unwrap_or_default();
    Ok(extract_mcp_text(&raw))
}

async fn anysearch_search(query: &str, api_key: &str) -> Result<String, String> {
    let client = http_client()?;
    let raw = client
        .post("https://api.anysearch.com/v1/search")
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {api_key}"))
        // extract_anysearch_text keeps the top 6; ask for exactly that
        // (API default is 10) to keep the payload lean.
        .json(&json!({ "query": query, "max_results": 6 }))
        .send()
        .await
        .map_err(|e| e.to_string())?
        .text()
        .await
        .unwrap_or_default();
    extract_anysearch_text(&raw)
}

/// AnySearch returns `{code, data: {results: [{title,url,snippet,content}]}}`.
/// Keep the top few results; transcripts dumps may carry the same ASR error
/// the agent is trying to adjudicate, so titles/URLs stay visible as hints.
pub fn extract_anysearch_text(raw: &str) -> Result<String, String> {
    let data: Value = serde_json::from_str(raw.trim()).map_err(|e| e.to_string())?;
    if data.get("code").and_then(|c| c.as_i64()) != Some(0) {
        let msg = data
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("unknown error");
        return Err(msg.to_string());
    }
    let Some(results) = data.pointer("/data/results").and_then(|r| r.as_array()) else {
        return Ok(String::new());
    };
    let mut blocks = Vec::new();
    for r in results.iter().take(6) {
        let title = r.get("title").and_then(|v| v.as_str()).unwrap_or("").trim();
        let url = r.get("url").and_then(|v| v.as_str()).unwrap_or("").trim();
        let snippet = r
            .get("snippet")
            .and_then(|v| v.as_str())
            .or_else(|| r.get("content").and_then(|v| v.as_str()))
            .unwrap_or("")
            .trim();
        if snippet.is_empty() {
            continue;
        }
        blocks.push(format!("- {title}\n  {url}\n  {snippet}"));
    }
    Ok(blocks.join("\n"))
}

pub fn extract_mcp_text(raw: &str) -> String {
    let mut candidates = vec![raw.trim().to_string()];
    for line in raw.lines() {
        if let Some(rest) = line.strip_prefix("data: ") {
            candidates.push(rest.trim().to_string());
        }
    }
    for candidate in candidates {
        if !candidate.starts_with('{') {
            continue;
        }
        let Ok(data) = serde_json::from_str::<Value>(&candidate) else {
            continue;
        };
        let Some(content) = data
            .pointer("/result/content")
            .and_then(|v| v.as_array())
        else {
            continue;
        };
        for item in content {
            if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
                if !text.trim().is_empty() {
                    return text.to_string();
                }
            }
        }
    }
    String::new()
}

pub fn tool_submit_result(args: &Value, ctx: &mut AgentToolContext<'_>) -> ToolOutcome {
    let glossary_val = args
        .get("glossary")
        .or_else(|| args.get("terms"))
        .cloned();
    let style_val = args
        .get("style_guide")
        .or_else(|| args.get("style"))
        .cloned();
    if glossary_val.is_none() && style_val.is_none() {
        return ToolOutcome::err(
            "Error: submit_result needs glossary (list) and style_guide (string). Empty glossary [] is OK if style_guide is non-empty. Hint: {\"glossary\":[{\"source\":\"...\",\"target\":\"...\"}],\"style_guide\":\"2-4 sentences\"}",
        );
    }
    if let Some(g) = &glossary_val {
        if !g.is_array() {
            return ToolOutcome::err(
                "Error: glossary must be a JSON array (use [] if none).",
            );
        }
    }
    if let Some(s) = &style_val {
        if !s.is_null() && !s.is_string() {
            return ToolOutcome::err("Error: style_guide must be a string.");
        }
    }
    let (mut glossary, dropped) = clean_glossary(glossary_val.as_ref());
    let mut style = style_val
        .as_ref()
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if glossary.is_empty() && style.is_empty() {
        return ToolOutcome::err(
            "Error: submit at least a non-empty style_guide or glossary entries.",
        );
    }

    // Hard gate: every flagged confusable pair must be consciously
    // adjudicated. A pair is resolved when at least one surface is absent
    // (excluded — the safe call for a suspected mishearing), or at least one
    // kept note names the other surface, or BOTH kept notes declare an ASR
    // mishearing (two surfaces each mapped onto a canonical term have both
    // been adjudicated — demanding they also cross-name each other just
    // thrashes the model). When both surfaces are kept with DIFFERENT
    // targets and neither declares a mishearing, the model claims two
    // distinct concepts — that claim must be grounded in at least one note
    // quoting (in "double quotes", >= 12 chars) a verbatim transcript line,
    // verified here against the cues. Same target on both = mishearing
    // mapping, no quote needed. This blocks invented distinctions without
    // demanding symmetric paperwork the model fumbles.
    // Deterministic ⚠ pairs plus pairs the model flagged itself via
    // flag_pair: identical adjudication rules for both.
    let all_pairs: Vec<(String, String)> = ctx
        .confusable_pairs
        .iter()
        .chain(ctx.declared_pairs.iter())
        .cloned()
        .collect();
    let transcript_lc = ctx
        .cues
        .iter()
        .map(|c| c.text.to_lowercase())
        .collect::<Vec<_>>()
        .join("\n");
    let kept: std::collections::HashMap<String, (String, String)> = glossary
        .iter()
        .map(|g| {
            (
                normalize_term_key(&g.source),
                (g.target.trim().to_lowercase(), g.note.to_lowercase()),
            )
        })
        .collect();
    let mut unresolved: Vec<String> = Vec::new();
    let mut ungrounded: Vec<String> = Vec::new();
    let mut unverified: Vec<String> = Vec::new();
    let mut failing: Vec<(String, String)> = Vec::new();
    for (a, b) in &all_pairs {
        let (Some((ta, note_a)), Some((tb, note_b))) = (
            kept.get(&normalize_term_key(a)),
            kept.get(&normalize_term_key(b)),
        ) else {
            continue; // at least one side excluded — pair resolved
        };
        let declares_a = note_a.contains("mishearing");
        let declares_b = note_b.contains("mishearing");
        let named = note_names_surface(note_a, b) || note_names_surface(note_b, a);
        if !named && !(declares_a && declares_b) {
            unresolved.push(format!("'{a}' vs '{b}'"));
            failing.push((a.clone(), b.clone()));
            continue;
        }
        if ta != tb && !(declares_a || declares_b) {
            let quoted = has_verified_quote(note_a, &transcript_lc)
                || has_verified_quote(note_b, &transcript_lc);
            if !quoted {
                ungrounded.push(format!("'{a}' vs '{b}'"));
                failing.push((a.clone(), b.clone()));
                continue;
            }
            // A distinct-concept claim on an ASR-suspicious pair must also be
            // web-verified (when the tool exists): at least one past query
            // must cover a surface of the pair. Never searched -> the claim
            // rests on the model's own confidence, which is not evidence.
            let web_covered = !ctx.web_search_enabled
                || ctx.web_queries.iter().any(|q| {
                    q.contains(&a.to_lowercase()) || q.contains(&b.to_lowercase())
                });
            if !web_covered {
                unverified.push(format!("'{a}' vs '{b}'"));
                failing.push((a.clone(), b.clone()));
            }
        }
    }
    // Style-guide backdoor: a ⚠ surface left OUT of the glossary must not be
    // taught a rendering in style_guide — free text the pair gate cannot see,
    // but translators read it and it has poisoned real output. Mentioning an
    // excluded surface is allowed only when style_guide explicitly frames it
    // as ASR noise (the word "ASR" appears).
    let style_violations = excluded_pair_surfaces_in_style(&all_pairs, &kept, &style);
    let reject_sig = format!(
        "u={};g={};w={}",
        unresolved.join(","),
        ungrounded.join(","),
        unverified.join(",")
    );
    if !failing.is_empty() {
        // Salvage stop-loss: a pair the model cannot adjudicate after
        // repeated attempts is resolved by EXCLUSION of BOTH surfaces. A
        // missing row does no damage; a wrong row enforces a fabricated
        // distinction on every matching line. Frequency is deliberately NOT
        // used to pick a survivor — a mishearing can out-occur the real term.
        if ctx.submit_rejects.len() >= 2 {
            let mut dropped: Vec<String> = Vec::new();
            for (a, b) in &failing {
                for surface in [a, b] {
                    let key = normalize_term_key(surface);
                    if dropped.iter().any(|d| d == surface) {
                        continue;
                    }
                    let before = glossary.len();
                    glossary.retain(|g| normalize_term_key(&g.source) != key);
                    if glossary.len() < before {
                        dropped.push((*surface).clone());
                    }
                }
            }
            // The style guide must not keep teaching what the salvage dropped
            // — nor any other ⚠ surface still excluded after the drop (the
            // pair gate's style backdoor check). Sentences that frame the
            // surface as ASR noise stay; unframed teaching is removed.
            let mut strip_surfaces: Vec<String> = dropped.clone();
            let kept_after: std::collections::HashMap<String, (String, String)> = glossary
                .iter()
                .map(|g| {
                    (
                        normalize_term_key(&g.source),
                        (g.target.trim().to_lowercase(), g.note.to_lowercase()),
                    )
                })
                .collect();
            for surface in
                excluded_pair_surfaces_in_style(&all_pairs, &kept_after, &style)
            {
                if !strip_surfaces.iter().any(|s| s == &surface) {
                    strip_surfaces.push(surface);
                }
            }
            let mut strip_note = String::new();
            if !strip_surfaces.is_empty() {
                let (new_style, removed) = strip_style_mentions(&style, &strip_surfaces);
                if removed > 0 {
                    style = new_style;
                    strip_note = format!(
                        " Removed {removed} style_guide sentence(s) naming excluded surface(s)."
                    );
                }
            }
            ctx.final_glossary = Some(glossary.clone());
            ctx.final_style = Some(style.clone());
            return ToolOutcome::submit_ok(format!(
                "Briefing submitted WITH SALVAGE: dropped {} row(s) from ⚠ pair(s) still unresolved after repeated attempts: {}. \
                 Exclusion is the safe call; add them next time via mapping/exclusion notes. glossary={}, style_guide={} chars.{strip_note} Finalizing.",
                dropped.len(),
                dropped.iter().map(|s| format!("'{s}'")).collect::<Vec<_>>().join(", "),
                glossary.len(),
                style.chars().count()
            ));
        }
        ctx.submit_rejects.push(reject_sig);
    }
    if !unresolved.is_empty() {
        return ToolOutcome::err(format!(
            "Error: unresolved confusable pair(s): {}. For EACH pair: exclude the suspect surface (leave it out), \
             or keep it with a note naming the other surface and the call (\"likely ASR mishearing of 'X'\" / \"'X' excluded as likely mishearing\" / \"distinct from 'X'\"). \
             One side naming the other is enough; two surfaces each declaring a mishearing of the same canonical term also resolves the pair. \
             Do not resubmit the same call with reworded notes — change the CALL or add what was missing. \
             Repeated rejected attempts trigger salvage: BOTH surfaces of the still-failing pair(s) get dropped; the rest is accepted.\
             {}",
            unresolved.join("; "),
            rejection_evidence(&unresolved, ctx)
        ));
    }
    if !ungrounded.is_empty() {
        return ToolOutcome::err(format!(
            "Error: ungrounded distinction claim(s): {}. Both surfaces are kept with DIFFERENT targets, which claims they are distinct concepts — \
             at least one note must quote (in \"double quotes\", 12+ chars) the exact transcript line showing the distinct usage; quotes are verified against the transcript. \
             If these contexts are interchangeable, they are the SAME concept: give both rows the same target (mishearing mapping) or exclude the suspect surface. \
             A plausible-sounding interpretation you invented is NOT evidence. Never invent a distinction.\
             {}",
            ungrounded.join("; "),
            rejection_evidence(&ungrounded, ctx)
        ));
    }

    if !unverified.is_empty() {
        return ToolOutcome::err(format!(
            "Error: unverified distinction claim(s): {}. Keeping both surfaces with DIFFERENT targets claims they are distinct real terms — \
             an ASR-suspicious pair needs external confirmation: run web_search with a domain-qualified query covering the surface (\"<surface>\" <domain keywords from this video>) BEFORE submitting. \
             If the search finds the exact phrase used as a term, that supports the distinction; if it finds nothing used as a term, that is a strong mishearing signal — map or exclude instead.",
            unverified.join("; ")
        ));
    }

    if !style_violations.is_empty() {
        return ToolOutcome::err(format!(
            "Error: style_guide teaches excluded ⚠ surface(s): {}. A surface left out of the glossary must not get a rendering in style_guide — \
             remove the mention, or explicitly mark it as an excluded ASR mishearing (the sentence mentioning it must contain an ASR-noise framing, e.g. the word 'ASR').",
            style_violations
                .iter()
                .map(|s| format!("'{s}'"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    ctx.final_glossary = Some(glossary.clone());
    ctx.final_style = Some(style.clone());
    let drop_msg = if dropped > 0 {
        format!(" ({dropped} invalid/duplicate glossary rows dropped)")
    } else {
        String::new()
    };
    ToolOutcome::submit_ok(format!(
        "Briefing submitted: glossary={}{drop_msg}, style_guide={} chars. Finalizing.",
        glossary.len(),
        style.chars().count()
    ))
}

/// ⚠-pair surfaces absent from the glossary that style_guide nevertheless
/// teaches a rendering for — the backdoor around the pair gate. ASR-noise
/// framing is judged PER SENTENCE: mentioning an excluded surface is allowed
/// only when the sentence doing so marks it as ASR noise. An "ASR" mention
/// elsewhere in the guide must not exempt unrelated teaching — the old
/// global check let real teaching escape both this gate and the salvage
/// strip (observed in production).
fn excluded_pair_surfaces_in_style(
    pairs: &[(String, String)],
    kept: &std::collections::HashMap<String, (String, String)>,
    style: &str,
) -> Vec<String> {
    let mut out = Vec::new();
    for (a, b) in pairs {
        for surface in [a, b] {
            if kept.contains_key(&normalize_term_key(surface)) {
                continue;
            }
            let surf_lc = surface.to_lowercase();
            let taught = style_sentences(style)
                .any(|s| s.contains(&surf_lc) && !s.contains("asr"));
            if taught && !out.iter().any(|x| x == surface) {
                out.push(surface.clone());
            }
        }
    }
    out
}

/// Lowercased sentences of a style guide. Boundaries mirror
/// strip_style_mentions: CJK/Latin sentence punctuation and newlines.
fn style_sentences(style: &str) -> impl Iterator<Item = String> + '_ {
    style
        .split_inclusive(['。', '.', '!', '?', '！', '？', '\n'])
        .map(|s| s.to_lowercase())
}

/// Remove style_guide sentences that teach a rendering for any of
/// `surfaces` without ASR-noise framing in the SAME sentence; returns the
/// cleaned text and how many sentences were removed. Sentences split on
/// CJK/Latin sentence punctuation and newlines.
fn strip_style_mentions(style: &str, surfaces: &[String]) -> (String, usize) {
    let mut removed = 0;
    let kept: Vec<&str> = style
        .split_inclusive(['。', '.', '!', '?', '！', '？', '\n'])
        .filter(|sentence| {
            let s = sentence.to_lowercase();
            let hit = surfaces
                .iter()
                .any(|surface| s.contains(&surface.to_lowercase()))
                && !s.contains("asr");
            if hit {
                removed += 1;
            }
            !hit
        })
        .collect();
    (kept.concat().trim().to_string(), removed)
}

/// Attach the verbatim transcript evidence for rejected pairs to the error,
/// so the model re-adjudicates from the actual lines instead of rewording
/// the same claim. `pairs` entries are formatted "'a' vs 'b'".
fn rejection_evidence(pairs: &[String], ctx: &AgentToolContext<'_>) -> String {
    let mut out = String::from("\nTranscript evidence for the flagged pair(s):");
    for pair in pairs {
        let mut surfaces = pair.split("' vs '").map(|s| s.trim_matches('\''));
        let (Some(a), Some(b)) = (surfaces.next(), surfaces.next()) else {
            continue;
        };
        out.push_str(&format!("\n'{a}':"));
        for line in super::candidates::surface_example_lines(a, ctx.cues, 2) {
            out.push_str(&format!("\n{line}"));
        }
        out.push_str(&format!("\n'{b}':"));
        for line in super::candidates::surface_example_lines(b, ctx.cues, 2) {
            out.push_str(&format!("\n{line}"));
        }
    }
    out
}

/// A note "names" a surface when it contains all of the surface's tokens
/// (order-free) — accepts compressions like "hard/high time frame" that a
/// literal substring check would miss.
fn note_names_surface(note_lc: &str, surface: &str) -> bool {
    let tokens: std::collections::HashSet<&str> = note_lc
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .collect();
    surface
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .all(|t| tokens.contains(t))
}

/// Extract a quoted span (12+ chars) from a note and verify it appears
/// verbatim in the (lowercased) transcript. Anti-fabrication check for
/// claims that two confusable surfaces are distinct concepts. Accepts
/// "straight", 'single', "curly" and 「corner」 quotes — the safety property
/// comes from transcript verification, not from the quote character, and
/// weaker models frequently pick the wrong one (observed in production:
/// repeated salvage after single-quoted verbatim evidence).
const MIN_QUOTE_CHARS: usize = 12;

fn has_verified_quote(note_lc: &str, transcript_lc: &str) -> bool {
    const PAIRS: [(char, char); 4] = [('"', '"'), ('\'', '\''), ('“', '”'), ('「', '」')];
    PAIRS.iter().any(|&(open, close)| {
        let mut rest = note_lc;
        while let Some(start) = rest.find(open) {
            let after = &rest[start + open.len_utf8()..];
            let Some(end) = after.find(close) else { break };
            let span = after[..end].trim();
            if span.chars().count() >= MIN_QUOTE_CHARS && transcript_lc.contains(span) {
                return true;
            }
            rest = &after[end + close.len_utf8()..];
        }
        false
    })
}

pub fn clean_glossary(raw: Option<&Value>) -> (Vec<GlossaryEntry>, usize) {
    let Some(Value::Array(arr)) = raw else {
        return (Vec::new(), 0);
    };
    let mut cleaned = Vec::new();
    let mut dropped = 0usize;
    let mut seen = std::collections::HashSet::new();
    for item in arr {
        let Some(obj) = item.as_object() else {
            dropped += 1;
            continue;
        };
        let src = obj
            .get("source")
            .or_else(|| obj.get("origin"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        let tgt = obj
            .get("target")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        if src.is_empty() || tgt.is_empty() {
            dropped += 1;
            continue;
        }
        let key = normalize_term_key(src);
        if key.is_empty() || !seen.insert(key) {
            dropped += 1;
            continue;
        }
        let note = obj
            .get("note")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .chars()
            .take(200)
            .collect::<String>();
        cleaned.push(GlossaryEntry::new(src, tgt, note));
        if cleaned.len() >= MAX_GLOSSARY_ROWS {
            dropped += arr.len().saturating_sub(cleaned.len() + dropped);
            break;
        }
    }
    (cleaned, dropped)
}

/// Keep only rows whose source appears in the transcript (agent rows).
fn format_mmss(ms: u64) -> String {
    let s = ms / 1000;
    format!("{:02}:{:02}", s / 60, s % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cues() -> Vec<TranscriptCue> {
        vec![
            TranscriptCue {
                index: 1,
                start_ms: 1000,
                text: "Welcome to Last Call Canyon".into(),
            },
            TranscriptCue {
                index: 2,
                start_ms: 5000,
                text: "Last Call Canyon is the map".into(),
            },
        ]
    }

    #[test]
    fn search_finds_phrase_with_timestamp() {
        let out = tool_search_transcript(
            &json!({"pattern":"Last Call","ignore_case":true}),
            &cues(),
        );
        assert!(out.ok);
        assert!(out.message.contains("2x  'Last Call'"));
        assert!(out.message.contains("examples:"));
        assert!(out.message.contains("#1"));
    }

    #[test]
    fn search_samples_first_and_last_when_many_hits() {
        let cues: Vec<TranscriptCue> = (1..=8)
            .map(|i| TranscriptCue {
                index: i,
                start_ms: i as u64 * 1000,
                text: format!("order block {i}"),
            })
            .collect();
        let out = tool_search_transcript(&json!({"pattern":"order block"}), &cues);
        assert!(out.message.contains("8x"));
        assert!(out.message.contains("#1"));
        assert!(out.message.contains("#8"));
        let shown = out.message.matches("[#").count();
        assert_eq!(shown, 3);
    }

    #[test]
    fn count_reports_frequency() {
        let out = tool_count_transcript(&json!({"terms":["Canyon","zzz"]}), &cues());
        assert!(out.ok);
        assert!(out.message.contains("2x  'Canyon'"));
        assert!(out.message.contains("0x  'zzz'"));
    }

    #[test]
    fn submit_rejects_empty_and_accepts_style_only() {
        let cues = cues();
        let mut ctx = AgentToolContext {
            cues: &cues,
            web_search_count: 0,
            web_search_enabled: false,
            web_queries: Vec::new(),
            submit_rejects: Vec::new(),
            final_glossary: None,
            final_style: None,
            confusable_pairs: &[],
            declared_pairs: Vec::new(),
        };
        let empty = tool_submit_result(&json!({"glossary":[],"style_guide":""}), &mut ctx);
        assert!(!empty.ok);
        let ok = tool_submit_result(
            &json!({"glossary":[],"style_guide":"Keep names in English."}),
            &mut ctx,
        );
        assert!(ok.terminate);
        assert_eq!(ctx.final_style.as_deref(), Some("Keep names in English."));
    }

    #[test]
    fn submit_accepts_glossary_without_web_search() {
        let cues = cues();
        let mut ctx = AgentToolContext {
            cues: &cues,
            web_search_count: 0,
            web_search_enabled: false,
            web_queries: Vec::new(),
            submit_rejects: Vec::new(),
            final_glossary: None,
            final_style: None,
            confusable_pairs: &[],
            declared_pairs: Vec::new(),
        };
        let args = json!({
            "glossary":[{"source":"Canyon","target":"峡谷"}],
            "style_guide":"Use 峡谷."
        });
        let first = tool_submit_result(&args, &mut ctx);
        assert!(first.terminate);
        assert_eq!(ctx.final_glossary.as_ref().unwrap()[0].target, "峡谷");
    }

    #[test]
    fn submit_gate_blocks_unresolved_confusable_pairs() {
        let cues = cues();
        let pairs = vec![
            ("hard time frame".to_string(), "high time frame".to_string()),
        ];
        let mut ctx = AgentToolContext {
            cues: &cues,
            web_search_count: 0,
            web_search_enabled: false,
            web_queries: Vec::new(),
            submit_rejects: Vec::new(),
            final_glossary: None,
            final_style: None,
            confusable_pairs: &pairs,
            declared_pairs: Vec::new(),
        };
        // Only one surface kept: the pair is resolved by exclusion on the
        // other side — no note needed.
        let one_side = tool_submit_result(
            &json!({
                "glossary":[{"source":"hard time frame","target":"高周期"}],
                "style_guide":"s"
            }),
            &mut ctx,
        );
        assert!(one_side.terminate, "{}", one_side.message);
        // Both kept but neither note names the other -> repairable rejection.
        let mute = tool_submit_result(
            &json!({
                "glossary":[
                    {"source":"hard time frame","target":"甲"},
                    {"source":"high time frame","target":"甲"}
                ],
                "style_guide":"s"
            }),
            &mut ctx,
        );
        assert!(!mute.ok && mute.repairable && !mute.terminate);
        assert!(mute.message.contains("high time frame"));
    }

    #[test]
    fn submit_gate_passes_when_pair_excluded_or_notes_cross_named() {
        let cues = cues();
        let pairs = vec![("foo bar".to_string(), "foo baz".to_string())];
        let mut ctx = AgentToolContext {
            cues: &cues,
            web_search_count: 0,
            web_search_enabled: false,
            web_queries: Vec::new(),
            submit_rejects: Vec::new(),
            final_glossary: None,
            final_style: None,
            confusable_pairs: &pairs,
            declared_pairs: Vec::new(),
        };
        // Neither surface kept -> adjudicated by exclusion.
        let none = tool_submit_result(&json!({"glossary":[],"style_guide":"s"}), &mut ctx);
        assert!(none.terminate);
        // Both kept as a mishearing mapping (same target, one note naming
        // the other) -> passes without quotes.
        let mapped = tool_submit_result(
            &json!({
                "glossary":[
                    {"source":"foo bar","target":"甲","note":"likely ASR mishearing of 'foo baz'"},
                    {"source":"foo baz","target":"甲"}
                ],
                "style_guide":"s"
            }),
            &mut ctx,
        );
        assert!(mapped.terminate, "{}", mapped.message);
        // Both kept with DIFFERENT targets, one side names the other but no
        // verified transcript quote anywhere -> ungrounded distinction.
        let invented = tool_submit_result(
            &json!({
                "glossary":[
                    {"source":"foo bar","target":"甲"},
                    {"source":"foo baz","target":"乙","note":"distinct from 'foo bar'"}
                ],
                "style_guide":"s"
            }),
            &mut ctx,
        );
        assert!(!invented.ok && invented.repairable);
        assert!(invented.message.contains("ungrounded"));
        // Both kept, different targets, one note quoting a verbatim
        // transcript line -> distinction is grounded, passes.
        let grounded = tool_submit_result(
            &json!({
                "glossary":[
                    {"source":"foo bar","target":"甲","note":"distinct from 'foo baz': \"Welcome to Last Call Canyon\""},
                    {"source":"foo baz","target":"乙"}
                ],
                "style_guide":"s"
            }),
            &mut ctx,
        );
        assert!(grounded.terminate, "{}", grounded.message);
        // A fabricated quote (not in the transcript) -> rejected.
        let fake = tool_submit_result(
            &json!({
                "glossary":[
                    {"source":"foo bar","target":"甲","note":"distinct from 'foo baz': \"Welcome to Last Call Canyon\""},
                    {"source":"foo baz","target":"乙","note":"distinct from 'foo bar': \"this line was never said at all\""}
                ],
                "style_guide":"s"
            }),
            &mut ctx,
        );
        assert!(fake.terminate, "one verified quote is enough: {}", fake.message);
        // A fabricated quote (not in the transcript) -> rejected. Fresh
        // rejection budget: the earlier "invented" case has the same
        // signature and would otherwise trigger salvage.
        ctx.submit_rejects.clear();
        let only_fake = tool_submit_result(
            &json!({
                "glossary":[
                    {"source":"foo bar","target":"甲","note":"distinct from 'foo baz': \"this line was never said at all\""},
                    {"source":"foo baz","target":"乙"}
                ],
                "style_guide":"s"
            }),
            &mut ctx,
        );
        assert!(!only_fake.ok && only_fake.repairable);
        // Both kept but NEITHER note names the partner -> rejected.
        let sloppy = tool_submit_result(
            &json!({
                "glossary":[
                    {"source":"foo bar","target":"甲"},
                    {"source":"foo baz","target":"乙"}
                ],
                "style_guide":"s"
            }),
            &mut ctx,
        );
        assert!(!sloppy.ok && sloppy.repairable);
        // Both kept, each declaring a mishearing of the same canonical term,
        // no cross-naming -> pair resolved (both adjudicated via mapping).
        let both_mapped = tool_submit_result(
            &json!({
                "glossary":[
                    {"source":"foo bar","target":"甲","note":"likely ASR mishearing of 'foo bar baz'"},
                    {"source":"foo baz","target":"甲","note":"likely ASR mishearing of 'foo bar baz'"}
                ],
                "style_guide":"s"
            }),
            &mut ctx,
        );
        assert!(both_mapped.terminate, "{}", both_mapped.message);
    }

    #[test]
    fn submit_gate_salvages_after_repeated_rejections() {
        let cues = cues();
        let pairs = vec![("foo bar".to_string(), "foo baz".to_string())];
        let mut ctx = AgentToolContext {
            cues: &cues,
            web_search_count: 0,
            web_search_enabled: false,
            web_queries: Vec::new(),
            submit_rejects: Vec::new(),
            final_glossary: None,
            final_style: None,
            confusable_pairs: &pairs,
            declared_pairs: Vec::new(),
        };
        let attempt = || {
            json!({
                "glossary":[
                    {"source":"foo bar","target":"甲"},
                    {"source":"foo baz","target":"乙"},
                    {"source":"Canyon","target":"峡谷"}
                ],
                "style_guide":"s"
            })
        };
        // Same failing signature twice -> rejected both times...
        let first = tool_submit_result(&attempt(), &mut ctx);
        assert!(!first.ok && first.repairable);
        let second = tool_submit_result(&attempt(), &mut ctx);
        assert!(!second.ok && second.repairable);
        // ...and the third rejection salvages: BOTH surfaces of the failing
        // pair are dropped, the rest is accepted and terminates.
        let third = tool_submit_result(&attempt(), &mut ctx);
        assert!(third.terminate, "{}", third.message);
        assert!(third.message.contains("SALVAGE"));
        let g = ctx.final_glossary.as_ref().unwrap();
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].source, "Canyon");
    }

    #[test]
    fn submit_gate_rejects_style_guide_teaching_excluded_surface() {
        let cues = cues();
        let pairs = vec![("foo bar".to_string(), "foo baz".to_string())];
        let mut ctx = AgentToolContext {
            cues: &cues,
            web_search_count: 0,
            web_search_enabled: false,
            web_queries: Vec::new(),
            submit_rejects: Vec::new(),
            final_glossary: None,
            final_style: None,
            confusable_pairs: &pairs,
            declared_pairs: Vec::new(),
        };
        // 'foo baz' excluded from the glossary but taught in style_guide,
        // without any ASR-noise framing -> rejected.
        let teaching = tool_submit_result(
            &json!({
                "glossary":[{"source":"foo bar","target":"甲"}],
                "style_guide":"把 foo baz 统一译为「乙」，保持全片一致。"
            }),
            &mut ctx,
        );
        assert!(!teaching.ok && teaching.repairable, "{}", teaching.message);
        assert!(teaching.message.contains("style_guide"), "{}", teaching.message);
        // Same mention, explicitly framed as ASR noise -> allowed.
        let marked = tool_submit_result(
            &json!({
                "glossary":[{"source":"foo bar","target":"甲"}],
                "style_guide":"foo baz 是 ASR 误听，按上下文处理。"
            }),
            &mut ctx,
        );
        assert!(marked.terminate, "{}", marked.message);
    }

    #[test]
    fn salvage_strips_style_sentences_naming_dropped_surfaces() {
        let cues = cues();
        let pairs = vec![("foo bar".to_string(), "foo baz".to_string())];
        let mut ctx = AgentToolContext {
            cues: &cues,
            web_search_count: 0,
            web_search_enabled: false,
            web_queries: Vec::new(),
            submit_rejects: Vec::new(),
            final_glossary: None,
            final_style: None,
            confusable_pairs: &pairs,
            declared_pairs: Vec::new(),
        };
        let attempt = || {
            json!({
                "glossary":[
                    {"source":"foo bar","target":"甲"},
                    {"source":"foo baz","target":"乙"},
                    {"source":"Canyon","target":"峡谷"}
                ],
                "style_guide":"语气保持口语化。foo bar 译为「甲」，foo baz 译为「乙」，两者不可混译。"
            })
        };
        tool_submit_result(&attempt(), &mut ctx);
        tool_submit_result(&attempt(), &mut ctx);
        let third = tool_submit_result(&attempt(), &mut ctx);
        assert!(third.terminate, "{}", third.message);
        assert!(third.message.contains("SALVAGE"));
        let style = ctx.final_style.as_ref().unwrap();
        // The sentence teaching the dropped surfaces is gone; the unrelated
        // sentence survives.
        assert!(!style.contains("foo bar"), "style={style}");
        assert!(!style.contains("foo baz"), "style={style}");
        assert!(style.contains("口语化"), "style={style}");
    }

    #[test]
    fn salvage_strips_style_sentences_naming_other_excluded_surfaces() {
        let cues = cues();
        // Second pair: only 'alpha' is kept — 'alpha2' is excluded from the
        // glossary but taught in style_guide. The pair resolves by exclusion,
        // so it never fails; the salvage path must still strip that mention,
        // not just sentences naming the dropped failing pair.
        let pairs = vec![
            ("foo bar".to_string(), "foo baz".to_string()),
            ("alpha".to_string(), "alpha2".to_string()),
        ];
        let mut ctx = AgentToolContext {
            cues: &cues,
            web_search_count: 0,
            web_search_enabled: false,
            web_queries: Vec::new(),
            submit_rejects: Vec::new(),
            final_glossary: None,
            final_style: None,
            confusable_pairs: &pairs,
            declared_pairs: Vec::new(),
        };
        let attempt = || {
            json!({
                "glossary":[
                    {"source":"foo bar","target":"甲"},
                    {"source":"foo baz","target":"乙"},
                    {"source":"alpha","target":"甲"}
                ],
                "style_guide":"foo bar 译为「甲」。alpha2 译为「乙」。整体保持口语化。"
            })
        };
        tool_submit_result(&attempt(), &mut ctx);
        tool_submit_result(&attempt(), &mut ctx);
        let third = tool_submit_result(&attempt(), &mut ctx);
        assert!(third.terminate, "{}", third.message);
        assert!(third.message.contains("SALVAGE"));
        let style = ctx.final_style.as_ref().unwrap();
        // Sentences teaching dropped AND still-excluded surfaces are gone;
        // the unrelated sentence survives.
        assert!(!style.contains("foo bar"), "style={style}");
        assert!(!style.contains("foo baz"), "style={style}");
        assert!(!style.contains("alpha2"), "style={style}");
        assert!(style.contains("口语化"), "style={style}");
        let g = ctx.final_glossary.as_ref().unwrap();
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].source, "alpha");
    }

    #[test]
    fn submit_gate_requires_web_verification_for_distinct_claims_when_web_enabled() {
        let cues = cues();
        let pairs = vec![("foo bar".to_string(), "foo baz".to_string())];
        let mut ctx = AgentToolContext {
            cues: &cues,
            web_search_count: 0,
            web_search_enabled: true,
            web_queries: Vec::new(),
            submit_rejects: Vec::new(),
            final_glossary: None,
            final_style: None,
            confusable_pairs: &pairs,
            declared_pairs: Vec::new(),
        };
        // Distinct claim with a verified quote but no web lookup -> rejected.
        let no_web = tool_submit_result(
            &json!({
                "glossary":[
                    {"source":"foo bar","target":"甲","note":"distinct from 'foo baz': \"Welcome to Last Call Canyon\""},
                    {"source":"foo baz","target":"乙"}
                ],
                "style_guide":"s"
            }),
            &mut ctx,
        );
        assert!(!no_web.ok && no_web.repairable);
        assert!(no_web.message.contains("unverified"));
        // A past query covering one surface of the pair -> passes.
        ctx.web_queries.push("\"foo bar\" map canyon".to_string());
        let verified = tool_submit_result(
            &json!({
                "glossary":[
                    {"source":"foo bar","target":"甲","note":"distinct from 'foo baz': \"Welcome to Last Call Canyon\""},
                    {"source":"foo baz","target":"乙"}
                ],
                "style_guide":"s"
            }),
            &mut ctx,
        );
        assert!(verified.terminate, "{}", verified.message);
    }

    #[test]
    fn tool_schemas_omit_web_search_when_disabled() {
        let off = tool_schemas(false);
        let names: Vec<&str> = off
            .iter()
            .filter_map(|s| s.pointer("/function/name").and_then(|v| v.as_str()))
            .collect();
        assert!(!names.contains(&"web_search"));
        assert!(names.contains(&"submit_result"));
        let on = tool_schemas(true);
        let names_on: Vec<&str> = on
            .iter()
            .filter_map(|s| s.pointer("/function/name").and_then(|v| v.as_str()))
            .collect();
        assert!(names_on.contains(&"web_search"));
    }

    #[test]
    fn clean_glossary_accepts_origin_alias_and_dedupes() {
        let (g, dropped) = clean_glossary(Some(&json!([
            {"origin":"A","target":"甲"},
            {"source":"a","target":"乙"},
            {"source":"","target":"x"}
        ])));
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].target, "甲");
        assert_eq!(dropped, 2);
    }

    #[test]
    fn parse_tool_arguments_handles_fences() {
        let v = parse_tool_arguments("```json\n{\"pattern\":\"x\"}\n```").unwrap();
        assert_eq!(v["pattern"], "x");
    }

    #[test]
    fn extract_mcp_text_from_jsonrpc() {
        let raw = r#"{"result":{"content":[{"type":"text","text":"hello world"}]}}"#;
        assert_eq!(extract_mcp_text(raw), "hello world");
    }

    #[test]
    fn read_cues_returns_verbatim_range_and_reverses_args() {
        let cues = cues();
        let out = tool_read_cues(&json!({"from_index":1,"to_index":2}), &cues);
        assert!(out.ok);
        assert!(out.message.contains("[#1 00:01] Welcome to Last Call Canyon"));
        assert!(out.message.contains("[#2 00:05] Last Call Canyon is the map"));
        let rev = tool_read_cues(&json!({"from_index":2,"to_index":1}), &cues);
        assert!(rev.ok);
        let miss = tool_read_cues(&json!({"from_index":9,"to_index":12}), &cues);
        assert!(!miss.ok);
        let bad = tool_read_cues(&json!({"from_index":1}), &cues);
        assert!(!bad.ok);
    }

    #[test]
    fn read_cues_caps_large_ranges() {
        let cues: Vec<TranscriptCue> = (1..=400)
            .map(|i| TranscriptCue {
                index: i,
                start_ms: 0,
                text: "x".repeat(400),
            })
            .collect();
        let out = tool_read_cues(&json!({"from_index":1,"to_index":400}), &cues);
        assert!(out.ok);
        assert!(out.message.contains("capped"));
    }

    #[test]
    fn flag_pair_validates_and_registers() {
        let cues = cues();
        let pairs: Vec<(String, String)> = Vec::new();
        let mut ctx = AgentToolContext {
            cues: &cues,
            web_search_count: 0,
            web_search_enabled: false,
            web_queries: Vec::new(),
            submit_rejects: Vec::new(),
            final_glossary: None,
            final_style: None,
            confusable_pairs: &pairs,
            declared_pairs: Vec::new(),
        };
        // A surface absent from the transcript is rejected.
        let absent = tool_flag_pair(&json!({"a":"Canyon","b":"zzznope"}), &mut ctx);
        assert!(!absent.ok);
        // Identical surfaces (modulo case/whitespace) are rejected.
        let same = tool_flag_pair(&json!({"a":"Canyon","b":"canyon"}), &mut ctx);
        assert!(!same.ok);
        // A valid pair is registered and returns verbatim evidence.
        let ok = tool_flag_pair(&json!({"a":"Last Call Canyon","b":"Last Call"}), &mut ctx);
        assert!(ok.ok, "{}", ok.message);
        assert_eq!(ctx.declared_pairs.len(), 1);
        assert!(ok.message.contains("[#1]"));
        // Re-flagging the same pair (either order, any case) is a no-op.
        let dup = tool_flag_pair(&json!({"a":"last call","b":"LAST CALL CANYON"}), &mut ctx);
        assert!(dup.ok);
        assert_eq!(ctx.declared_pairs.len(), 1);
    }

    #[test]
    fn submit_gate_enforces_declared_pairs_like_deterministic_ones() {
        let cues = vec![
            TranscriptCue {
                index: 1,
                start_ms: 0,
                text: "We use order blocks here.".into(),
            },
            TranscriptCue {
                index: 2,
                start_ms: 1000,
                text: "That order block held.".into(),
            },
            TranscriptCue {
                index: 3,
                start_ms: 2000,
                text: "order blok again".into(),
            },
        ];
        let pairs: Vec<(String, String)> = Vec::new();
        let mut ctx = AgentToolContext {
            cues: &cues,
            web_search_count: 0,
            web_search_enabled: false,
            web_queries: Vec::new(),
            submit_rejects: Vec::new(),
            final_glossary: None,
            final_style: None,
            confusable_pairs: &pairs,
            declared_pairs: Vec::new(),
        };
        tool_flag_pair(&json!({"a":"order block","b":"order blok"}), &mut ctx);
        // Both surfaces kept, neither note names the other -> rejected,
        // exactly like a deterministic ⚠ pair.
        let mute = tool_submit_result(
            &json!({
                "glossary":[
                    {"source":"order block","target":"订单块"},
                    {"source":"order blok","target":"订单块"}
                ],
                "style_guide":"s"
            }),
            &mut ctx,
        );
        assert!(!mute.ok && mute.repairable && !mute.terminate);
        assert!(mute.message.contains("order blok"));
        // One side excluded -> the pair is resolved.
        let excluded = tool_submit_result(
            &json!({
                "glossary":[{"source":"order block","target":"订单块"}],
                "style_guide":"s"
            }),
            &mut ctx,
        );
        assert!(excluded.terminate, "{}", excluded.message);
    }

    #[test]
    fn submit_gate_accepts_single_and_curly_quote_styles() {
        // Production failure: the model quoted verbatim evidence with
        // 'single' quotes, the gate only accepted "double" quotes, and the
        // pair was salvaged away despite being correctly adjudicated. The
        // anti-fabrication property comes from transcript verification, not
        // from the quote character.
        let cues = cues();
        let pairs = vec![("foo bar".to_string(), "foo baz".to_string())];
        for note in [
            "distinct from 'foo baz': 'Welcome to Last Call Canyon'",
            "distinct from 'foo baz': “Welcome to Last Call Canyon”",
        ] {
            let mut ctx = AgentToolContext {
                cues: &cues,
                web_search_count: 0,
                web_search_enabled: false,
                web_queries: Vec::new(),
                submit_rejects: Vec::new(),
                final_glossary: None,
                final_style: None,
                confusable_pairs: &pairs,
                declared_pairs: Vec::new(),
            };
            let out = tool_submit_result(
                &json!({
                    "glossary":[
                        {"source":"foo bar","target":"甲","note":note},
                        {"source":"foo baz","target":"乙"}
                    ],
                    "style_guide":"s"
                }),
                &mut ctx,
            );
            assert!(out.terminate, "note={note}: {}", out.message);
        }
    }

    #[test]
    fn style_asr_exemption_is_per_sentence_not_global() {
        let cues = cues();
        let pairs = vec![
            ("foo bar".to_string(), "foo baz".to_string()),
            ("hard time frame".to_string(), "high time frame".to_string()),
        ];
        fn mk_ctx<'a>(
            cues: &'a [TranscriptCue],
            pairs: &'a [(String, String)],
        ) -> AgentToolContext<'a> {
            AgentToolContext {
                cues,
                web_search_count: 0,
                web_search_enabled: false,
                web_queries: Vec::new(),
                submit_rejects: Vec::new(),
                final_glossary: None,
                final_style: None,
                confusable_pairs: pairs,
                declared_pairs: Vec::new(),
            }
        }
        // 'hard time frame' excluded with an ASR-framed sentence; 'foo baz'
        // excluded but TAUGHT in an unframed sentence. The ASR mention in
        // sentence 1 must not exempt sentence 2.
        let mut ctx = mk_ctx(&cues, &pairs);
        let mixed = tool_submit_result(
            &json!({
                "glossary":[
                    {"source":"foo bar","target":"甲"},
                    {"source":"hard time frame","target":"更高时间框架"}
                ],
                "style_guide":"hard time frame 是 ASR 误听，按更高时间框架处理。把 foo baz 统一译为「乙」。"
            }),
            &mut ctx,
        );
        assert!(!mixed.ok && mixed.repairable, "{}", mixed.message);
        assert!(mixed.message.contains("foo baz"), "{}", mixed.message);
        assert!(!mixed.message.contains("hard time frame"), "{}", mixed.message);
        // Both excluded surfaces framed per-sentence -> passes.
        let mut ctx2 = mk_ctx(&cues, &pairs);
        let framed = tool_submit_result(
            &json!({
                "glossary":[
                    {"source":"foo bar","target":"甲"},
                    {"source":"hard time frame","target":"更高时间框架"}
                ],
                "style_guide":"hard time frame 是 ASR 误听。foo baz 是 ASR 误听。"
            }),
            &mut ctx2,
        );
        assert!(framed.terminate, "{}", framed.message);
    }

    #[test]
    fn salvage_strips_only_unframed_sentences() {
        let cues = cues();
        let pairs = vec![("foo bar".to_string(), "foo baz".to_string())];
        let mut ctx = AgentToolContext {
            cues: &cues,
            web_search_count: 0,
            web_search_enabled: false,
            web_queries: Vec::new(),
            submit_rejects: Vec::new(),
            final_glossary: None,
            final_style: None,
            confusable_pairs: &pairs,
            declared_pairs: Vec::new(),
        };
        let attempt = || {
            json!({
                "glossary":[
                    {"source":"foo bar","target":"甲"},
                    {"source":"foo baz","target":"乙"},
                    {"source":"Canyon","target":"峡谷"}
                ],
                "style_guide":"foo bar 是 ASR 误听，不要直译。foo baz 译为「乙」。整体保持口语化。"
            })
        };
        tool_submit_result(&attempt(), &mut ctx);
        tool_submit_result(&attempt(), &mut ctx);
        let third = tool_submit_result(&attempt(), &mut ctx);
        assert!(third.terminate, "{}", third.message);
        let style = ctx.final_style.as_ref().unwrap();
        // ASR-framed sentence naming a dropped surface survives; the
        // unframed teaching sentence is removed; unrelated sentence stays.
        assert!(style.contains("ASR 误听"), "style={style}");
        assert!(!style.contains("译为「乙」"), "style={style}");
        assert!(style.contains("口语化"), "style={style}");
    }
}
