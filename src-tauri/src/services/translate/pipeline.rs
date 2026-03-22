use std::collections::HashMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use voxtrans_core::subtitle::srt::{SrtCue, to_srt_from_cues};
use voxtrans_core::subtitle::text_rules::has_break_terminal_punctuation;
use crate::services::translate::adapters::rig_node::{
    JsonResponseValidator, RigNodeClient, RigNodeConfig, RigNodeJsonTask,
};

use super::prompt::{
    TranslatePromptInput, TranslatePromptSegmentInput, TranslateStylePromptInput,
    TranslateTerminologyPromptEntry, build_translate_style_system_prompt,
    build_translate_style_user_prompt, build_translate_system_prompt, build_translate_user_prompt,
    resolve_translate_style,
};
use super::types::{
    TranslatePipelineRequest, TranslatePipelineResponse, TranslateSegment, TranslateTerminologyEntry,
    TranslateToken,
};
use super::validation::{validate_llm_segments, validate_request};

const BATCH_SEGMENT_SIZE: usize = 20;
const STYLE_CONTEXT_WORDS: usize = 1000;

#[derive(Debug, Clone)]
struct SourceSegment {
    index: usize,
    start_ms: u64,
    end_ms: u64,
    source_text: String,
}

#[derive(Debug, Clone)]
struct StyleProfile {
    topic_summary: String,
    tone_strategy: String,
}

#[derive(Debug, Clone)]
struct SegmentBatch {
    start_idx: usize,
    end_idx: usize,
    segments: Vec<SourceSegment>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct StyleExtraction {
    topic_summary: String,
    style_id: String,
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
    let (topic_summary, tone_strategy) = summarize_translate_style(&request).await?;
    on_phase("translate");
    run_translate_with_style(
        request,
        topic_summary,
        tone_strategy,
        &mut on_batch_progress,
    )
    .await
}

pub async fn summarize_translate_style(
    request: &TranslatePipelineRequest,
) -> Result<(String, String), String> {
    validate_request(request)?;
    let segments = build_source_segments(&request.tokens);
    if segments.is_empty() {
        return Err("tokens did not produce any non-empty subtitle segment".to_string());
    }
    let rig_client = build_rig_client(request)?;
    let terminology_entries = normalize_terminology_entries(&request.terminology_entries);
    let style_profile =
        build_global_style_profile(request, &segments, &terminology_entries, &rig_client).await?;
    Ok((style_profile.topic_summary, style_profile.tone_strategy))
}

pub async fn run_translate_with_style<G>(
    request: TranslatePipelineRequest,
    topic_summary: String,
    tone_strategy: String,
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
    let rig_client = build_rig_client(&request)?;
    let terminology_entries = normalize_terminology_entries(&request.terminology_entries);
    let style_profile = StyleProfile {
        topic_summary,
        tone_strategy,
    };
    translate_from_style(
        &request,
        &segments,
        &terminology_entries,
        &style_profile,
        &rig_client,
        on_batch_progress,
    )
    .await
}

async fn translate_from_style<G>(
    request: &TranslatePipelineRequest,
    segments: &[SourceSegment],
    terminology_entries: &[TranslateTerminologyPromptEntry],
    style_profile: &StyleProfile,
    rig_client: &RigNodeClient,
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
        terminology_entries,
        style_profile,
        rig_client,
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
        style_topic_summary: style_profile.topic_summary.clone(),
        style_tone_strategy: style_profile.tone_strategy.clone(),
    })
}

fn build_rig_client(request: &TranslatePipelineRequest) -> Result<RigNodeClient, String> {
    RigNodeClient::new(RigNodeConfig::new(
        request.translate_base_url.clone(),
        request.translate_api_key.clone(),
        request.translate_model.clone(),
    ))
}

async fn build_global_style_profile(
    request: &TranslatePipelineRequest,
    segments: &[SourceSegment],
    terminology_entries: &[TranslateTerminologyPromptEntry],
    rig_client: &RigNodeClient,
) -> Result<StyleProfile, String> {
    let (head, middle, tail) = sample_global_contexts(segments, STYLE_CONTEXT_WORDS);
    if head.is_empty() && middle.is_empty() && tail.is_empty() {
        return Err("summarize failed: empty style context".to_string());
    }

    let style_prompt = build_translate_style_user_prompt(&TranslateStylePromptInput {
        source_lang: request.source_lang.clone(),
        target_lang: request.target_lang.clone(),
        context_head: head,
        context_middle: middle,
        context_tail: tail,
        terminology_entries: terminology_entries.to_vec(),
    });
    let style_system_prompt = build_translate_style_system_prompt();
    let validator = JsonResponseValidator::with_required_keys(&["topicSummary", "styleId"]);
    let result = rig_client
        .call(
            &request.task_id,
            Some(&request.media_path),
            "summarize",
            &style_system_prompt,
            &style_prompt,
            Some(&validator),
        )
        .await;
    let parsed = result
        .map_err(|err| format!("summarize failed: {}", err.message))?
        .json;
    let style = serde_json::from_value::<StyleExtraction>(parsed)
        .map_err(|err| format!("summarize parse failed: {err}"))?;
    let topic_summary = style.topic_summary.trim().to_string();
    let resolved_style = resolve_translate_style(&style.style_id);
    let tone_strategy = format!("{} ({})", resolved_style.label, resolved_style.guidance);
    if topic_summary.is_empty() {
        return Err("summarize failed: empty summary fields".to_string());
    }
    Ok(StyleProfile {
        topic_summary,
        tone_strategy,
    })
}

async fn run_batch_translate_pipeline<G>(
    request: &TranslatePipelineRequest,
    segments: &[SourceSegment],
    batches: &[SegmentBatch],
    terminology_entries: &[TranslateTerminologyPromptEntry],
    style_profile: &StyleProfile,
    rig_client: &RigNodeClient,
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
                topic_summary: style_profile.topic_summary.clone(),
                tone_strategy: style_profile.tone_strategy.clone(),
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
        .map(|(index, user_prompt)| RigNodeJsonTask {
            id: index,
            system_prompt: build_translate_system_prompt(),
            user_prompt,
            response_validator: Some(validator.clone()),
        })
        .collect::<Vec<_>>();
    let results = rig_client
        .call_batch(
            &request.task_id,
            Some(&request.media_path),
            "translate",
            tasks,
            concurrency,
        )
        .await;
    let mut done = 0usize;
    let mut out: Vec<Option<TranslationBatchExtraction>> = vec![None; total_batches];

    for (index, result) in results {
        if index >= total_batches {
            return Err(format!("translate task returned invalid index {index}"));
        }
        let json = match result {
            Ok(ok) => ok.json,
            Err(err) => return Err(format!("translate pipeline failed: {}", err.message)),
        };
        let extracted = parse_translation_batch_extraction(json)
            .map_err(|err| format!("translate batch parse failed at {}: {err}", index + 1))?;
        out[index] = Some(extracted);
        done += 1;
        on_batch_progress(done.min(total_batches), total_batches);
    }

    out.into_iter()
        .enumerate()
        .map(|(index, item)| item.ok_or_else(|| format!("missing translated batch at index {index}")))
        .collect()
}

fn parse_translation_batch_extraction(value: Value) -> Result<TranslationBatchExtraction, String> {
    serde_json::from_value(value).map_err(|err| err.to_string())
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

fn is_removable_edge_punctuation(ch: char, neighbor: Option<char>, is_leading: bool) -> bool {
    if !matches!(ch, ',' | '，' | '.' | '。') {
        return false;
    }
    match neighbor {
        Some(next) => {
            if is_leading {
                is_cjk(next)
            } else {
                is_cjk(next)
            }
        }
        None => false,
    }
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
