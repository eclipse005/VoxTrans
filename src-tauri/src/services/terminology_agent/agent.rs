use serde_json::{json, Value};

use crate::services::llm::client::OpenAiCompatLlmClient;
use crate::services::llm::{ChatMessage, ToolCall};
use crate::services::task_log::{event, TaskLogger};
use crate::services::task_usage::{record_llm_usage_best_effort, LlmTokenUsage as TaskUsage};

use super::ground::transcript_plain;
use super::tools::{
    blocked_probe_message, dispatch_tool, execute_web_search, is_probe_tool, is_submit_tool,
    tool_schemas, verification_tool_names, AgentToolContext,
};
use super::types::{EndReason, GlossaryEntry, ToolKind, TranscriptCue};

pub const DEFAULT_MAX_ROUNDS: u32 = 15;
pub const MULTI_WINDOW_MAX_ROUNDS: u32 = 12;
pub const DOOM_SOFT: u32 = 5;
pub const DOOM_HARD: u32 = 8;
pub const PROBE_BUDGET_SINGLE: u32 = 12;
pub const PROBE_BUDGET_MULTI: u32 = 12;
const KEEP_RECENT_TURNS: usize = 3;
const PROJECT_MAX_CHARS: usize = 220_000;
const TOOL_ERROR_NUDGE_MAX: u32 = 2;
const CONSECUTIVE_LLM_ERROR_ABORT: u32 = 3;
const ESTABLISHED_PROMPT_CAP: usize = 60;

pub fn build_system_prompt(
    title: &str,
    source_lang: &str,
    target_lang: &str,
    user_terms_block: &str,
    established_terms: &[GlossaryEntry],
    probe_budget: u32,
    web_search: bool,
) -> String {
    let tools = if web_search {
        "count/search/read/web_search"
    } else {
        "count/search/read"
    };
    // ⚠ pairs are noise zones. Decision order: map with evidence, exclude
    // when in doubt, keep-both only with contrastive verbatim quotes (the
    // gate verifies them). Frequency is not realness — ASR mishears
    // consistently. No self-check tool exists: the model's own confidence
    // is not evidence.
    let pair_evidence_rule = if web_search {
        "- If unsure which surface is real, compare evidence for BOTH surfaces with web_search: \
         qualify each query with 1–2 domain keywords from this video (title + transcript) — \"<surface>\" <domain keywords> — \
         or search the exact collocation heard around the surface. Bare-term searches prove nothing: generic word combinations always get hits. \
         A domain-qualified search that finds the exact phrase USED AS A TERM supports it being real; finding nothing used as a term is a strong mishearing signal — act on it. \
         Still in doubt after probing -> exclude the surface."
    } else {
        "- If unsure which surface is real, weigh transcript collocations and the title. Still in doubt -> exclude the surface."
    };
    let term_reality_check = if web_search {
        "domain knowledge, or a domain-qualified web_search that finds it used as a term vs finds nothing"
    } else {
        "domain knowledge plus the video's title/context"
    };
    let distinct_requirements = if web_search {
        "at least one kept note quoting (in \"double quotes\", 12+ chars — the harness verifies it against the transcript) the line showing the distinct usage, AND a domain-qualified web_search covering the surface before you submit"
    } else {
        "at least one kept note quoting (in \"double quotes\", 12+ chars — the harness verifies it against the transcript) the line showing the distinct usage"
    };
    let established_block = if established_terms.is_empty() {
        String::new()
    } else {
        let rows = established_terms
            .iter()
            .take(ESTABLISHED_PROMPT_CAP)
            .map(|t| format!("  - {} -> {}", t.source, t.target))
            .collect::<Vec<_>>()
            .join("\n");
        format!(
            "\nESTABLISHED TERMS (decided by earlier windows of THIS video):\n{rows}\n\
Reuse these targets for the same concept — including singular/plural and abbreviation variants \
(add the variant surface as its own row with the SAME target). Add new rows only for concepts not covered.\n"
        )
    };
    format!(
        "You are a terminology briefing agent for a subtitle translation pipeline.\n\
Downstream translators will receive your glossary as an enforced term table and your style guide as styleGuide. Maximize translation consistency and fluency — do NOT translate the full transcript.\n\n\
TITLE: {title}\n\
LANG: {source_lang} → {target_lang}\n\n\
USER TERMS (optional candidates; include only if relevant; may be noisy — not a dump):\n\
{user_terms_block}\n\
{established_block}\n\
Glossary rules (critical for enforcement):\n\
- source must appear in THIS transcript (exact phrase as written). You judge meaning: if a user/established term's concept appears under another surface (full form, abbreviation, ASR variant), add that surface → same target. Harness does not invent those links.\n\
- Same concept, multiple surfaces → one row per surface, same target. No invented sources; no \"A (B)\" unless that exact string appears.\n\
- When a concept has multiple surfaces, add a short note on the primary/full form listing the variants you identified; downstream translators use this for context.\n\
- Prefer names, proper nouns, abbreviations, recurring technical phrases.\n\
- target: consistent rendering in {target_lang} (or keep source form when conventional); user target wins when you include a user concept.\n\
- Never append the source phrase in its original script to a target (no parenthetical originals): the source is already the row's key, and leftover source script inside a target is rejected downstream as untranslated leakage. Latin-alphabet renderings of names are fine when conventional in {target_lang}.\n\
- note: optional, short.\n\n\
Style guide rules:\n\
- ONE plain string, 2–4 sentences, written to guide a {target_lang} translator.\n\
- Cover tone/register, how to treat names/abbreviations, and any consistency traps for THIS video.\n\
- Do not ask translators to keep source-script text inline in the output (e.g. 'keep the original in parentheses') — the translation must read as clean {target_lang}.\n\
- State only what the transcript shows: never complete a partial fact from imagination (turning a surname into a full name) or assert affiliations/backstory that do not appear.\n\
- No bullet keys, no domain templates.\n\n\
Workflow (probe budget: {probe_budget} calls across {tools}; flag_pair and submit_result are free):\n\
1. The user message contains a pre-extracted CANDIDATES list (phrase + transcript frequency). Treat it as a starting hint, not a closed list: real recurring terms may be missing from it, so while reading the transcript, also note domain terms absent from the list and include them.\n\
2. Read the transcript ONCE (it is removed from context after round 1; re-read any cue range verbatim with read_cues when you need the exact wording again). Draft glossary + style guide from it plus candidates.\n\
3. Spend probes only to disambiguate spelling/sense/surface variants you are unsure about.\n\
4. If while reading you notice TWO similar surfaces that may be ONE term (e.g. one reads as a consistent mishearing of the other — including pairs the ⚠ list missed), call flag_pair(a, b) to register the pair; it returns verbatim evidence lines. Flagged pairs face the SAME adjudication rules as ⚠ pairs at submit_result. The goal is consistent translation — a wrong row enforces a systematic mistranslation on every matching line — not fixing the transcript.\n\
5. When done: call submit_result ALONE (no other tools in that turn) with glossary + style_guide only.\n\n\
ASR noise handling (critical):\n\
- ASR mishears — and mishears CONSISTENTLY: a wrong surface can appear 20+ times. Frequency is NOT evidence that a surface is a real term.\n\
- Candidates marked ⚠ differ from another candidate by one word and the differing words look confusable — treat every ⚠ pair as a danger zone and decide it BEFORE submitting, in this order. Verbatim lines for both surfaces are attached in the ⚠ PAIR EVIDENCE section — adjudicate from those first; probe only if they are insufficient:\n\
  1. Compare the context slots. Both surfaces appearing in the same slot (\"___ <same surrounding words>\") = strong same-concept signal. Interchangeable contexts mean SAME concept — go straight to mapping (step 2) or exclusion (step 3); do NOT build a distinct-concept claim from interchangeable contexts.\n\
  2. One surface is a known domain term and the other reads as nonsense in this domain ({term_reality_check}) -> the odd one is a mishearing: keep it as its own row with the REAL term's target, note \"likely ASR mishearing of 'Y'\". Correct mappings are valuable: translators then render those lines right.\n\
  3. You cannot confirm which is real -> EXCLUDE the suspect surface entirely. This glossary is enforced on every matching subtitle line: a wrong row does systematic damage — worse than no row.\n\
  4. Keep BOTH surfaces with DIFFERENT targets only with external, contrastive evidence: {distinct_requirements}. Never invent a semantic distinction just to keep both surfaces — when the evidence is not there, map or exclude.\n\
- For every ⚠ pair, at least one kept side's note must name the other surface and the call (\"likely ASR mishearing of 'X'\" / \"'X' excluded as likely mishearing\" / \"distinct from 'X'\"). Excluded surfaces need nothing. submit_result is REJECTED while any kept ⚠ pair lacks that note.\n\
- The same discipline applies to style_guide: never assert a ⚠-pair distinction there that you could not ground in the glossary. A surface you excluded from the glossary must not be taught a rendering in style_guide — omit it, or explicitly mark it as an excluded ASR mishearing (the harness rejects style_guide text that names an excluded surface without an ASR-noise framing).\n\
{pair_evidence_rule}\n\
- Beyond ⚠ pairs: a candidate that makes no sense in this domain but resembles a well-known term: correct it only when confident from domain knowledge + context; otherwise exclude it.\n\n\
Coverage: include one glossary row for EVERY candidate you judge to be a real domain term. The glossary is NOT a list of important words: include a term only when fixing its rendering now is likely to prevent inconsistent or wrong translations later — high frequency alone does not qualify (\"good\", \"think\", \"people\" stay out no matter how often they appear). Skip generic everyday phrases even when frequent (\"everything else\", month names, discourse fillers) — they need no enforcement. When in doubt about a surface's authenticity, leave it out. Submitting in round 1 with zero probes usually means you under-extracted — check the ⚠ pairs and uncertain candidates first.\n"
    )
}

pub fn format_user_terms_block(terms: &[GlossaryEntry]) -> String {
    if terms.is_empty() {
        return "(none)".to_string();
    }
    terms
        .iter()
        .map(|t| {
            if t.note.trim().is_empty() {
                format!("  - {} -> {}", t.source, t.target)
            } else {
                format!("  - {} -> {} ({})", t.source, t.target, t.note.trim())
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub const TRANSCRIPT_BEGIN: &str = "=== TRANSCRIPT (removed from context after round 1; use tools for evidence later) ===";
pub const TRANSCRIPT_END: &str = "=== END TRANSCRIPT ===";

pub fn build_user_message(
    cues: &[TranscriptCue],
    source_lang: &str,
    target_lang: &str,
    window_label: Option<(usize, usize)>,
    candidates_block: &str,
) -> String {
    let plain = transcript_plain(cues);
    let mut head = format!(
        "Analyze this {source_lang} transcript ({} segments). Extract glossary + style_guide for {target_lang} translation. Do not translate the full transcript.\n",
        cues.len()
    );
    if let Some((i, n)) = window_label {
        head = format!(
            "[Transcript window {i}/{n}. Extract glossary/style for THIS window. Tools may search the full file.]\n\n{head}"
        );
    }
    format!(
        "{head}\n=== CANDIDATES (pre-extracted phrase frequencies; your discovery list) ===\n{candidates_block}\n\n{TRANSCRIPT_BEGIN}\n{plain}\n{TRANSCRIPT_END}"
    )
}

pub struct WindowRun {
    pub glossary: Vec<GlossaryEntry>,
    pub style_guide: String,
    pub end_reason: EndReason,
    pub rounds_used: u32,
}

pub struct AgentRunConfig<'a> {
    pub client: &'a OpenAiCompatLlmClient,
    pub title: &'a str,
    pub source_lang: &'a str,
    pub target_lang: &'a str,
    pub window_cues: &'a [TranscriptCue],
    pub all_cues: &'a [TranscriptCue],
    pub user_terms: &'a [GlossaryEntry],
    pub established_terms: &'a [GlossaryEntry],
    pub candidates_block: &'a str,
    pub confusable_pairs: &'a [(String, String)],
    pub max_rounds: u32,
    pub probe_budget: u32,
    pub window_label: Option<(usize, usize)>,
    pub task_id: &'a str,
    pub store: Option<crate::db::store::TaskStore>,
    pub web_search: Option<&'a super::tools::WebSearchProvider>,
    pub logger: Option<&'a TaskLogger>,
}

pub async fn run_window_agent(
    cfg: &AgentRunConfig<'_>,
    mut on_round: Option<&mut (dyn FnMut(u32, u32, Option<String>) + Send)>,
) -> WindowRun {
    let web_search = cfg.web_search.is_some();
    let system = build_system_prompt(
        cfg.title,
        cfg.source_lang,
        cfg.target_lang,
        &format_user_terms_block(cfg.user_terms),
        cfg.established_terms,
        cfg.probe_budget,
        web_search,
    );
    let user = build_user_message(
        cfg.window_cues,
        cfg.source_lang,
        cfg.target_lang,
        cfg.window_label,
        cfg.candidates_block,
    );
    let tools = tool_schemas(web_search);
    let mut messages = vec![ChatMessage::system(system), ChatMessage::user(user)];
    let mut consecutive_verify = 0u32;
    let mut tool_error_nudges = 0u32;
    let mut consecutive_llm_errors = 0u32;
    let mut probe_count = 0u32;

    let mut web_count = 0u32;
    let mut web_queries: Vec<String> = Vec::new();
    let mut declared_pairs: Vec<(String, String)> = Vec::new();
    let mut submit_rejects: Vec<String> = Vec::new();
    let mut rounds_used = 0u32;
    let mut end_reason = EndReason::NoToolCalls;
    let mut final_glossary = None;
    let mut final_style = None;
    let window_n = cfg.window_label.map(|(i, _)| i).unwrap_or(1);
    // Exact-duplicate probe cache: re-asking the identical question returns
    // the earlier answer without burning budget or adding new information.
    let mut probe_cache: std::collections::HashMap<String, String> = std::collections::HashMap::new();

    for round in 1..=cfg.max_rounds {
        rounds_used = round;
        if let Some(cb) = on_round.as_mut() {
            cb(round, cfg.max_rounds, None);
        }
        let projected = project_context(&messages, KEEP_RECENT_TURNS, PROJECT_MAX_CHARS);
        let turn = match cfg.client.call_tools(&projected, &tools, Some(0.3)).await {
            Ok(t) => {
                consecutive_llm_errors = 0;
                t
            }
            Err(err) => {
                consecutive_llm_errors += 1;
                eprintln!(
                    "[terminology-agent] llm error round {round}: {err}"
                );
                log_agent(
                    cfg.logger,
                    event::AGENT_HARNESS,
                    json!({
                        "window": window_n,
                        "round": round,
                        "kind": "llm_error",
                        "consecutive": consecutive_llm_errors,
                        "error": err.to_string(),
                    }),
                );
                if consecutive_llm_errors >= CONSECUTIVE_LLM_ERROR_ABORT {
                    end_reason = EndReason::LlmError;
                    break;
                }
                messages.push(ChatMessage::user(format!(
                    "[HARNESS] The previous model call failed ({err}). Retry. When the task is complete, call submit_result."
                )));
                continue;
            }
        };
        record_llm_usage_best_effort(
            cfg.task_id,
            "terminology",
            TaskUsage {
                prompt_tokens: turn.usage.prompt_tokens,
                completion_tokens: turn.usage.completion_tokens,
                total_tokens: turn.usage.total_tokens,
            },
            cfg.store.clone(),
        );

        let tool_calls = turn.tool_calls.clone();
        log_agent(
            cfg.logger,
            event::AGENT_ROUND,
            json!({
                "window": window_n,
                "round": round,
                "maxRounds": cfg.max_rounds,
                "usage": {
                    "promptTokens": turn.usage.prompt_tokens,
                    "completionTokens": turn.usage.completion_tokens,
                    "totalTokens": turn.usage.total_tokens,
                },
                "content": clip_chars(&turn.content, 800),
                "reasoning": clip_chars(turn.reasoning_content.as_deref().unwrap_or(""), 800),
                "tools": tool_calls.iter().map(|tc| json!({
                    "id": tc.id,
                    "name": tc.function.name,
                    "arguments": parse_logged_args(&tc.function.arguments),
                })).collect::<Vec<_>>(),
            }),
        );
        messages.push(ChatMessage::assistant_tools(
            if turn.content.trim().is_empty() {
                None
            } else {
                Some(turn.content.clone())
            },
            tool_calls.clone(),
            turn.reasoning_content.clone(),
        ));

        if tool_calls.is_empty() {
            // final_glossary/final_style are only ever set on the
            // submit_ok termination path, which breaks out of this loop
            // before reaching here — so no SubmitOk can be pending.
            end_reason = EndReason::NoToolCalls;
            break;
        }

        let called: Vec<String> = tool_calls
            .iter()
            .map(|tc| tc.function.name.clone())
            .collect();
        if let Some(cb) = on_round.as_mut() {
            cb(round, cfg.max_rounds, called.first().cloned());
        }

        let mut had_repairable = false;
        let mut terminate = false;
        let batch_has_submit = tool_calls.iter().any(|tc| is_submit_tool(&tc.function.name));
        for tc in &tool_calls {
            let is_probe = is_probe_tool(&tc.function.name);
            let outcome = if batch_has_submit && is_probe {
                super::types::ToolOutcome::blocked(blocked_probe_message(&tc.function.name))
            } else if is_probe && probe_count >= cfg.probe_budget {
                super::types::ToolOutcome::blocked(format!(
                    "Probe budget ({}) for this window is exhausted — further probes return nothing. \
                     Decide the ⚠ pairs from the evidence already gathered: interchangeable contexts = same concept (map or exclude the suspect); when in doubt, EXCLUDE. \
                     Call submit_result ALONE with glossary + style_guide now.",
                    cfg.probe_budget
                ))
            } else if is_probe {
                let cache_key = format!("{}\n{}", tc.function.name, tc.function.arguments.trim());
                if let Some(cached) = probe_cache.get(&cache_key) {
                    super::types::ToolOutcome::ok_msg(format!(
                        "{cached}\n[duplicate call — identical to an earlier probe; same result, not counted against your probe budget. Do not repeat probes.]"
                    ))
                } else {
                    probe_count += 1;
                    let out = execute_one_tool(tc, cfg, &mut web_count, &mut web_queries, &mut submit_rejects, &mut declared_pairs, &mut final_glossary, &mut final_style).await;
                    if out.ok {
                        probe_cache.insert(cache_key, out.message.clone());
                    }
                    out
                }
            } else {
                execute_one_tool(tc, cfg, &mut web_count, &mut web_queries, &mut submit_rejects, &mut declared_pairs, &mut final_glossary, &mut final_style).await
            };
            log_agent(
                cfg.logger,
                event::AGENT_TOOL,
                json!({
                    "window": window_n,
                    "round": round,
                    "id": tc.id,
                    "name": tc.function.name,
                    "arguments": parse_logged_args(&tc.function.arguments),
                    "ok": outcome.ok,
                    "kind": match outcome.kind {
                        ToolKind::Ok => "ok",
                        ToolKind::Error => "error",
                        ToolKind::SubmitOk => "submit_ok",
                        ToolKind::Blocked => "blocked",
                    },
                    "result": clip_chars(&outcome.message, 2_000),
                }),
            );
            if outcome.kind == ToolKind::Error && outcome.repairable {
                had_repairable = true;
            }
            if outcome.terminate && outcome.ok {
                terminate = true;
            }
            messages.push(ChatMessage::tool(&tc.id, outcome.message));
        }
        if terminate {
            end_reason = EndReason::SubmitOk;
            break;
        }
        if had_repairable && tool_error_nudges < TOOL_ERROR_NUDGE_MAX {
            tool_error_nudges += 1;
            log_agent(
                cfg.logger,
                event::AGENT_HARNESS,
                json!({
                    "window": window_n,
                    "round": round,
                    "kind": "tool_error",
                }),
            );
            messages.push(ChatMessage::user(
                "[HARNESS] A tool call returned an error. Read the Hint, fix arguments, and retry. When the task is complete, call submit_result.",
            ));
        }

        let verify_only = !called.is_empty()
            && called.iter().all(|n| verification_tool_names().contains(&n.as_str()));
        if verify_only {
            consecutive_verify += 1;
        } else {
            consecutive_verify = 0;
        }
        if consecutive_verify >= DOOM_HARD {
            log_agent(
                cfg.logger,
                event::AGENT_HARNESS,
                json!({
                    "window": window_n,
                    "round": round,
                    "kind": "doom_hard",
                    "consecutiveVerify": consecutive_verify,
                }),
            );
            messages.push(ChatMessage::user(format!(
                "[HARNESS] You have run verification tools for {consecutive_verify} consecutive rounds without submitting. \
                 This is a doom loop — you already have enough findings. Call submit_result NOW with glossary + style_guide \
                 (the submission gate still applies — resolve ⚠ pairs as usual; do not start new verification)."
            )));
        } else if consecutive_verify >= DOOM_SOFT {
            log_agent(
                cfg.logger,
                event::AGENT_HARNESS,
                json!({
                    "window": window_n,
                    "round": round,
                    "kind": "doom_soft",
                    "consecutiveVerify": consecutive_verify,
                }),
            );
            messages.push(ChatMessage::user(format!(
                "[HARNESS] You've used verification tools for {consecutive_verify} rounds. \
                 Consider calling submit_result with glossary + style_guide."
            )));
        }
    }

    if rounds_used >= cfg.max_rounds
        && !matches!(end_reason, EndReason::SubmitOk | EndReason::LlmError)
    {
        // SubmitOk/LlmError already carry their final reason; anything else
        // past the round budget is a plain exhaustion. final_* are only set
        // on the submit_ok termination path, which exits with SubmitOk.
        end_reason = EndReason::MaxRounds;
    }

    let glossary = final_glossary.unwrap_or_default();
    let style_guide = final_style.unwrap_or_default();
    log_agent(
        cfg.logger,
        event::AGENT_WINDOW_END,
        json!({
            "window": window_n,
            "endReason": end_reason.as_str(),
            "rounds": rounds_used,
            "glossary": glossary.len(),
            "styleChars": style_guide.chars().count(),
            "styleGuide": clip_chars(&style_guide, 1_200),
            "terms": glossary.iter().map(|g| json!({
                "source": g.source,
                "target": g.target,
            })).collect::<Vec<_>>(),
        }),
    );
    WindowRun {
        glossary,
        style_guide,
        end_reason,
        rounds_used,
    }
}

/// The transcript is only needed for the round-1 read-through. From round 2
/// on, replace it with a pointer (candidates block stays — it is the working
/// list). Pi's stable-prefix discipline adapted for providers without
/// caching: the big static blob leaves the wire instead of being re-sent.
fn evict_transcript_after_round_one(messages: &[ChatMessage]) -> Vec<ChatMessage> {
    let has_assistant = messages.iter().any(|m| m.role == "assistant");
    if !has_assistant {
        return messages.to_vec();
    }
    let mut out = messages.to_vec();
    if let Some(m) = out.get_mut(1) {
        if m.role == "user" {
            if let Some(content) = &m.content {
                m.content = Some(replace_transcript_section(content));
            }
        }
    }
    out
}

fn replace_transcript_section(content: &str) -> String {
    let begin = match content.find(TRANSCRIPT_BEGIN) {
        Some(i) => i + TRANSCRIPT_BEGIN.len(),
        None => return content.to_string(),
    };
    let end = match content[begin..].find(TRANSCRIPT_END) {
        Some(i) => begin + i,
        None => return content.to_string(),
    };
    format!(
        "{}\n(transcript omitted to save context — you read it in round 1; use search_transcript / count_transcript / read_cues for evidence)\n{}",
        &content[..begin],
        &content[end..]
    )
}

fn log_agent(logger: Option<&TaskLogger>, event_type: &str, payload: Value) {
    if let Some(logger) = logger {
        logger.event(event_type, Some(&payload));
    }
}

fn clip_chars(input: &str, max_chars: usize) -> String {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }
    trimmed.chars().take(max_chars).collect::<String>() + "…"
}

fn parse_logged_args(raw: &str) -> Value {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return json!({});
    }
    serde_json::from_str(trimmed).unwrap_or_else(|_| Value::String(clip_chars(trimmed, 400)))
}

async fn execute_one_tool(
    tc: &ToolCall,
    cfg: &AgentRunConfig<'_>,
    web_count: &mut u32,
    web_queries: &mut Vec<String>,
    submit_rejects: &mut Vec<String>,
    declared_pairs: &mut Vec<(String, String)>,
    final_glossary: &mut Option<Vec<GlossaryEntry>>,
    final_style: &mut Option<String>,
) -> super::types::ToolOutcome {
    let args = match super::tools::parse_tool_arguments(&tc.function.arguments) {
        Ok(v) => v,
        Err(e) => {
            return super::types::ToolOutcome::err(format!("Error: {}: {e}", tc.function.name));
        }
    };
    let mut tool_ctx = AgentToolContext {
        cues: cfg.all_cues,
        web_search_count: *web_count,
        web_search_enabled: cfg.web_search.is_some(),
        web_queries: web_queries.clone(),
        submit_rejects: submit_rejects.clone(),
        declared_pairs: declared_pairs.clone(),
        final_glossary: final_glossary.clone(),
        final_style: final_style.clone(),
        confusable_pairs: cfg.confusable_pairs,
    };
    let pre = dispatch_tool(&tc.function.name, &args, &mut tool_ctx);
    *web_queries = tool_ctx.web_queries;
    *submit_rejects = tool_ctx.submit_rejects;
    *declared_pairs = tool_ctx.declared_pairs;
    if let Some(g) = tool_ctx.final_glossary {
        *final_glossary = Some(g);
    }
    if let Some(s) = tool_ctx.final_style {
        *final_style = Some(s);
    }

    if pre.message.starts_with("WEB_PENDING\t") {
        let query = pre.message.trim_start_matches("WEB_PENDING\t").to_string();
        let Some(provider) = cfg.web_search else {
            return super::types::ToolOutcome::err(
                "Error: web_search is not configured. Ground terms in the transcript.".to_string(),
            );
        };
        *web_count += 1;
        let result = execute_web_search(&query, cfg.task_id, provider).await;
        return super::types::ToolOutcome::ok_msg(result);
    }
    pre
}

pub fn project_context(
    messages: &[ChatMessage],
    keep_recent_turns: usize,
    max_chars: usize,
) -> Vec<ChatMessage> {
    let messages = evict_transcript_after_round_one(messages);
    let messages: &[ChatMessage] = &messages;
    if messages.len() <= 2 {
        return messages.to_vec();
    }
    let mut keep = keep_recent_turns.max(1);
    loop {
        let shaped = project_shape(messages, keep);
        if keep == 1 || estimate_chars(&shaped) <= max_chars {
            return prepare_wire(shaped);
        }
        keep -= 1;
    }
}

fn project_shape(messages: &[ChatMessage], keep_recent_turns: usize) -> Vec<ChatMessage> {
    let assistant_idx: Vec<usize> = messages
        .iter()
        .enumerate()
        .filter(|(_, m)| m.role == "assistant")
        .map(|(i, _)| i)
        .collect();
    if assistant_idx.len() <= keep_recent_turns {
        return messages.to_vec();
    }
    let keep_from = assistant_idx[assistant_idx.len() - keep_recent_turns];
    let head = messages[..2].to_vec();
    let tail = messages[keep_from..].to_vec();
    let middle = &messages[2..keep_from];
    let mut tool_results = std::collections::HashMap::new();
    for m in middle {
        if m.role == "tool" {
            if let Some(id) = &m.tool_call_id {
                tool_results.insert(id.clone(), m.content.clone().unwrap_or_default());
            }
        }
    }
    let mut compressed = Vec::new();
    for m in middle {
        if m.role == "assistant" {
            compressed.push(compress_assistant_turn(m, &tool_results));
        } else if m.role == "tool" {
            continue;
        } else {
            compressed.push(m.clone());
        }
    }
    let mut out = head;
    out.extend(compressed);
    out.extend(tail);
    out
}

fn compress_assistant_turn(
    assistant: &ChatMessage,
    tool_results: &std::collections::HashMap<String, String>,
) -> ChatMessage {
    let keep_for = |name: &str| -> usize {
        match name {
            "web_search" => 1000,
            "search_transcript" => 160,
            "count_transcript" => 100,
            "submit_result" => 220,
            _ => 160,
        }
    };
    let mut parts = Vec::new();
    if let Some(tcs) = &assistant.tool_calls {
        for tc in tcs {
            let args = one_line(&tc.function.arguments, 40);
            let tres = one_line(
                tool_results.get(&tc.id).map(String::as_str).unwrap_or(""),
                keep_for(&tc.function.name),
            );
            parts.push(format!("{}({args}) -> {tres}", tc.function.name));
        }
    }
    let said = one_line(assistant.content.as_deref().unwrap_or(""), 100);
    let summary = if parts.is_empty() {
        "(no tool call)".to_string()
    } else {
        parts.join("; ")
    };
    let note = if said.is_empty() {
        String::new()
    } else {
        format!(" | said: {said}")
    };
    ChatMessage::user(format!("[prior round: {summary}{note}]"))
}

fn prepare_wire(messages: Vec<ChatMessage>) -> Vec<ChatMessage> {
    messages
        .into_iter()
        .map(|mut m| {
            let has_tools = m
                .tool_calls
                .as_ref()
                .map(|t| !t.is_empty())
                .unwrap_or(false);
            if m.role != "assistant" || !has_tools {
                m.reasoning_content = None;
            }
            m
        })
        .collect()
}

fn estimate_chars(messages: &[ChatMessage]) -> usize {
    messages.iter().map(|m| {
        let mut n = m.content.as_deref().map(str::len).unwrap_or(0);
        n += m.reasoning_content.as_deref().map(str::len).unwrap_or(0);
        if let Some(tcs) = &m.tool_calls {
            for tc in tcs {
                n += tc.function.name.len() + tc.function.arguments.len();
            }
        }
        n
    }).sum()
}

fn one_line(text: &str, limit: usize) -> String {
    let s: String = text.chars().map(|c| if c == '\n' { ' ' } else { c }).collect();
    let s = s.trim();
    if s.chars().count() <= limit {
        return s.to_string();
    }
    s.chars().take(limit.saturating_sub(1)).collect::<String>() + "…"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::llm::{ToolCall, ToolCallFunction};

    fn tool_call(id: &str, name: &str) -> ToolCall {
        ToolCall {
            id: id.into(),
            type_: "function".into(),
            function: ToolCallFunction {
                name: name.into(),
                arguments: "{\"pattern\":\"x\"}".into(),
            },
        }
    }

    #[test]
    fn project_compresses_old_tool_turns() {
        let mut msgs = vec![
            ChatMessage::system("sys"),
            ChatMessage::user("transcript here"),
        ];
        for i in 0..6 {
            let id = format!("c{i}");
            msgs.push(ChatMessage::assistant_tools(
                None,
                vec![tool_call(&id, "search_transcript")],
                Some("secret-cot".into()),
            ));
            msgs.push(ChatMessage::tool(&id, "lots of evidence text"));
        }
        let projected = project_context(&msgs, 2, 10_000);
        assert_eq!(projected[0].role, "system");
        assert_eq!(projected[1].role, "user");
        let prior = projected
            .iter()
            .filter(|m| m.content.as_deref().unwrap_or("").starts_with("[prior round:"))
            .count();
        assert!(prior >= 1);
        // Live assistant+tool pairs keep tool_calls; compressed ones do not.
        let live_tools = projected.iter().filter(|m| m.tool_calls.is_some()).count();
        assert_eq!(live_tools, 2);
        for m in &projected {
            if m.tool_calls.is_none() {
                assert!(m.reasoning_content.is_none());
            }
        }
    }

    #[test]
    fn user_terms_block_none_when_empty() {
        assert_eq!(format_user_terms_block(&[]), "(none)");
        let block = format_user_terms_block(&[GlossaryEntry::new("A", "甲", "n")]);
        assert!(block.contains("A -> 甲 (n)"));
    }

    #[test]
    fn system_prompt_advertises_web_search_only_when_enabled() {
        let off = build_system_prompt("t", "en", "zh", "(none)", &[], 24, false);
        assert!(!off.contains("web_search"));
        let on = build_system_prompt("t", "en", "zh", "(none)", &[], 24, true);
        assert!(on.contains("web_search"));
    }

    #[test]
    fn system_prompt_lists_established_terms_only_when_present() {
        let none = build_system_prompt("t", "en", "zh", "(none)", &[], 24, false);
        assert!(!none.contains("ESTABLISHED TERMS"));
        let est = vec![GlossaryEntry::new("quarterly shifts", "季度切换", "")];
        let some = build_system_prompt("t", "en", "zh", "(none)", &est, 24, false);
        assert!(some.contains("ESTABLISHED TERMS"));
        assert!(some.contains("quarterly shifts -> 季度切换"));
    }

    #[test]
    fn projection_evicts_transcript_but_keeps_candidates_after_round_one() {
        let user = build_user_message(
            &[TranscriptCue {
                index: 1,
                start_ms: 0,
                text: "order block here".into(),
            }],
            "en",
            "zh",
            None,
            "  - 3x 'order block' (e.g. cue #1)",
        );
        assert!(user.contains(TRANSCRIPT_BEGIN));
        let mut msgs = vec![ChatMessage::system("sys"), ChatMessage::user(user)];
        // Round 1: transcript still on the wire.
        let r1 = project_context(&msgs, 4, 10_000);
        assert!(r1[1].content.as_deref().unwrap().contains("order block here"));
        msgs.push(ChatMessage::assistant_tools(
            None,
            vec![tool_call("c0", "search_transcript")],
            None,
        ));
        msgs.push(ChatMessage::tool("c0", "3x 'order block'"));
        // Round 2: transcript evicted, candidates retained.
        let r2 = project_context(&msgs, 4, 10_000);
        let c = r2[1].content.as_deref().unwrap();
        assert!(!c.contains("order block here"));
        assert!(c.contains("transcript omitted"));
        assert!(c.contains("3x 'order block'"));
    }

    #[test]
    fn logged_args_prefer_json_and_clip_garbage() {
        assert_eq!(parse_logged_args("{\"pattern\":\"x\"}")["pattern"], "x");
        assert_eq!(parse_logged_args(""), serde_json::json!({}));
        let clipped = parse_logged_args(&"not-json-".repeat(80));
        assert!(clipped.as_str().unwrap().ends_with('…'));
    }
}
