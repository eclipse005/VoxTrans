use std::collections::HashMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use voxtrans_core::subtitle::srt::{SrtCue, to_srt_from_cues};
use voxtrans_core::subtitle::text_rules::has_break_terminal_punctuation;
use crate::services::llm::client::OpenAiCompatLlmClient;
use crate::services::llm::client::LlmSemanticValidationError;
use crate::services::llm::json_guard::JsonResponseValidator;
use crate::services::llm::batch::run_indexed_concurrent;
use crate::services::llm::port::{LlmCallContext, LlmConfig, LlmJsonTask, next_llm_request_id};

use super::prompt::{
    TranslatePromptInput, TranslatePromptSegmentInput, TranslateSummaryPromptInput,
    TranslateTerminologyPromptEntry, build_translate_summary_system_prompt,
    build_translate_summary_user_prompt, build_translate_system_prompt, build_translate_user_prompt,
};
use super::types::{
    TranslatePipelineRequest, TranslatePipelineResponse, TranslateSegment, TranslateTerminologyEntry,
    TranslateToken,
};
use super::validation::{validate_llm_segments, validate_request};

const BATCH_SEGMENT_SIZE: usize = 20;
const STYLE_CONTEXT_WORDS: usize = 1000;
const DEFAULT_THEME: &str = "内容围绕一个明确主题展开。关键信息以解释和示例为主。";

#[derive(Debug, Clone)]
struct SourceSegment {
    index: usize,
    start_ms: u64,
    end_ms: u64,
    source_text: String,
}

#[derive(Debug, Clone)]
struct SummaryProfile {
    theme: String,
    primary_terminology_entries: Vec<TranslateTerminologyPromptEntry>,
    supporting_terminology_entries: Vec<TranslateTerminologyPromptEntry>,
    terminology_entries: Vec<TranslateTerminologyPromptEntry>,
}

#[derive(Debug, Clone)]
struct SegmentBatch {
    start_idx: usize,
    end_idx: usize,
    segments: Vec<SourceSegment>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct SummaryExtraction {
    #[serde(default, deserialize_with = "deserialize_string_or_empty")]
    theme: String,
    #[serde(default)]
    primary_terminology_entries: Vec<TranslateTerminologyPromptEntry>,
    #[serde(default)]
    supporting_terminology_entries: Vec<TranslateTerminologyPromptEntry>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct TranslationBatchExtraction {
    segments: Vec<TranslationBatchItem>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct TranslationBatchItem {
    index: usize,
    #[serde(alias = "translatedText")]
    #[serde(alias = "translated_text")]
    #[serde(rename = "translation")]
    translated_text: String,
}

pub async fn run_translate_pipeline<F, G>(
    request: TranslatePipelineRequest,
    mut on_phase: F,
    mut on_batch_progress: G,
) -> Result<TranslatePipelineResponse, String>
where
    F: FnMut(&str),
    G: FnMut(usize, usize),
{
    on_phase("summarize");
    let (theme, terminology_entries, _, _) = summarize_translate_theme(&request).await?;
    on_phase("translate");
    run_translate_with_theme(request, theme, terminology_entries, &mut on_batch_progress).await
}

pub async fn summarize_translate_theme(
    request: &TranslatePipelineRequest,
) -> Result<(String, Vec<TranslateTerminologyEntry>, usize, usize), String> {
    validate_request(request)?;
    let segments = build_source_segments(&request.tokens);
    if segments.is_empty() {
        return Err("tokens did not produce any non-empty subtitle segment".to_string());
    }
    let llm_client = build_llm_client(request)?;
    let terminology_entries = normalize_terminology_entries(&request.terminology_entries);
    let summary_profile =
        build_global_summary_profile(request, &segments, &terminology_entries, &llm_client).await?;
    Ok((
        summary_profile.theme,
        to_translate_terminology_entries(&summary_profile.terminology_entries),
        summary_profile.primary_terminology_entries.len(),
        summary_profile.supporting_terminology_entries.len(),
    ))
}

pub async fn run_translate_with_theme<G>(
    request: TranslatePipelineRequest,
    theme: String,
    summary_terminology_entries: Vec<TranslateTerminologyEntry>,
    on_batch_progress: &mut G,
) -> Result<TranslatePipelineResponse, String>
where
    G: FnMut(usize, usize),
{
    validate_request(&request)?;
    let segments = build_source_segments(&request.tokens);
    if segments.is_empty() {
        return Err("tokens did not produce any non-empty subtitle segment".to_string());
    }
    let llm_client = build_llm_client(&request)?;
    let terminology_entries = normalize_terminology_entries(&summary_terminology_entries);
    let summary_profile = SummaryProfile {
        theme,
        primary_terminology_entries: Vec::new(),
        supporting_terminology_entries: terminology_entries.clone(),
        terminology_entries,
    };
    translate_from_theme(
        &request,
        &segments,
        &summary_profile,
        &llm_client,
        on_batch_progress,
    )
    .await
}

async fn translate_from_theme<G>(
    request: &TranslatePipelineRequest,
    segments: &[SourceSegment],
    summary_profile: &SummaryProfile,
    llm_client: &OpenAiCompatLlmClient,
    on_batch_progress: &mut G,
) -> Result<TranslatePipelineResponse, String>
where
    G: FnMut(usize, usize),
{
    let batches = split_batches(segments, BATCH_SEGMENT_SIZE);
    let extracted_batches = run_batch_translate_pipeline(
        request,
        segments,
        &batches,
        &summary_profile.terminology_entries,
        summary_profile,
        llm_client,
        on_batch_progress,
    )
    .await?;

    let mut translated_by_index: HashMap<usize, String> = HashMap::new();
    for (batch_id, extracted) in extracted_batches.into_iter().enumerate() {
        let batch = batches
            .get(batch_id)
            .ok_or_else(|| format!("internal error: unknown batch id {batch_id}"))?;
        let expected = (1..=batch.segments.len()).collect::<Vec<_>>();
        let extracted_json = serde_json::to_value(extracted).map_err(|err| err.to_string())?;
        let mut translated_local = validate_llm_segments(&extracted_json, &expected)
            .map_err(|err| format!("translate batch {} invalid: {err}", batch_id + 1))?;
        for (offset, segment) in batch.segments.iter().enumerate() {
            let local_index = offset + 1;
            let translated_text = translated_local.remove(&local_index).ok_or_else(|| {
                format!(
                    "translate batch {} invalid: missing local index {}",
                    batch_id + 1,
                    local_index
                )
            })?;
            translated_by_index.insert(segment.index, translated_text);
        }
    }

    let mut translated_segments: Vec<TranslateSegment> = Vec::new();
    for segment in segments {
        let translated_text = translated_by_index
            .remove(&segment.index)
            .ok_or_else(|| format!("missing translation for segment {}", segment.index))?;
        translated_segments.push(TranslateSegment {
            start_ms: segment.start_ms,
            end_ms: segment.end_ms,
            source_text: segment.source_text.clone(),
            translated_text,
        });
    }
    let source_srt = build_srt(&translated_segments, false);
    let target_srt = build_srt(&translated_segments, true);
    let bilingual_srt_source_first = build_bilingual_srt(&translated_segments, true);
    let bilingual_srt_target_first = build_bilingual_srt(&translated_segments, false);

    Ok(TranslatePipelineResponse {
        source_srt,
        target_srt,
        bilingual_srt_source_first,
        bilingual_srt_target_first,
        segments: translated_segments,
        theme_summary: summary_profile.theme.clone(),
    })
}

fn build_llm_client(request: &TranslatePipelineRequest) -> Result<OpenAiCompatLlmClient, String> {
    OpenAiCompatLlmClient::new(LlmConfig::new(
        request.translate_base_url.clone(),
        request.translate_api_key.clone(),
        request.translate_model.clone(),
    ))
    .map_err(|err| err.message)
}

async fn build_global_summary_profile(
    request: &TranslatePipelineRequest,
    segments: &[SourceSegment],
    terminology_entries: &[TranslateTerminologyPromptEntry],
    llm_client: &OpenAiCompatLlmClient,
) -> Result<SummaryProfile, String> {
    let (head, middle, tail) = sample_global_contexts(segments, STYLE_CONTEXT_WORDS);
    if head.is_empty() && middle.is_empty() && tail.is_empty() {
        return Err("summarize failed: empty context".to_string());
    }

    let summary_prompt = build_translate_summary_user_prompt(&TranslateSummaryPromptInput {
        source_lang: request.source_lang.clone(),
        target_lang: request.target_lang.clone(),
        context_head: head,
        context_middle: middle,
        context_tail: tail,
        terminology_entries: terminology_entries.to_vec(),
    });
    let summary_system_prompt = build_translate_summary_system_prompt();
    let validator = JsonResponseValidator::with_required_keys(&[
        "theme",
        "primaryTerminologyEntries",
        "supportingTerminologyEntries",
    ]);
    let context = LlmCallContext {
        task_id: request.task_id.clone(),
        media_path: Some(request.media_path.clone()),
        phase: "summarize".to_string(),
    };
    let llm_id = next_llm_request_id();
    let result = llm_client
        .call_json_validated(
            &context,
            &llm_id,
            &summary_system_prompt,
            &summary_prompt,
            Some(&validator),
            |value| {
                serde_json::from_value::<SummaryExtraction>(value)
                    .map_err(|err| LlmSemanticValidationError::retryable(format!("summarize parse failed: {err}")))
            },
        )
        .await;
    let summary_result = result
        .map_err(|err| format!("summarize failed (llmId={}): {}", llm_id, err.message))?;
    let summary = summary_result.value;
    let theme = normalize_theme(summary.theme.as_str());
    let primary_terms = filter_selected_terminology_entries(
        &summary.primary_terminology_entries,
        terminology_entries,
    );
    let supporting_terms = filter_selected_terminology_entries(
        &summary.supporting_terminology_entries,
        terminology_entries,
    );
    let selected_terms = merge_selected_terms(&primary_terms, &supporting_terms, terminology_entries);
    Ok(SummaryProfile {
        theme,
        primary_terminology_entries: primary_terms,
        supporting_terminology_entries: supporting_terms,
        terminology_entries: selected_terms,
    })
}

fn normalize_theme(theme: &str) -> String {
    let normalized = theme
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string();
    if normalized.is_empty() {
        DEFAULT_THEME.to_string()
    } else {
        normalized
    }
}

fn deserialize_string_or_empty<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<String>::deserialize(deserializer)?;
    Ok(value.unwrap_or_default())
}

async fn run_batch_translate_pipeline<G>(
    request: &TranslatePipelineRequest,
    segments: &[SourceSegment],
    batches: &[SegmentBatch],
    terminology_entries: &[TranslateTerminologyPromptEntry],
    summary_profile: &SummaryProfile,
    llm_client: &OpenAiCompatLlmClient,
    on_batch_progress: &mut G,
) -> Result<Vec<TranslationBatchExtraction>, String>
where
    G: FnMut(usize, usize),
{
    let prompts = batches
        .iter()
        .map(|batch| {
            let prev = context_before(segments, batch.start_idx, 2);
            let next = context_after(segments, batch.end_idx, 2);
            let prompt = TranslatePromptInput {
                source_lang: request.source_lang.clone(),
                target_lang: request.target_lang.clone(),
                previous_context: prev,
                next_context: next,
                theme: summary_profile.theme.clone(),
                terminology_entries: terminology_entries.to_vec(),
                segments: batch
                    .segments
                    .iter()
                    .enumerate()
                    .map(|(idx, segment)| TranslatePromptSegmentInput {
                        index: idx + 1,
                        source_text: segment.source_text.clone(),
                    })
                    .collect(),
            };
            build_translate_user_prompt(&prompt)
        })
        .collect::<Vec<_>>();

    let total_batches = prompts.len();
    if total_batches == 0 {
        return Ok(Vec::new());
    }

    let concurrency = request.llm_concurrency.clamp(1, 16) as usize;
    let validator = JsonResponseValidator::with_required_keys(&["segments"]);
    let tasks = prompts
        .into_iter()
        .enumerate()
        .map(|(index, user_prompt)| LlmJsonTask {
            id: index,
            request_id: next_llm_request_id(),
            system_prompt: build_translate_system_prompt(),
            user_prompt,
            response_validator: Some(validator.clone()),
        })
        .collect::<Vec<_>>();
    let context = LlmCallContext {
        task_id: request.task_id.clone(),
        media_path: Some(request.media_path.clone()),
        phase: "translate".to_string(),
    };
    let batches_for_validation = batches.to_vec();
    let results = run_indexed_concurrent(
        tasks,
        concurrency,
        {
            let llm_client = llm_client.clone();
            let context = context.clone();
            let batches_for_validation = batches_for_validation.clone();
            move |task| {
                let llm_client = llm_client.clone();
                let context = context.clone();
                let batches_for_validation = batches_for_validation.clone();
                async move {
                    let llm_id = task.request_id.clone();
                    let Some(batch) = batches_for_validation.get(task.id) else {
                        return Err(format!(
                            "translate pipeline failed at {} (llmId={}): missing batch",
                            task.id + 1,
                            llm_id
                        ));
                    };
                    let expected = (1..=batch.segments.len()).collect::<Vec<_>>();
                    let result = llm_client
                        .call_json_validated(
                            &context,
                            &llm_id,
                            &task.system_prompt,
                            &task.user_prompt,
                            task.response_validator.as_ref(),
                            |value| {
                                let extraction = serde_json::from_value::<TranslationBatchExtraction>(value)
                                    .map_err(|err| LlmSemanticValidationError::retryable(format!("translate parse failed: {err}")))?;
                                validate_translation_batch_extraction(&extraction, &expected)
                                    .map_err(LlmSemanticValidationError::retryable)?;
                                Ok(extraction)
                            },
                        )
                        .await;
                    match result {
                        Ok(validated) => Ok((task.id, validated.value)),
                        Err(err) => Err(format!(
                            "translate pipeline failed at {} (llmId={}): {}",
                            task.id + 1,
                            llm_id,
                            err.message
                        )),
                    }
                }
            }
        },
        |message| message,
    )
    .await;
    let mut done = 0usize;
    let mut out: Vec<Option<TranslationBatchExtraction>> = vec![None; total_batches];

    for (index, result) in results {
        if index >= total_batches {
            return Err(format!("translate task returned invalid index {index}"));
        }
        match result {
            Ok((_, extracted)) => out[index] = Some(extracted),
            Err(err) => return Err(err),
        }
        done += 1;
        on_batch_progress(done.min(total_batches), total_batches);
    }

    out.into_iter()
        .enumerate()
        .map(|(index, item)| item.ok_or_else(|| format!("missing translated batch at index {index}")))
        .collect()
}

fn validate_translation_batch_extraction(
    extraction: &TranslationBatchExtraction,
    expected_indexes: &[usize],
) -> Result<(), String> {
    let value = serde_json::to_value(extraction).map_err(|err| err.to_string())?;
    validate_llm_segments(&value, expected_indexes).map(|_| ())
}

fn normalize_terminology_entries(
    entries: &[TranslateTerminologyEntry],
) -> Vec<TranslateTerminologyPromptEntry> {
    entries
        .iter()
        .filter_map(|entry| {
            let source = entry.source.trim().to_string();
            let target = entry.target.trim().to_string();
            if source.is_empty() || target.is_empty() {
                return None;
            }
            Some(TranslateTerminologyPromptEntry {
                source,
                target,
                note: entry.note.trim().to_string(),
            })
        })
        .collect()
}

fn filter_selected_terminology_entries(
    selected: &[TranslateTerminologyPromptEntry],
    full: &[TranslateTerminologyPromptEntry],
) -> Vec<TranslateTerminologyPromptEntry> {
    if full.is_empty() || selected.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for item in selected {
        let source = item.source.trim();
        let target = item.target.trim();
        if source.is_empty() || target.is_empty() {
            continue;
        }
        let matched = full.iter().find(|entry| {
            entry.source.trim().eq_ignore_ascii_case(source)
                && entry.target.trim().eq_ignore_ascii_case(target)
        });
        if let Some(entry) = matched {
            if !out.iter().any(|v: &TranslateTerminologyPromptEntry| {
                v.source.trim().eq_ignore_ascii_case(entry.source.trim())
                    && v.target.trim().eq_ignore_ascii_case(entry.target.trim())
            }) {
                out.push(entry.clone());
            }
        }
    }
    out
}

fn merge_selected_terms(
    priority: &[TranslateTerminologyPromptEntry],
    related: &[TranslateTerminologyPromptEntry],
    full: &[TranslateTerminologyPromptEntry],
) -> Vec<TranslateTerminologyPromptEntry> {
    let mut out: Vec<TranslateTerminologyPromptEntry> = Vec::new();
    for entry in priority.iter().chain(related.iter()) {
        if !out.iter().any(|v| {
            v.source.trim().eq_ignore_ascii_case(entry.source.trim())
                && v.target.trim().eq_ignore_ascii_case(entry.target.trim())
        }) {
            out.push(entry.clone());
        }
    }
    // Prefer recall over precision: if model returned none, fallback to full terminology set.
    if out.is_empty() {
        return full.to_vec();
    }
    out
}

fn to_translate_terminology_entries(
    entries: &[TranslateTerminologyPromptEntry],
) -> Vec<TranslateTerminologyEntry> {
    entries
        .iter()
        .map(|entry| TranslateTerminologyEntry {
            source: entry.source.clone(),
            target: entry.target.clone(),
            note: entry.note.clone(),
        })
        .collect()
}

fn sample_global_contexts(
    segments: &[SourceSegment],
    words_per_window: usize,
) -> (String, String, String) {
    let text = segments
        .iter()
        .map(|segment| segment.source_text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let words = text
        .split_whitespace()
        .filter(|w| !w.trim().is_empty())
        .collect::<Vec<_>>();
    if words.is_empty() {
        return (String::new(), String::new(), String::new());
    }

    let head_end = words_per_window.min(words.len());
    let head = words[..head_end].join(" ");

    let middle_start = words
        .len()
        .saturating_div(2)
        .saturating_sub(words_per_window / 2);
    let middle_end = (middle_start + words_per_window).min(words.len());
    let middle = words[middle_start..middle_end].join(" ");

    let tail_start = words.len().saturating_sub(words_per_window);
    let tail = words[tail_start..].join(" ");

    (head, middle, tail)
}

fn split_batches(segments: &[SourceSegment], batch_size: usize) -> Vec<SegmentBatch> {
    let mut out = Vec::new();
    let mut cursor = 0usize;
    while cursor < segments.len() {
        let end_exclusive = (cursor + batch_size).min(segments.len());
        out.push(SegmentBatch {
            start_idx: cursor,
            end_idx: end_exclusive.saturating_sub(1),
            segments: segments[cursor..end_exclusive].to_vec(),
        });
        cursor = end_exclusive;
    }
    out
}

fn context_before(segments: &[SourceSegment], start_idx: usize, count: usize) -> String {
    if start_idx == 0 {
        return String::new();
    }
    let begin = start_idx.saturating_sub(count);
    segments[begin..start_idx]
        .iter()
        .map(|segment| segment.source_text.clone())
        .collect::<Vec<_>>()
        .join(" ")
}

fn context_after(segments: &[SourceSegment], end_idx: usize, count: usize) -> String {
    if end_idx + 1 >= segments.len() {
        return String::new();
    }
    let start = end_idx + 1;
    let end_exclusive = (start + count).min(segments.len());
    segments[start..end_exclusive]
        .iter()
        .map(|segment| segment.source_text.clone())
        .collect::<Vec<_>>()
        .join(" ")
}

fn build_source_segments(tokens: &[TranslateToken]) -> Vec<SourceSegment> {
    let non_empty_tokens = tokens
        .iter()
        .filter(|token| !token.word.trim().is_empty())
        .cloned()
        .collect::<Vec<_>>();
    if non_empty_tokens.is_empty() {
        return Vec::new();
    }

    let phrase_like = non_empty_tokens
        .iter()
        .filter(|token| token.word.chars().any(|c| c.is_whitespace()))
        .count();

    if phrase_like * 10 >= non_empty_tokens.len() * 6 {
        return non_empty_tokens
            .into_iter()
            .enumerate()
            .filter_map(|(index, token)| {
                let source_text = normalize_source_text(&token.word);
                if source_text.is_empty() {
                    return None;
                }
                let start_ms = sec_to_ms(token.start);
                let end_ms = sec_to_ms(token.end.max(token.start));
                Some(SourceSegment {
                    index,
                    start_ms,
                    end_ms: end_ms.max(start_ms),
                    source_text,
                })
            })
            .collect();
    }

    let mut out = Vec::new();
    let mut current_words: Vec<String> = Vec::new();
    let mut current_start = 0.0f64;
    let mut current_end = 0.0f64;
    let mut prev_end = 0.0f64;

    for token in non_empty_tokens {
        let word = token.word.trim().to_string();
        let start = token.start.max(0.0);
        let end = token.end.max(start);

        if current_words.is_empty() {
            current_start = start;
            current_end = end;
        } else {
            let gap = (start - prev_end).max(0.0);
            if gap >= 1.2 {
                push_segment(&mut out, &mut current_words, current_start, current_end);
                current_start = start;
                current_end = end;
            } else {
                current_end = end.max(current_end);
            }
        }

        current_words.push(word.clone());
        prev_end = end;

        let reached_word_limit = current_words.len() >= 14;
        let has_terminal = has_terminal_punctuation(&word);
        if reached_word_limit || has_terminal {
            push_segment(&mut out, &mut current_words, current_start, current_end);
        }
    }
    push_segment(&mut out, &mut current_words, current_start, current_end);

    for (index, segment) in out.iter_mut().enumerate() {
        segment.index = index;
    }
    out
}

fn push_segment(
    out: &mut Vec<SourceSegment>,
    current_words: &mut Vec<String>,
    current_start: f64,
    current_end: f64,
) {
    if current_words.is_empty() {
        return;
    }
    let source_text = normalize_source_text(&current_words.join(" "));
    current_words.clear();
    if source_text.is_empty() {
        return;
    }
    let start_ms = sec_to_ms(current_start);
    let end_ms = sec_to_ms(current_end.max(current_start));
    out.push(SourceSegment {
        index: 0,
        start_ms,
        end_ms: end_ms.max(start_ms),
        source_text,
    });
}

fn has_terminal_punctuation(word: &str) -> bool {
    has_break_terminal_punctuation(word)
}

fn normalize_source_text(raw: &str) -> String {
    let mut text = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    const FIXES: [(&str, &str); 12] = [
        (" ,", ","),
        (" .", "."),
        (" !", "!"),
        (" ?", "?"),
        (" ;", ";"),
        (" :", ":"),
        (" )", ")"),
        ("( ", "("),
        (" n't", "n't"),
        (" 's", "'s"),
        (" 're", "'re"),
        (" 've", "'ve"),
    ];
    for (from, to) in FIXES {
        text = text.replace(from, to);
    }
    text.trim().to_string()
}

fn sec_to_ms(seconds: f64) -> u64 {
    (seconds.max(0.0) * 1000.0).round() as u64
}

pub(crate) fn beautify_translated_text(raw: &str) -> String {
    let trimmed = trim_edge_punctuation(raw.trim());
    if trimmed.is_empty() {
        return String::new();
    }
    let replaced = replace_commas_with_space(trimmed);
    optimize_cn_ascii_spacing(&replaced)
}

fn trim_edge_punctuation(raw: &str) -> &str {
    let chars = raw.char_indices().collect::<Vec<_>>();
    if chars.is_empty() {
        return raw;
    }

    let mut start = 0usize;
    let mut end_exclusive = raw.len();

    while start < end_exclusive {
        let slice = &raw[start..end_exclusive];
        let mut iter = slice.char_indices();
        let Some((_, ch)) = iter.next() else {
            break;
        };
        let next = iter.next().map(|(_, c)| c);
        if !is_removable_edge_punctuation(ch, next, true) {
            break;
        }
        start += ch.len_utf8();
    }

    while start < end_exclusive {
        let slice = &raw[start..end_exclusive];
        let mut prev: Option<char> = None;
        let mut last: Option<char> = None;
        for c in slice.chars() {
            prev = last;
            last = Some(c);
        }
        let Some(ch) = last else {
            break;
        };
        if !is_removable_edge_punctuation(ch, prev, false) {
            break;
        }
        end_exclusive -= ch.len_utf8();
    }

    &raw[start..end_exclusive]
}

fn is_removable_edge_punctuation(ch: char, _neighbor: Option<char>, _is_leading: bool) -> bool {
    matches!(ch, ',' | '，' | '.' | '。')
}

fn replace_commas_with_space(raw: &str) -> String {
    raw.chars()
        .map(|ch| if matches!(ch, ',' | '，') { ' ' } else { ch })
        .collect::<String>()
}

fn optimize_cn_ascii_spacing(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len() + 8);
    let mut prev_non_space: Option<char> = None;

    for ch in raw.chars() {
        if ch.is_whitespace() {
            if !out.ends_with(' ') {
                out.push(' ');
            }
            continue;
        }

        if let Some(prev) = prev_non_space {
            if need_space_between(prev, ch) && !out.ends_with(' ') {
                out.push(' ');
            }
        }

        out.push(ch);
        prev_non_space = Some(ch);
    }

    out.trim().to_string()
}

fn need_space_between(left: char, right: char) -> bool {
    (is_cjk(left) && is_ascii_word(right)) || (is_ascii_word(left) && is_cjk(right))
}

fn is_ascii_word(ch: char) -> bool {
    ch.is_ascii_alphanumeric()
}

fn is_cjk(ch: char) -> bool {
    matches!(ch as u32, 0x4E00..=0x9FFF | 0x3400..=0x4DBF | 0xF900..=0xFAFF)
}

fn build_srt(segments: &[TranslateSegment], translated: bool) -> String {
    let cues = segments
        .iter()
        .enumerate()
        .map(|(idx, segment)| SrtCue {
            index: idx + 1,
            start_ms: segment.start_ms,
            end_ms: segment.end_ms.max(segment.start_ms),
            text: if translated {
                segment.translated_text.trim().to_string()
            } else {
                segment.source_text.trim().to_string()
            },
        })
        .collect::<Vec<_>>();
    to_srt_from_cues(&cues)
}

fn build_bilingual_srt(segments: &[TranslateSegment], source_first: bool) -> String {
    let cues = segments
        .iter()
        .enumerate()
        .map(|(idx, segment)| {
            let first = if source_first {
                segment.source_text.trim()
            } else {
                segment.translated_text.trim()
            };
            let second = if source_first {
                segment.translated_text.trim()
            } else {
                segment.source_text.trim()
            };
            SrtCue {
                index: idx + 1,
                start_ms: segment.start_ms,
                end_ms: segment.end_ms.max(segment.start_ms),
                text: format!("{first}\n{second}"),
            }
        })
        .collect::<Vec<_>>();
    to_srt_from_cues(&cues)
}
