//! Terminology briefing via sequential windowed extraction.
//!
//! One small non-streaming JSON call per window (prompt ~1-1.5K tokens — a
//! shape every endpoint can serve, including small-TPM free tiers). Each
//! window receives the established glossary as compact context and reports
//! only NEW terms; a width ladder (halve on failure) is built into the loop.
//!
//! Fail-open by design: any error — including a panic — returns a skipped
//! briefing so translation still runs (user glossary only).

mod ground;
mod types;
mod windowed;

pub use ground::{merge_glossary_user_priority, source_grounded_in_text};
pub use types::{EndReason, GlossaryEntry, TerminologyBriefing, TranscriptCue};

use serde_json::json;

use crate::db::store::TaskStore;
use crate::services::llm::client::{LlmClient, LlmSemanticValidationError};
use crate::services::llm::json_guard::JsonResponseValidator;
use crate::services::llm::port::{LlmCallContext, LlmConfig};
use crate::services::task_log::{event, TaskLogger};

use ground::{ground_glossary, merge_style_guides, transcript_haystack};
use windowed::{TermJsonCaller, WindowExtraction, WindowedParams};

/// Merged briefing artifacts share this prefix. The `_v2` suffix invalidates
/// caches written by the old agent implementation; review-flow artifact
/// cleanup matches on the same prefix, so deletion still reaches them.
pub const TERMINOLOGY_ARTIFACT_PREFIX: &str = "step_03_terminology_v2";

const WINDOW_START_CHARS: usize = 4000;
const WINDOW_MIN_CHARS: usize = 1200;

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
    pub store: Option<TaskStore>,
}

/// Production [`TermJsonCaller`]: routes every window through the shared
/// `LlmClient`, so retry budget, the JSON repair chain, protocol reroute and
/// usage accounting (llm.log `llm.call` lines with `phase="terminology"`,
/// plus task_usage rows) are all inherited unchanged.
struct LlmTermCaller<'a> {
    client: &'a LlmClient,
    context: LlmCallContext,
}

#[async_trait::async_trait]
impl TermJsonCaller for LlmTermCaller<'_> {
    async fn call_window(&self, request_id: &str, prompt: &str) -> Result<WindowExtraction, crate::services::llm::error::LlmError> {
        let validator = JsonResponseValidator::with_required_keys(&["terms"]);
        let result = self
            .client
            .call_json_validated(&self.context, request_id, prompt, Some(&validator), |value| {
                serde_json::from_value::<WindowExtraction>(value).map_err(|err| {
                    LlmSemanticValidationError::retryable(format!("window extraction shape: {err}"))
                })
            })
            .await?;
        Ok(result.value)
    }
}

/// Run the terminology extraction. Never returns Err — failures become
/// `skipped`, and even a panic inside the extraction is caught so translation
/// always continues with just the user's own glossary.
pub async fn run_terminology_briefing<F>(
    input: TerminologyAgentInput,
    mut on_progress: F,
) -> TerminologyBriefing
where
    F: FnMut(usize, usize, usize) + Send,
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
            format!("terminology extraction panicked: {detail}")
        }
    };
    crate::services::app_log::error(
        "terminology",
        "TERMINOLOGY_SKIPPED",
        &skipped_reason,
        Some(serde_json::json!({ "taskId": &task_id })),
    );
    TaskLogger::main_with_media(task_id, media_path).event(
        event::TERMINOLOGY_SKIP,
        Some(&json!({ "reason": skipped_reason })),
    );
    TerminologyBriefing::skipped(skipped_reason)
}

async fn run_terminology_briefing_inner<F>(
    input: TerminologyAgentInput,
    on_progress: &mut F,
) -> Result<TerminologyBriefing, String>
where
    F: FnMut(usize, usize, usize) + Send,
{
    let main_logger = TaskLogger::main_with_media(input.task_id.clone(), input.media_path.clone());
    if input.cues.is_empty() {
        main_logger.event(
            event::TERMINOLOGY_SKIP,
            Some(&json!({ "reason": "no subtitle cues" })),
        );
        return Ok(TerminologyBriefing::skipped("no subtitle cues"));
    }

    // Merged-briefing cache: a completed run (or a previous partial with the
    // same width) is reused verbatim on restart.
    if let Some(store) = &input.store
        && let Some(json) = store
            .load_artifact(&input.task_id, TERMINOLOGY_ARTIFACT_PREFIX)
            .await?
        && let Ok(cached) = serde_json::from_str::<TerminologyBriefing>(&json)
        && !cached.skipped
    {
        main_logger.event(
            event::TERMINOLOGY_RESUMED,
            Some(&json!({
                "taskId": input.task_id,
                "glossary": cached.glossary.len(),
                "styleChars": cached.style_guide.chars().count(),
                "windows": cached.windows,
                "endReason": cached.end_reason.as_str(),
            })),
        );
        return Ok(cached);
    }

    let client = LlmClient::new(LlmConfig::new(
        input.base_url.clone(),
        input.api_key.clone(),
        input.model.clone(),
    ))
    .map_err(|e| format!("llm client: {e}"))?;

    main_logger.event(
        event::TERMINOLOGY_START,
        Some(&json!({
            "taskId": input.task_id,
            "cues": input.cues.len(),
            "windowChars": WINDOW_START_CHARS,
            "userTerms": input.user_terms.len(),
            "model": input.model,
        })),
    );

    let caller = LlmTermCaller {
        client: &client,
        context: LlmCallContext {
            task_id: input.task_id.clone(),
            media_path: Some(input.media_path.clone()),
            phase: "terminology".to_string(),
            store: input.store.clone(),
        },
    };

    let mut on_progress_ref = |done: usize, est: usize, width: usize| {
        on_progress(done, est, width);
    };
    let outcome = windowed::run_windowed_extraction(WindowedParams {
        cues: &input.cues,
        user_terms: &input.user_terms,
        source_lang: &input.source_lang,
        target_lang: &input.target_lang,
        start_window_chars: WINDOW_START_CHARS,
        min_window_chars: WINDOW_MIN_CHARS,
        caller: &caller,
        on_progress: &mut on_progress_ref,
    })
    .await;

    // Ground every entry against the full transcript; ungrounded rows are
    // hallucinations or formatting debris and must not reach the translator.
    let hay = transcript_haystack(&input.cues);
    let grounded = ground_glossary(&outcome.glossary, &hay);
    let style = merge_style_guides(std::slice::from_ref(&outcome.style_guide), &grounded, &input.target_lang);
    let skipped = grounded.is_empty() && style.is_empty();
    let briefing = TerminologyBriefing {
        glossary: grounded,
        style_guide: style,
        windows: outcome.windows_done,
        end_reason: if skipped { EndReason::Skipped } else { outcome.end_reason },
        skipped,
        skip_reason: if skipped {
            Some(format!("terminology extraction ended: {}", outcome.end_reason.as_str()))
        } else {
            None
        },
    };

    if !briefing.skipped
        && let Some(store) = &input.store
        && let Ok(json) = serde_json::to_string(&briefing)
    {
        let _ = store
            .save_artifact(&input.task_id, TERMINOLOGY_ARTIFACT_PREFIX, &json)
            .await;
    }

    main_logger.event(
        event::TERMINOLOGY_END,
        Some(&json!({
            "taskId": input.task_id,
            "ok": !briefing.skipped,
            "endReason": briefing.end_reason.as_str(),
            "glossary": briefing.glossary.len(),
            "styleChars": briefing.style_guide.chars().count(),
            "windows": briefing.windows,
            "terms": briefing
                .glossary
                .iter()
                .map(|g| json!({"source": g.source, "target": g.target}))
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
