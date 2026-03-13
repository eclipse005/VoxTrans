use crate::llm::LlmInteractRequest;
use crate::prompt_builder::{
    BuildTranslationProfilePromptRequest, BuildTranslationPromptRequest,
    TranslationPromptTerm, build_translation_profile_prompt, build_translation_prompt,
};
use crate::services::translation::domain::{
    SentenceUnit, SourceCue, TranslatedUnit, TranslationProfile, TranslationTerm,
};
use crate::services::translation::json_tool::parse_llm_json_response;
use serde_json::Value;

const TRANSLATE_BATCH_SIZE: usize = 40;

#[derive(Debug, Clone)]
pub struct TranslationLlmClient {
    config: TranslationLlmRuntimeConfig,
}

#[derive(Debug, Clone)]
pub struct TranslationLlmRuntimeConfig {
    pub api_key: String,
    pub base_url: Option<String>,
    pub model: String,
    pub max_concurrency: usize,
    pub timeout_secs: Option<u64>,
    pub max_retries: Option<u32>,
    pub log_task_id: Option<String>,
    pub log_media_path: Option<String>,
    pub usage_pool: Option<sqlx::SqlitePool>,
}

#[derive(Debug, Clone)]
struct TranslateBatchRequest {
    batch_idx: usize,
    sentence_ids_in_order: Vec<String>,
    user_prompt: String,
}

#[derive(Debug, Clone)]
struct TranslateBatchResult {
    batch_idx: usize,
    units: Vec<TranslatedUnit>,
}

impl TranslationLlmClient {
    pub fn new(config: TranslationLlmRuntimeConfig) -> Self {
        Self { config }
    }

    pub async fn summary_task(
        &self,
        cues: &[SourceCue],
        source_language: &str,
        target_language: &str,
        preferred_translation_style: Option<&str>,
        terms: &[TranslationTerm],
    ) -> Result<TranslationProfile, String> {
        self.validate_config()?;

        let sample_texts = cues
            .iter()
            .map(|c| c.source_text.trim().to_string())
            .filter(|s| !s.is_empty())
            .take(120)
            .collect::<Vec<_>>()
            ;

        let prompts = build_translation_profile_prompt(BuildTranslationProfilePromptRequest {
            source_language: source_language.to_string(),
            target_language: target_language.to_string(),
            style: preferred_translation_style.map(str::to_string),
            terms: to_prompt_terms(terms),
            sample_texts,
        })?;

        let raw = self
            .chat_json(
                &prompts.system_prompt,
                &prompts.user_prompt,
                Some("summary"),
            )
            .await?;
        let topic_summary = string_from_json(&raw, &["topicSummary", "topic_summary"])
            .or_else(|| string_from_json(&raw, &["theme"]))
            .unwrap_or_else(|| "待分析".to_string());
        let content_style = string_or_array_from_json(&raw, &["contentStyle", "content_style"])
            .unwrap_or_else(|| "未定义".to_string());
        let translation_style = string_from_json(&raw, &["translationStyle", "translation_style"])
            .or_else(|| {
                preferred_translation_style
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
            })
            .unwrap_or_else(|| "自然流畅、忠实原意".to_string());
        let primary_terms = terms_array_from_json(&raw, &["primaryTerms", "primary_terms"])
            .map(|subset| filter_terminology_subset(&subset, terms))
            .unwrap_or_default();
        let supporting_terms = terms_array_from_json(&raw, &["supportingTerms", "supporting_terms"])
            .map(|subset| filter_terminology_subset(&subset, terms))
            .unwrap_or_default();
        let terminology_subset = merge_terms_unique(&primary_terms, &supporting_terms);

        Ok(TranslationProfile {
            topic_summary,
            content_style,
            translation_style,
            terminology_subset,
            primary_terms,
            supporting_terms,
        })
    }

    pub async fn translate_sentences(
        &self,
        source_language: &str,
        target_language: &str,
        style: Option<&str>,
        profile_topic_summary: Option<&str>,
        primary_terms: &[TranslationTerm],
        supporting_terms: &[TranslationTerm],
        sentences: &[SentenceUnit],
    ) -> Result<Vec<TranslatedUnit>, String> {
        self.validate_config()?;
        if sentences.is_empty() {
            return Ok(Vec::new());
        }

        let all_lines = sentences
            .iter()
            .map(|s| s.source_text.clone())
            .collect::<Vec<_>>();
        let total_batches = sentences.len().div_ceil(TRANSLATE_BATCH_SIZE);
        let terms_union = merge_terms_unique(primary_terms, supporting_terms);
        let system_prompt = format!(
            "You are a professional Netflix subtitle translator, fluent in both {} and {}, with strong domain terminology consistency.",
            source_language, target_language
        );

        let requests = sentences
            .chunks(TRANSLATE_BATCH_SIZE)
            .enumerate()
            .map(|(batch_idx, batch)| {
                let sentence_ids_in_order = batch
                    .iter()
                    .map(|unit| unit.sentence_id.clone())
                    .collect::<Vec<_>>();
                let batch_start = batch_idx * TRANSLATE_BATCH_SIZE;
                let batch_end = batch_start + batch.len();
                let previous_lines = all_lines
                    .iter()
                    .take(batch_start)
                    .rev()
                    .take(3)
                    .cloned()
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect::<Vec<_>>();
                let next_lines = all_lines
                    .iter()
                    .skip(batch_end)
                    .take(3)
                    .cloned()
                    .collect::<Vec<_>>();
                let batch_lines = batch
                    .iter()
                    .map(|u| u.source_text.clone())
                    .collect::<Vec<_>>();
                let batch_text = batch_lines.join("\n").to_lowercase();
                let near_text = format!("{}\n{}\n{}", previous_lines.join("\n"), batch_lines.join("\n"), next_lines.join("\n")).to_lowercase();
                let focus_terms = primary_terms
                    .iter()
                    .filter(|t| !t.source.trim().is_empty())
                    .filter(|t| batch_text.contains(&t.source.to_lowercase()))
                    .cloned()
                    .collect::<Vec<_>>();
                let jit_supporting_terms = supporting_terms
                    .iter()
                    .filter(|t| !t.source.trim().is_empty())
                    .filter(|t| near_text.contains(&t.source.to_lowercase()))
                    .filter(|t| !focus_terms.iter().any(|f| f.source.eq_ignore_ascii_case(&t.source)))
                    .take(20)
                    .cloned()
                    .collect::<Vec<_>>();
                let shared_prompt = build_shared_prompt(
                    source_language,
                    target_language,
                    profile_topic_summary.unwrap_or(""),
                    primary_terms,
                    batch_idx + 1,
                    total_batches,
                    &previous_lines,
                    &next_lines,
                    &focus_terms,
                    &jit_supporting_terms,
                    batch_lines.len(),
                )?;
                let prompts = build_translation_prompt(BuildTranslationPromptRequest {
                    source_language: source_language.to_string(),
                    target_language: target_language.to_string(),
                    style: style.map(str::to_string),
                    profile_topic_summary: profile_topic_summary.map(str::to_string),
                    terminology_subset: to_prompt_terms(&terms_union),
                    shared_prompt,
                    lines: batch_lines,
                })?;
                Ok(TranslateBatchRequest {
                    batch_idx,
                    sentence_ids_in_order,
                    user_prompt: prompts.user_prompt,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;

        let mut batch_results = Vec::with_capacity(requests.len());
        for request_group in requests.chunks(self.config.max_concurrency.max(1)) {
            let mut handles = Vec::with_capacity(request_group.len());
            for batch_request in request_group {
                let client = self.clone();
                let system_prompt = system_prompt.clone();
                let request = batch_request.clone();
                handles.push(tauri::async_runtime::spawn(async move {
                    client
                        .translate_batch(request, &system_prompt)
                        .await
                }));
            }
            for handle in handles {
                let result = handle
                    .await
                    .map_err(|e| format!("translation batch task join error: {}", e))??;
                batch_results.push(result);
            }
        }

        batch_results.sort_by_key(|r| r.batch_idx);
        let mut out = Vec::with_capacity(sentences.len());
        for result in batch_results {
            out.extend(result.units);
        }
        Ok(out)
    }

    fn validate_config(&self) -> Result<(), String> {
        if self.config.api_key.trim().is_empty() {
            return Err("translation llm apiKey is required".to_string());
        }
        if self.config.model.trim().is_empty() {
            return Err("translation llm model is required".to_string());
        }
        Ok(())
    }

    async fn chat_json(
        &self,
        system_prompt: &str,
        user_prompt: &str,
        stage: Option<&str>,
    ) -> Result<Value, String> {
        let response = crate::llm::llm_interact(LlmInteractRequest {
            api_key: self.config.api_key.clone(),
            model: self.config.model.clone(),
            base_url: self.config.base_url.clone(),
            system_prompt: Some(system_prompt.to_string()),
            prompt: Some(user_prompt.to_string()),
            messages: None,
            mode: Some("chat".to_string()),
            tools: None,
            tool_results: None,
            tool_choice: None,
            temperature: None,
            max_tokens: None,
            timeout_secs: self.config.timeout_secs,
            max_retries: self.config.max_retries,
            log_task_id: self.config.log_task_id.clone(),
            log_media_path: self.config.log_media_path.clone(),
            log_stage: stage.map(str::to_string),
            usage_pool: self.config.usage_pool.clone(),
        })
        .await?;

        let content = response
            .message
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| "empty llm response content".to_string())?;
        parse_llm_json_response(content)
    }

    async fn translate_batch(
        &self,
        request: TranslateBatchRequest,
        system_prompt: &str,
    ) -> Result<TranslateBatchResult, String> {
        let raw = self
            .chat_json(system_prompt, &request.user_prompt, Some("translate"))
            .await?;

        let translated_object = raw
            .as_object()
            .ok_or_else(|| format!("translation response must be JSON object: {}", raw))?;

        let units = request
            .sentence_ids_in_order
            .iter()
            .enumerate()
            .map(|(idx, sentence_id)| {
                let key = (idx + 1).to_string();
                let translated_text = translated_object
                    .get(&key)
                    .and_then(extract_translation_text)
                    .unwrap_or_default();
                TranslatedUnit {
                    sentence_id: sentence_id.clone(),
                    translated_text,
                }
            })
            .collect::<Vec<_>>();

        Ok(TranslateBatchResult {
            batch_idx: request.batch_idx,
            units,
        })
    }
}

fn string_from_json(value: &Value, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(s) = value.get(*key).and_then(|v| v.as_str()) {
            let trimmed = s.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

fn string_or_array_from_json(value: &Value, keys: &[&str]) -> Option<String> {
    if let Some(single) = string_from_json(value, keys) {
        return Some(single);
    }
    for key in keys {
        let Some(arr) = value.get(*key).and_then(|v| v.as_array()) else {
            continue;
        };
        let items = arr
            .iter()
            .filter_map(|v| v.as_str().map(str::trim))
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect::<Vec<_>>();
        if !items.is_empty() {
            return Some(items.join("、"));
        }
    }
    None
}

fn terms_array_from_json(value: &Value, keys: &[&str]) -> Option<Vec<TranslationTerm>> {
    for key in keys {
        let Some(arr) = value.get(*key).and_then(|v| v.as_array()) else {
            continue;
        };
        let mut out = Vec::new();
        for item in arr {
            if let Some(obj) = item.as_object() {
                let source = obj
                    .get("source")
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .unwrap_or("")
                    .to_string();
                if source.is_empty() {
                    continue;
                }
                let target = obj
                    .get("target")
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .unwrap_or("")
                    .to_string();
                let note = obj
                    .get("note")
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .unwrap_or("")
                    .to_string();
                out.push(TranslationTerm {
                    source,
                    target,
                    note,
                });
            }
        }
        return Some(out);
    }
    None
}

fn filter_terminology_subset(
    subset: &[TranslationTerm],
    allowed: &[TranslationTerm],
) -> Vec<TranslationTerm> {
    if allowed.is_empty() {
        return Vec::new();
    }
    let allowed_by_source = allowed
        .iter()
        .filter_map(|term| {
            let key = term.source.trim().to_lowercase();
            if key.is_empty() {
                None
            } else {
                Some((key, term.clone()))
            }
        })
        .collect::<std::collections::HashMap<_, _>>();
    let mut seen = std::collections::HashSet::<String>::new();
    let mut out = Vec::new();
    for item in subset {
        let key = item.source.trim().to_lowercase();
        if key.is_empty() || seen.contains(&key) {
            continue;
        }
        if let Some(original) = allowed_by_source.get(&key) {
            seen.insert(key);
            out.push(original.clone());
        }
    }
    out
}

fn merge_terms_unique(a: &[TranslationTerm], b: &[TranslationTerm]) -> Vec<TranslationTerm> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::<String>::new();
    for term in a.iter().chain(b.iter()) {
        let key = term.source.trim().to_lowercase();
        if key.is_empty() || seen.contains(&key) {
            continue;
        }
        seen.insert(key);
        out.push(term.clone());
    }
    out
}

fn to_prompt_terms(terms: &[TranslationTerm]) -> Vec<TranslationPromptTerm> {
    terms
        .iter()
        .map(|term| TranslationPromptTerm {
            source: term.source.clone(),
            target: term.target.clone(),
            note: term.note.clone(),
        })
        .collect()
}

fn build_shared_prompt(
    source_language: &str,
    target_language: &str,
    theme: &str,
    primary_terms: &[TranslationTerm],
    batch_index: usize,
    total_batches: usize,
    previous_lines: &[String],
    next_lines: &[String],
    focus_terms: &[TranslationTerm],
    jit_supporting_terms: &[TranslationTerm],
    current_line_count: usize,
) -> Result<String, String> {
    let stable = serde_json::json!({
        "source_language": source_language,
        "target_language": target_language,
        "theme": theme,
        "primary_terms": to_prompt_terms(primary_terms),
        "constraints": {
            "line_by_line": true,
            "same_line_count": true,
            "no_cross_line_meaning_transfer": true
        }
    });
    let dynamic = serde_json::json!({
        "batch_index": batch_index,
        "total_batches": total_batches,
        "previous_lines": previous_lines,
        "next_lines": next_lines,
        "focus_terms": to_prompt_terms(focus_terms),
        "jit_supporting_terms": to_prompt_terms(jit_supporting_terms),
        "current_line_count": current_line_count
    });
    let stable_text = serde_json::to_string_pretty(&stable).map_err(|e| e.to_string())?;
    let dynamic_text = serde_json::to_string_pretty(&dynamic).map_err(|e| e.to_string())?;
    Ok(format!(
        "### Stable Video Context\n```json\n{}\n```\n\n### Dynamic Batch Context\n```json\n{}\n```",
        stable_text, dynamic_text
    ))
}

fn extract_translation_text(entry: &Value) -> Option<String> {
    if let Some(text) = entry.as_str() {
        let text = text.trim();
        if !text.is_empty() {
            return Some(text.to_string());
        }
    }
    let obj = entry.as_object()?;
    const PRIMARY_KEYS: &[&str] = &[
        "translation",
        "translated_text",
        "translatedText",
        "译文",
        "翻译",
    ];
    for key in PRIMARY_KEYS {
        if let Some(text) = obj.get(*key).and_then(|v| v.as_str()) {
            let text = text.trim();
            if !text.is_empty() {
                return Some(text.to_string());
            }
        }
    }

    for (key, value) in obj {
        let k = key.trim();
        if k.eq_ignore_ascii_case("origin")
            || k.eq_ignore_ascii_case("source")
            || k.eq_ignore_ascii_case("source_text")
        {
            continue;
        }
        if k.starts_with("翻译") || k.starts_with("译文") {
            if let Some(text) = value.as_str() {
                let text = text.trim();
                if !text.is_empty() {
                    return Some(text.to_string());
                }
            }
        }
    }

    for (key, value) in obj {
        let k = key.trim();
        if k.eq_ignore_ascii_case("origin")
            || k.eq_ignore_ascii_case("source")
            || k.eq_ignore_ascii_case("source_text")
            || k.eq_ignore_ascii_case("sentence_id")
            || k.eq_ignore_ascii_case("id")
        {
            continue;
        }
        if let Some(text) = value.as_str() {
            let text = text.trim();
            if !text.is_empty() {
                return Some(text.to_string());
            }
        }
    }
    None
}
