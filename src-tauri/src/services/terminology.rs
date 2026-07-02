use std::collections::HashMap;
use std::sync::Arc;

use crate::services::llm::batch::run_indexed_concurrent_idempotent;
use crate::services::llm::client::OpenAiCompatLlmClient;
use crate::services::llm::port::{LlmCallContext, LlmConfig, next_llm_request_id};
use crate::services::prompts::terminology::{IndexedUserTermPromptItem, build_briefing_prompt};
use crate::services::terminology_responses::{BriefingResponse, parse_briefing_response};
use crate::services::terminology_terms::{force_include_user_terms, normalize_entries};
use crate::services::terminology_text::{build_context_lines, chunk_lines};

#[derive(Debug, Clone)]
pub struct TerminologyToken {
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct TerminologySegment {
    pub segment: String,
    pub tokens: Vec<TerminologyToken>,
}

#[derive(Debug, Clone)]
pub struct TerminologyEntry {
    pub source: String,
    pub target: String,
    pub note: String,
}

#[derive(Debug, Clone)]
pub struct BuildTerminologyLayerRequest {
    pub task_id: String,
    pub media_path: String,
    pub source_lang: String,
    pub target_lang: String,
    pub segments: Vec<TerminologySegment>,
    pub terminology_entries: Vec<TerminologyEntry>,
    pub translate_api_key: String,
    pub translate_base_url: String,
    pub translate_model: String,
    pub llm_concurrency: u32,
}

/// `theme_summary` is a legacy field name kept for checkpoint/command
/// compatibility; it now carries the briefing STYLE GUIDE (free-form,
/// content-adaptive translator guidance) rather than a one-line topic.
#[derive(Debug, Clone)]
pub struct BuildTerminologyLayerResponse {
    pub theme_summary: String,
    pub terminology_entries: Vec<TerminologyEntry>,
}

pub async fn build_terminology_layer(
    request: BuildTerminologyLayerRequest,
) -> Result<BuildTerminologyLayerResponse, String> {
    if request.task_id.trim().is_empty() {
        return Err("taskId is required".to_string());
    }
    if request.media_path.trim().is_empty() {
        return Err("mediaPath is required".to_string());
    }
    if request.source_lang.trim().is_empty() {
        return Err("sourceLang is required".to_string());
    }
    if request.target_lang.trim().is_empty() {
        return Err("targetLang is required".to_string());
    }
    if request.segments.is_empty() {
        return Err("segments is required".to_string());
    }
    if request.translate_api_key.trim().is_empty() {
        return Err("translateApiKey is required".to_string());
    }
    if request.translate_base_url.trim().is_empty() {
        return Err("translateBaseUrl is required".to_string());
    }
    if request.translate_model.trim().is_empty() {
        return Err("translateModel is required".to_string());
    }

    let llm_client = OpenAiCompatLlmClient::new(LlmConfig::new(
        request.translate_base_url.clone(),
        request.translate_api_key.clone(),
        request.translate_model.clone(),
    ))?;

    let context_lines = build_context_lines(&request.segments);
    if context_lines.is_empty() {
        return Err("segments contain no text".to_string());
    }
    let windows = chunk_lines(&context_lines);
    let user_terms = normalize_entries(request.terminology_entries.clone());
    let user_prompt_items = indexed_user_terms(&user_terms);

    // One briefing call per window, run concurrently up to `llm_concurrency`.
    // Glossary terms are unioned across windows (full transcript coverage);
    // the style guide is taken from window index 0 (high-level guidance that
    // frames the whole video). Windows are independent — each extracts from
    // its own 8000-char slice with no ordering dependency — so they are safe
    // to dispatch in parallel, exactly like the Step4 translation batches.
    let concurrency = request.llm_concurrency.max(1) as usize;
    let context = LlmCallContext {
        task_id: request.task_id.clone(),
        media_path: Some(request.media_path.clone()),
        phase: "step3_briefing".to_string(),
        store: None,
    };
    let source_lang = request.source_lang.clone();
    let target_lang = request.target_lang.clone();
    let on_progress: Option<Arc<dyn Fn(usize, usize) + Send + Sync>> = None;

    let results = run_indexed_concurrent_idempotent(
        windows.clone(),
        concurrency,
        {
            let llm_client = llm_client.clone();
            let context = context.clone();
            let user_prompt_items = user_prompt_items.clone();
            let source_lang = source_lang.clone();
            let target_lang = target_lang.clone();
            move |window| {
                let llm_client = llm_client.clone();
                let context = context.clone();
                let user_prompt_items = user_prompt_items.clone();
                let source_lang = source_lang.clone();
                let target_lang = target_lang.clone();
                async move {
                    build_briefing_for_window(
                        &llm_client,
                        &context,
                        &source_lang,
                        &target_lang,
                        &window,
                        &user_prompt_items,
                    )
                    .await
                }
            }
        },
        |msg| msg,
        move |_done, _total, _result| {
            if let Some(callback) = on_progress.as_ref() {
                callback(_done, _total);
            }
        },
        HashMap::new(),
        |_idx, _val| async { Ok(()) },
    )
    .await;

    // Style guide comes from window index 0 (not "first to complete").
    let mut all_extracted: Vec<TerminologyEntry> = Vec::new();
    let mut style_guide: Option<String> = None;
    for (idx, item) in results {
        let briefing = item?;
        all_extracted.extend(briefing.glossary);
        if idx == 0 {
            style_guide = Some(briefing.style_guide);
        }
    }

    let extracted = normalize_entries(all_extracted);
    let glossary = force_include_user_terms(&user_terms, &extracted);

    Ok(BuildTerminologyLayerResponse {
        theme_summary: style_guide.unwrap_or_default(),
        terminology_entries: glossary,
    })
}

/// Number of Step3 briefing windows a transcript will produce. Exposed so the
/// eval harness can report an accurate per-call count without a data-contract
/// change.
pub fn briefing_window_count(segments: &[TerminologySegment]) -> usize {
    chunk_lines(&build_context_lines(segments)).len()
}

async fn build_briefing_for_window(
    llm_client: &OpenAiCompatLlmClient,
    context: &LlmCallContext,
    source_lang: &str,
    target_lang: &str,
    window: &str,
    user_terms: &[IndexedUserTermPromptItem],
) -> Result<BriefingResponse, String> {
    let prompt = build_briefing_prompt(source_lang, target_lang, window, user_terms);
    let llm_id = next_llm_request_id();

    llm_client
        .call_json_validated(context, &llm_id, &prompt, None, None, parse_briefing_response)
        .await
        .map(|result| result.value)
        .map_err(|err| {
            format!(
                "build terminology briefing failed (llmId={}): {}",
                llm_id, err.message
            )
        })
}

fn indexed_user_terms(user_terms: &[TerminologyEntry]) -> Vec<IndexedUserTermPromptItem> {
    user_terms
        .iter()
        .enumerate()
        .map(|(idx, entry)| IndexedUserTermPromptItem {
            index: idx + 1,
            source: entry.source.clone(),
            target: entry.target.clone(),
            note: entry.note.clone(),
        })
        .collect()
}
