use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::services::llm::client::{LlmSemanticValidationError, OpenAiCompatLlmClient};
use crate::services::llm::json_guard::JsonResponseValidator;
use crate::services::llm::port::{LlmCallContext, LlmConfig, next_llm_request_id};

const DEFAULT_THEME: &str = "内容围绕一个明确主题展开。";
const MAX_CONTEXT_CHARS: usize = 8_000;
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

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ThemeExtraction {
    #[serde(default)]
    theme: String,
    #[serde(default)]
    output: Option<ThemeOutputExtraction>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct UserTermFilterExtraction {
    #[serde(default)]
    #[serde(alias = "keep_indexes")]
    keep_indexes: Vec<usize>,
    #[serde(default)]
    output: Option<UserTermFilterOutputExtraction>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ExtractedTermsExtraction {
    #[serde(default)]
    terms: Vec<TerminologyEntryExtraction>,
    #[serde(default)]
    output: Option<ExtractedTermsOutputExtraction>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ThemeOutputExtraction {
    #[serde(default)]
    theme: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct UserTermFilterOutputExtraction {
    #[serde(default)]
    #[serde(alias = "keep_indexes")]
    keep_indexes: Vec<usize>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ExtractedTermsOutputExtraction {
    #[serde(default)]
    terms: Vec<TerminologyEntryExtraction>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct TerminologyEntryExtraction {
    #[serde(default)]
    source: String,
    #[serde(default)]
    target: String,
    #[serde(default)]
    note: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct IndexedUserTermPromptItem {
    index: usize,
    source: String,
    target: String,
    note: String,
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
    .map_err(|err| err.message)?;

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
        .call_json_validated(&context, &llm_id, &prompt, Some(&validator), |value| {
            serde_json::from_value::<ThemeExtraction>(value).map_err(|err| {
                LlmSemanticValidationError::retryable(format!("theme parse failed: {err}"))
            })
        })
        .await
        .map_err(|err| {
            format!(
                "build terminology theme failed (llmId={}): {}",
                llm_id, err.message
            )
        })?;

    let theme_text = if !result.value.theme.trim().is_empty() {
        result.value.theme
    } else {
        result
            .value
            .output
            .as_ref()
            .map(|item| item.theme.clone())
            .unwrap_or_default()
    };
    let normalized = normalize_theme(&theme_text);
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
        .call_json_validated(&context, &llm_id, &prompt, None, |value| {
            serde_json::from_value::<UserTermFilterExtraction>(value).map_err(|err| {
                LlmSemanticValidationError::retryable(format!(
                    "user term filter parse failed: {err}"
                ))
            })
        })
        .await
        .map_err(|err| {
            format!(
                "filter user terminology failed (llmId={}): {}",
                llm_id, err.message
            )
        })?;

    let mut selected = Vec::<TerminologyEntry>::new();
    let mut seen = HashSet::<usize>::new();
    let keep_indexes = if result.value.keep_indexes.is_empty() {
        result
            .value
            .output
            .as_ref()
            .map(|item| item.keep_indexes.clone())
            .unwrap_or_default()
    } else {
        result.value.keep_indexes
    };

    for raw in keep_indexes {
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
        .call_json_validated(&context, &llm_id, &prompt, None, |value| {
            serde_json::from_value::<ExtractedTermsExtraction>(value).map_err(|err| {
                LlmSemanticValidationError::retryable(format!(
                    "term extraction parse failed: {err}"
                ))
            })
        })
        .await
        .map_err(|err| {
            format!(
                "extract terminology failed (llmId={}): {}",
                llm_id, err.message
            )
        })?;

    let extracted = if result.value.terms.is_empty() {
        result
            .value
            .output
            .as_ref()
            .map(|item| item.terms.clone())
            .unwrap_or_default()
    } else {
        result.value.terms
    };

    Ok(normalize_entries(
        extracted
            .into_iter()
            .map(|entry| TerminologyEntry {
                source: entry.source,
                target: entry.target,
                note: entry.note,
            })
            .collect(),
    ))
}

fn merge_terms_with_user_priority(
    user_terms: &[TerminologyEntry],
    extracted_terms: &[TerminologyEntry],
) -> Vec<TerminologyEntry> {
    let mut out = Vec::<TerminologyEntry>::new();
    let mut seen_source = HashSet::<String>::new();

    for entry in user_terms {
        let key = entry.source.to_ascii_lowercase();
        if key.is_empty() || !seen_source.insert(key) {
            continue;
        }
        out.push(entry.clone());
    }

    for entry in extracted_terms {
        let key = entry.source.to_ascii_lowercase();
        if key.is_empty() || !seen_source.insert(key) {
            continue;
        }
        out.push(entry.clone());
    }

    out
}

fn normalize_entries(entries: Vec<TerminologyEntry>) -> Vec<TerminologyEntry> {
    let mut out = Vec::<TerminologyEntry>::new();
    let mut seen = HashSet::<(String, String)>::new();

    for entry in entries {
        let source = normalize_inline_text(&entry.source);
        let target = normalize_inline_text(&entry.target);
        let note = normalize_inline_text(&entry.note);
        if source.is_empty() || target.is_empty() {
            continue;
        }
        let key = (source.to_ascii_lowercase(), target.to_ascii_lowercase());
        if !seen.insert(key) {
            continue;
        }
        out.push(TerminologyEntry {
            source,
            target,
            note,
        });
    }

    out
}

fn build_context_text(segments: &[TerminologySegment]) -> String {
    let lines = segments
        .iter()
        .filter_map(|segment| {
            if !segment.segment.trim().is_empty() {
                return Some(normalize_inline_text(&segment.segment));
            }
            let text = segment
                .tokens
                .iter()
                .map(|token| token.text.trim())
                .filter(|token| !token.is_empty())
                .collect::<Vec<_>>()
                .join(" ");
            let normalized = normalize_inline_text(&text);
            if normalized.is_empty() {
                None
            } else {
                Some(normalized)
            }
        })
        .collect::<Vec<_>>();

    truncate_chars(&lines.join("\n"), MAX_CONTEXT_CHARS)
}

fn normalize_theme(raw: &str) -> String {
    normalize_inline_text(raw)
}

fn normalize_inline_text(raw: &str) -> String {
    raw.replace('\r', " ")
        .replace('\n', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string()
}

fn truncate_chars(input: &str, max_chars: usize) -> String {
    if input.chars().count() <= max_chars {
        return input.to_string();
    }
    input.chars().take(max_chars).collect::<String>()
}

fn build_theme_prompt(source_lang: &str, target_lang: &str, context_text: &str) -> String {
    serde_json::json!({
        "task": "summarize_video_theme_for_terminology",
        "rule": "Return JSON only.",
        "sourceLanguage": source_lang,
        "targetLanguage": target_lang,
        "transcript": context_text,
        "goal": "Summarize the dominant topic and field of this transcript for terminology selection.",
        "output": {
            "theme": "One concise sentence."
        }
    })
    .to_string()
}

fn build_user_filter_prompt(
    source_lang: &str,
    target_lang: &str,
    theme: &str,
    context_text: &str,
    terms: &[IndexedUserTermPromptItem],
) -> String {
    serde_json::json!({
        "task": "filter_user_terminology_by_video_relevance",
        "rule": "Return JSON only.",
        "sourceLanguage": source_lang,
        "targetLanguage": target_lang,
        "theme": theme,
        "transcript": context_text,
        "userTerms": terms,
        "goal": "Keep only terms that are relevant to this video's domain and content.",
        "output": {
            "keepIndexes": [1, 2]
        }
    })
    .to_string()
}

fn build_extract_terms_prompt(
    source_lang: &str,
    target_lang: &str,
    theme: &str,
    context_text: &str,
    max_terms: usize,
) -> String {
    serde_json::json!({
        "task": "extract_domain_terminology_for_translation_consistency",
        "rule": "Return JSON only.",
        "sourceLanguage": source_lang,
        "targetLanguage": target_lang,
        "theme": theme,
        "transcript": context_text,
        "constraints": {
            "maxTerms": max_terms,
            "focus": "domain terminology, named entities, fixed expressions in this context",
            "avoid": "full clauses, long sentence fragments, generic filler words"
        },
        "output": {
            "terms": [
                {
                    "source": "term in source language",
                    "target": "target translation",
                    "note": "optional short context note"
                }
            ]
        }
    })
    .to_string()
}
