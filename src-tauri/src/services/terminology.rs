use std::collections::HashSet;

use crate::services::llm::client::OpenAiCompatLlmClient;
use crate::services::llm::json_guard::JsonResponseValidator;
use crate::services::llm::port::{LlmCallContext, LlmConfig, next_llm_request_id};
use crate::services::prompts::terminology::{
    IndexedUserTermPromptItem, build_extract_terms_prompt, build_theme_prompt,
    build_user_filter_prompt,
};
use crate::services::terminology_responses::{
    parse_extracted_terms_response, parse_theme_response, parse_user_term_filter_response,
};
use crate::services::terminology_terms::{merge_terms_with_user_priority, normalize_entries};
use crate::services::terminology_text::{build_context_text, normalize_theme};

const DEFAULT_THEME: &str = "内容围绕一个明确主题展开。";
const MAX_EXTRACT_TERMS: usize = 24;

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
}

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
    ))
    ?;

    let context_text = build_context_text(&request.segments);
    if context_text.trim().is_empty() {
        return Err("segments contain no text".to_string());
    }

    let theme = extract_theme(&request, &llm_client, &context_text).await?;
    let user_terms = normalize_entries(request.terminology_entries.clone());
    let filtered_user_terms =
        filter_user_terms(&request, &llm_client, &context_text, &theme, &user_terms)
            .await
            .unwrap_or(user_terms);
    let extracted_terms = extract_terms(&request, &llm_client, &context_text, &theme).await?;
    let merged = merge_terms_with_user_priority(&filtered_user_terms, &extracted_terms);

    Ok(BuildTerminologyLayerResponse {
        theme_summary: theme,
        terminology_entries: merged,
    })
}

async fn extract_theme(
    request: &BuildTerminologyLayerRequest,
    llm_client: &OpenAiCompatLlmClient,
    context_text: &str,
) -> Result<String, String> {
    let prompt = build_theme_prompt(&request.source_lang, &request.target_lang, context_text);
    let validator = JsonResponseValidator::with_required_keys(&["theme"]);
    let context = LlmCallContext {
        task_id: request.task_id.clone(),
        media_path: Some(request.media_path.clone()),
        phase: "step3_theme".to_string(),
    };
    let llm_id = next_llm_request_id();

    let result = llm_client
        .call_json_validated(
            &context,
            &llm_id,
            &prompt,
            Some(&validator),
            parse_theme_response,
        )
        .await
        .map_err(|err| {
            format!(
                "build terminology theme failed (llmId={}): {}",
                llm_id, err.message
            )
        })?;

    let normalized = normalize_theme(&result.value);
    if normalized.is_empty() {
        Ok(DEFAULT_THEME.to_string())
    } else {
        Ok(normalized)
    }
}

async fn filter_user_terms(
    request: &BuildTerminologyLayerRequest,
    llm_client: &OpenAiCompatLlmClient,
    context_text: &str,
    theme: &str,
    user_terms: &[TerminologyEntry],
) -> Result<Vec<TerminologyEntry>, String> {
    if user_terms.is_empty() {
        return Ok(Vec::new());
    }

    let indexed = user_terms
        .iter()
        .enumerate()
        .map(|(idx, entry)| IndexedUserTermPromptItem {
            index: idx + 1,
            source: entry.source.clone(),
            target: entry.target.clone(),
            note: entry.note.clone(),
        })
        .collect::<Vec<_>>();

    let prompt = build_user_filter_prompt(
        &request.source_lang,
        &request.target_lang,
        theme,
        context_text,
        &indexed,
    );
    let context = LlmCallContext {
        task_id: request.task_id.clone(),
        media_path: Some(request.media_path.clone()),
        phase: "step3_filter_user_terms".to_string(),
    };
    let llm_id = next_llm_request_id();

    let result = llm_client
        .call_json_validated(
            &context,
            &llm_id,
            &prompt,
            None,
            parse_user_term_filter_response,
        )
        .await
        .map_err(|err| {
            format!(
                "filter user terminology failed (llmId={}): {}",
                llm_id, err.message
            )
        })?;

    let mut selected = Vec::<TerminologyEntry>::new();
    let mut seen = HashSet::<usize>::new();
    for raw in result.value {
        if raw == 0 || raw > user_terms.len() {
            continue;
        }
        if !seen.insert(raw) {
            continue;
        }
        selected.push(user_terms[raw - 1].clone());
    }

    Ok(selected)
}

async fn extract_terms(
    request: &BuildTerminologyLayerRequest,
    llm_client: &OpenAiCompatLlmClient,
    context_text: &str,
    theme: &str,
) -> Result<Vec<TerminologyEntry>, String> {
    let prompt = build_extract_terms_prompt(
        &request.source_lang,
        &request.target_lang,
        theme,
        context_text,
        MAX_EXTRACT_TERMS,
    );
    let context = LlmCallContext {
        task_id: request.task_id.clone(),
        media_path: Some(request.media_path.clone()),
        phase: "step3_extract_terms".to_string(),
    };
    let llm_id = next_llm_request_id();

    let result = llm_client
        .call_json_validated(
            &context,
            &llm_id,
            &prompt,
            None,
            parse_extracted_terms_response,
        )
        .await
        .map_err(|err| {
            format!(
                "extract terminology failed (llmId={}): {}",
                llm_id, err.message
            )
        })?;

    Ok(normalize_entries(result.value))
}
