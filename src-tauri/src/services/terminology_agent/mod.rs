//! Terminology Agent: harness (loop + tools + grounding) that produces a
//! per-task glossary + style guide for the batched translator.
//!
//! Fail-open by design: any agent error — including a panic — returns a
//! skipped briefing so translation still runs (user glossary only).

mod agent;
mod candidates;
mod ground;
mod tools;
mod types;

pub use ground::{merge_glossary_user_priority, source_grounded_in_text};
pub use types::{EndReason, GlossaryEntry, TerminologyBriefing, TranscriptCue};

/// For examples/replay tooling: deterministic candidate list as prompt block.
pub fn debug_candidates_block(cues: &[TranscriptCue]) -> String {
    let cands = candidates::extract_candidates(cues);
    let pairs = candidates::find_confusable_pairs(&cands);
    candidates::format_candidates_block(&cands, &pairs)
        + &candidates::format_pair_evidence(&pairs, cues)
}

use serde::{Deserialize, Serialize};

use crate::db::store::TaskStore;
use crate::services::llm::client::AnthropicLlmClient;
use crate::services::llm::port::LlmConfig;
use crate::services::task_log::{event, TaskLogger};

use agent::{
    run_window_agent, AgentRunConfig, DEFAULT_MAX_ROUNDS, MULTI_WINDOW_MAX_ROUNDS,
    PROBE_BUDGET_MULTI, PROBE_BUDGET_SINGLE,
};
use ground::{
    ground_glossary, merge_style_guides, split_cues_windows, transcript_haystack,
    union_glossaries,
};

/// Merged briefing and per-window checkpoints share this prefix.
pub const TERMINOLOGY_ARTIFACT_PREFIX: &str = "step_03_terminology";

fn window_artifact_name(index: usize) -> String {
    format!("{TERMINOLOGY_ARTIFACT_PREFIX}_w{index}")
}

#[derive(Debug, Clone)]
pub struct TerminologyAgentInput {
    pub task_id: String,
    pub media_path: String,
    pub title: String,
    pub source_lang: String,
    pub target_lang: String,
    pub cues: Vec<TranscriptCue>,
    pub user_terms: Vec<GlossaryEntry>,
    pub api_key: String,
    pub base_url: String,
    pub model: String,
    /// AnySearch key from settings UI. Empty = fall back to the free
    /// Parallel endpoint (see tools::web_search_provider).
    pub anysearch_api_key: String,
    pub store: Option<TaskStore>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WindowCheckpoint {
    glossary: Vec<GlossaryEntry>,
    style_guide: String,
}

/// Run the terminology Agent. Never returns Err — failures become `skipped`,
/// and even a panic inside the agent is caught so translation always
/// continues with just the user's own glossary.
pub async fn run_terminology_briefing<F>(
    input: TerminologyAgentInput,
    mut on_progress: F,
) -> TerminologyBriefing
where
    F: FnMut(usize, usize, u32, u32, Option<&str>) + Send,
{
    let task_id = input.task_id.clone();
    let media_path = input.media_path.clone();
    use futures_util::FutureExt;
    let result = std::panic::AssertUnwindSafe(run_terminology_briefing_inner(input, &mut on_progress))
        .catch_unwind()
        .await;
    let skipped_reason = match result {
        Ok(Ok(b)) => return b,
        Ok(Err(e)) => e,
        Err(payload) => {
            let detail = payload
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "unknown panic".to_string());
            format!("agent panicked: {detail}")
        }
    };
    eprintln!("[terminology-agent] skipped: {skipped_reason}");
    TaskLogger::agent_with_media(task_id, media_path).event(
        event::AGENT_SKIP,
        Some(&serde_json::json!({ "reason": skipped_reason })),
    );
    TerminologyBriefing::skipped(skipped_reason)
}

async fn run_terminology_briefing_inner<F>(
    input: TerminologyAgentInput,
    on_progress: &mut F,
) -> Result<TerminologyBriefing, String>
where
    F: FnMut(usize, usize, u32, u32, Option<&str>) + Send,
{
    let main_logger = TaskLogger::main_with_media(input.task_id.clone(), input.media_path.clone());
    let agent_logger = TaskLogger::agent_with_media(input.task_id.clone(), input.media_path.clone());
    if input.cues.is_empty() {
        agent_logger.event(
            event::AGENT_SKIP,
            Some(&serde_json::json!({ "reason": "no subtitle cues" })),
        );
        return Ok(TerminologyBriefing::skipped("no subtitle cues"));
    }
    if let Some(store) = &input.store {
        if let Some(json) = store.load_artifact(&input.task_id, TERMINOLOGY_ARTIFACT_PREFIX).await? {
            if let Ok(cached) = serde_json::from_str::<TerminologyBriefing>(&json) {
                if !cached.skipped {
                    main_logger.event(
                        event::TERMINOLOGY_AGENT_RESUMED,
                        Some(&serde_json::json!({
                            "taskId": input.task_id,
                            "glossary": cached.glossary.len(),
                        })),
                    );
                    agent_logger.event(
                        event::AGENT_RESUMED,
                        Some(&serde_json::json!({
                            "glossary": cached.glossary.len(),
                            "styleChars": cached.style_guide.chars().count(),
                            "windows": cached.windows,
                            "endReason": cached.end_reason.as_str(),
                        })),
                    );
                    return Ok(cached);
                }
            }
        }
    }

    let client = AnthropicLlmClient::new(LlmConfig::new(
        input.base_url.clone(),
        input.api_key.clone(),
        input.model.clone(),
    ))
    .map_err(|e| format!("llm client: {e}"))?;

    let windows = split_cues_windows(&input.cues);
    let n_win = windows.len().max(1);
    let (max_rounds, probe_budget) = if n_win > 1 {
        (MULTI_WINDOW_MAX_ROUNDS, PROBE_BUDGET_MULTI)
    } else {
        (DEFAULT_MAX_ROUNDS, PROBE_BUDGET_SINGLE)
    };
    let cands = candidates::extract_candidates(&input.cues);
    let confusable_pairs = candidates::find_confusable_pairs(&cands);
    let candidates_block = candidates::format_candidates_block(&cands, &confusable_pairs)
        + &candidates::format_pair_evidence(&confusable_pairs, &input.cues);
    let web_search_provider = tools::web_search_provider(&input.anysearch_api_key);

    main_logger.event(
        event::TERMINOLOGY_AGENT_START,
        Some(&serde_json::json!({
            "taskId": input.task_id,
            "windows": n_win,
            "cues": input.cues.len(),
            "maxRounds": max_rounds,
            "userTerms": input.user_terms.len(),
        })),
    );
    agent_logger.event(
        event::AGENT_START,
        Some(&serde_json::json!({
            "title": input.title,
            "sourceLang": input.source_lang,
            "targetLang": input.target_lang,
            "windows": n_win,
            "cues": input.cues.len(),
            "maxRounds": max_rounds,
            "userTerms": input.user_terms.len(),
            "webSearch": web_search_provider.is_some(),
            "model": input.model,
        })),
    );

    let mut glo_parts: Vec<Vec<GlossaryEntry>> = Vec::new();
    let mut styles: Vec<String> = Vec::new();
    let mut last_reason = EndReason::NoToolCalls;

    for (wi, win) in windows.iter().enumerate() {
        if win.is_empty() {
            continue;
        }
        if let Some(store) = &input.store {
            if let Some(json) = store
                .load_artifact(&input.task_id, &window_artifact_name(wi))
                .await?
            {
                if let Ok(cp) = serde_json::from_str::<WindowCheckpoint>(&json) {
                    agent_logger.event(
                        event::AGENT_WINDOW_CHECKPOINT,
                        Some(&serde_json::json!({
                            "window": wi + 1,
                            "windows": n_win,
                            "glossary": cp.glossary.len(),
                            "styleChars": cp.style_guide.chars().count(),
                        })),
                    );
                    glo_parts.push(cp.glossary);
                    if !cp.style_guide.is_empty() {
                        styles.push(cp.style_guide);
                    }
                    last_reason = EndReason::SubmitOk;
                    continue;
                }
            }
        }

        let window_label = if n_win > 1 {
            Some((wi + 1, n_win))
        } else {
            None
        };
        let mut on_round = |round, max, tool: Option<String>| {
            on_progress(wi + 1, n_win, round, max, tool.as_deref());
        };
        agent_logger.event(
            event::AGENT_WINDOW_START,
            Some(&serde_json::json!({
                "window": wi + 1,
                "windows": n_win,
                "cues": win.len(),
                "fromIndex": win.first().map(|c| c.index),
                "toIndex": win.last().map(|c| c.index),
            })),
        );
        let established = union_glossaries(&glo_parts);
        let cfg = AgentRunConfig {
            client: &client,
            title: &input.title,
            source_lang: &input.source_lang,
            target_lang: &input.target_lang,
            window_cues: win,
            all_cues: &input.cues,
            user_terms: &input.user_terms,
            established_terms: &established,
            candidates_block: &candidates_block,
            confusable_pairs: &confusable_pairs,
            max_rounds,
            probe_budget,
            window_label,
            task_id: &input.task_id,
            store: input.store.clone(),
            web_search: web_search_provider.as_ref(),
            logger: Some(&agent_logger),
        };
        let result = run_window_agent(&cfg, Some(&mut on_round)).await;
        last_reason = result.end_reason;
        if result.end_reason == EndReason::LlmError && n_win == 1 {
            let briefing = TerminologyBriefing::skipped(format!(
                "agent llm error after {} rounds",
                result.rounds_used
            ));
            main_logger.event(
                event::TERMINOLOGY_AGENT_END,
                Some(&serde_json::json!({
                    "taskId": input.task_id,
                    "ok": false,
                    "endReason": "llm_error",
                    "skipped": true,
                })),
            );
            agent_logger.event(
                event::AGENT_END,
                Some(&serde_json::json!({
                    "ok": false,
                    "endReason": "llm_error",
                    "skipped": true,
                    "rounds": result.rounds_used,
                })),
            );
            return Ok(briefing);
        }
        if result.end_reason == EndReason::SubmitOk
            || !result.glossary.is_empty()
            || !result.style_guide.is_empty()
        {
            glo_parts.push(result.glossary.clone());
            if !result.style_guide.is_empty() {
                styles.push(result.style_guide.clone());
            }
            if let Some(store) = &input.store {
                let cp = WindowCheckpoint {
                    glossary: result.glossary,
                    style_guide: result.style_guide,
                };
                if let Ok(json) = serde_json::to_string(&cp) {
                    let _ = store
                        .save_artifact(&input.task_id, &window_artifact_name(wi), &json)
                        .await;
                }
            }
        }
    }

    let glossary = union_glossaries(&glo_parts);
    let hay = transcript_haystack(&input.cues);
    let grounded = ground_glossary(&glossary, &hay);
    let style = merge_style_guides(&styles, &grounded, &input.target_lang);
    let skipped = grounded.is_empty() && style.is_empty() && last_reason != EndReason::SubmitOk;
    let briefing = TerminologyBriefing {
        glossary: grounded,
        style_guide: style,
        windows: n_win,
        end_reason: if skipped { EndReason::Skipped } else { last_reason },
        skipped,
        skip_reason: if skipped {
            Some(format!("agent ended: {}", last_reason.as_str()))
        } else {
            None
        },
    };

    if !briefing.skipped {
        if let Some(store) = &input.store {
            if let Ok(json) = serde_json::to_string(&briefing) {
                let _ = store
                    .save_artifact(&input.task_id, TERMINOLOGY_ARTIFACT_PREFIX, &json)
                    .await;
            }
        }
    }

    main_logger.event(
        event::TERMINOLOGY_AGENT_END,
        Some(&serde_json::json!({
            "taskId": input.task_id,
            "ok": !briefing.skipped,
            "endReason": briefing.end_reason.as_str(),
            "glossary": briefing.glossary.len(),
            "styleChars": briefing.style_guide.chars().count(),
            "windows": briefing.windows,
        })),
    );
    agent_logger.event(
        event::AGENT_END,
        Some(&serde_json::json!({
            "ok": !briefing.skipped,
            "endReason": briefing.end_reason.as_str(),
            "skipReason": briefing.skip_reason,
            "windows": briefing.windows,
            "glossary": briefing.glossary.len(),
            "styleChars": briefing.style_guide.chars().count(),
            "styleGuide": briefing.style_guide,
            "terms": briefing
                .glossary
                .iter()
                .map(|g| serde_json::json!({
                    "source": g.source,
                    "target": g.target,
                }))
                .collect::<Vec<_>>(),
        })),
    );

    Ok(briefing)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::terminology_agent::ground::merge_glossary_user_priority;

    #[test]
    fn skipped_briefing_is_fail_open() {
        let b = TerminologyBriefing::skipped("boom");
        assert!(b.skipped);
        assert!(b.glossary.is_empty());
        assert!(b.style_guide.is_empty());
    }

    #[test]
    fn merge_user_and_agent_for_pipeline() {
        let user = vec![GlossaryEntry::new("RIO", "里约", "")];
        let agent = vec![GlossaryEntry::new("Canyon", "峡谷", "")];
        let merged = merge_glossary_user_priority(&user, &agent, "RIO at Canyon");
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].target, "里约");
    }
}
