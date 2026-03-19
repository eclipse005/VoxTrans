use std::collections::HashMap;
use tokio::task::JoinSet;

use rig::pipeline::{self, TryOp};
use rig::providers::openai;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use voxtrans_core::subtitle::srt::{SrtCue, to_srt_from_cues};
use voxtrans_core::subtitle::text_rules::has_break_terminal_punctuation;
use crate::services::task_log::TaskLogger;

use super::prompt::{
    TranslatePromptInput, TranslatePromptSegmentInput, TranslateStylePromptInput,
    TranslateTerminologyPromptEntry, build_translate_style_system_prompt,
    build_translate_style_user_prompt, build_translate_system_prompt, build_translate_user_prompt,
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
    tone_strategy: String,
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
    validate_request(&request)?;
    let llm_logger = TaskLogger::llm_with_media(request.task_id.clone(), request.media_path.clone());

    let segments = build_source_segments(&request.tokens);
    if segments.is_empty() {
        return Err("tokens did not produce any non-empty subtitle segment".to_string());
    }

    let completions_client = build_openai_completions_client(&request)?;
    let terminology_entries = normalize_terminology_entries(&request.terminology_entries);

    on_phase("summarize");
    llm_logger.event(
        "translate.llm.summarize.start",
        Some(&serde_json::json!({
            "model": &request.translate_model,
            "segmentTotal": segments.len(),
        })),
    );
    let style_profile =
        build_global_style_profile(&request, &segments, &terminology_entries, &completions_client).await;
    llm_logger.event(
        "translate.llm.summarize.done",
        Some(&serde_json::json!({
            "topicSummary": &style_profile.topic_summary,
            "toneStrategy": &style_profile.tone_strategy,
        })),
    );

    on_phase("translate");
    let batches = split_batches(&segments, BATCH_SEGMENT_SIZE);
    llm_logger.event(
        "translate.llm.batch.start",
        Some(&serde_json::json!({
            "batchSize": BATCH_SEGMENT_SIZE,
            "batchTotal": batches.len(),
            "concurrency": request.llm_concurrency,
        })),
    );
    let extracted_batches = run_batch_translate_pipeline(
        &request,
        &segments,
        &batches,
        &terminology_entries,
        &style_profile,
        &completions_client,
        &mut on_batch_progress,
    )
    .await?;
    llm_logger.event(
        "translate.llm.batch.done",
        Some(&serde_json::json!({
            "batchTotal": extracted_batches.len(),
        })),
    );

    let mut translated_by_index: HashMap<usize, String> = HashMap::new();
    for (batch_id, extracted) in extracted_batches.into_iter().enumerate() {
        let batch = batches
            .get(batch_id)
            .ok_or_else(|| format!("internal error: unknown batch id {batch_id}"))?;
        let expected = batch
            .segments
            .iter()
            .map(|segment| segment.index)
            .collect::<Vec<_>>();
        let extracted_json = serde_json::to_value(extracted).map_err(|err| err.to_string())?;
        let translated = validate_llm_segments(&extracted_json, &expected)
            .map_err(|err| format!("translate batch {} invalid: {err}", batch_id + 1))?;
        translated_by_index.extend(translated);
    }

    let mut translated_segments: Vec<TranslateSegment> = Vec::new();
    for segment in &segments {
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
        style_topic_summary: style_profile.topic_summary,
        style_tone_strategy: style_profile.tone_strategy,
    })
}

fn build_openai_completions_client(
    request: &TranslatePipelineRequest,
) -> Result<openai::CompletionsClient, String> {
    let mut builder = openai::Client::builder().api_key(request.translate_api_key.trim());
    if !request.translate_base_url.trim().is_empty() {
        builder = builder.base_url(request.translate_base_url.trim());
    }
    let client = builder
        .build()
        .map_err(|err| format!("failed to create rig openai client: {err}"))?;
    Ok(client.completions_api())
}

async fn build_global_style_profile(
    request: &TranslatePipelineRequest,
    segments: &[SourceSegment],
    terminology_entries: &[TranslateTerminologyPromptEntry],
    completions_client: &openai::CompletionsClient,
) -> StyleProfile {
    let fallback = StyleProfile {
        topic_summary: "General subtitle translation.".to_string(),
        tone_strategy: "Natural, concise, and consistent subtitle tone.".to_string(),
    };
    let (head, middle, tail) = sample_global_contexts(segments, STYLE_CONTEXT_WORDS);
    if head.is_empty() && middle.is_empty() && tail.is_empty() {
        return fallback;
    }

    let style_prompt = build_translate_style_user_prompt(&TranslateStylePromptInput {
        source_lang: request.source_lang.clone(),
        target_lang: request.target_lang.clone(),
        context_head: head,
        context_middle: middle,
        context_tail: tail,
        terminology_entries: terminology_entries.to_vec(),
    });

    let style_extractor = completions_client
        .extractor::<StyleExtraction>(request.translate_model.clone())
        .preamble(&build_translate_style_system_prompt())
        .retries(2)
        .build();

    let style_pipeline = pipeline::new().extract(style_extractor);
    match style_pipeline.try_call(style_prompt).await {
        Ok(style) => {
            let topic_summary = style.topic_summary.trim().to_string();
            let tone_strategy = style.tone_strategy.trim().to_string();
            StyleProfile {
                topic_summary: if topic_summary.is_empty() {
                    fallback.topic_summary
                } else {
                    topic_summary
                },
                tone_strategy: if tone_strategy.is_empty() {
                    fallback.tone_strategy
                } else {
                    tone_strategy
                },
            }
        }
        Err(_) => fallback,
    }
}

async fn run_batch_translate_pipeline<G>(
    request: &TranslatePipelineRequest,
    segments: &[SourceSegment],
    batches: &[SegmentBatch],
    terminology_entries: &[TranslateTerminologyPromptEntry],
    style_profile: &StyleProfile,
    _completions_client: &openai::CompletionsClient,
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
                    .map(|segment| TranslatePromptSegmentInput {
                        index: segment.index,
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
    let model = request.translate_model.clone();
    let api_key = request.translate_api_key.clone();
    let base_url = request.translate_base_url.clone();

    let mut join_set: JoinSet<Result<(usize, TranslationBatchExtraction), String>> = JoinSet::new();
    let mut next_to_spawn = 0usize;
    let mut done = 0usize;
    let mut out: Vec<Option<TranslationBatchExtraction>> = vec![None; total_batches];

    while next_to_spawn < total_batches && join_set.len() < concurrency {
        spawn_translate_task(
            &mut join_set,
            next_to_spawn,
            prompts[next_to_spawn].clone(),
            model.clone(),
            api_key.clone(),
            base_url.clone(),
        );
        next_to_spawn += 1;
    }

    while let Some(joined) = join_set.join_next().await {
        let result = joined.map_err(|err| format!("translate task join failed: {err}"))??;
        let (index, extracted) = result;
        if index >= total_batches {
            return Err(format!("translate task returned invalid index {index}"));
        }
        out[index] = Some(extracted);
        done += 1;
        on_batch_progress(done.min(total_batches), total_batches);

        if next_to_spawn < total_batches {
            spawn_translate_task(
                &mut join_set,
                next_to_spawn,
                prompts[next_to_spawn].clone(),
                model.clone(),
                api_key.clone(),
                base_url.clone(),
            );
            next_to_spawn += 1;
        }
    }

    out.into_iter()
        .enumerate()
        .map(|(index, item)| item.ok_or_else(|| format!("missing translated batch at index {index}")))
        .collect()
}

fn spawn_translate_task(
    join_set: &mut JoinSet<Result<(usize, TranslationBatchExtraction), String>>,
    index: usize,
    prompt: String,
    model: String,
    api_key: String,
    base_url: String,
) {
    join_set.spawn(async move {
        let mut builder = openai::Client::builder().api_key(api_key.trim());
        if !base_url.trim().is_empty() {
            builder = builder.base_url(base_url.trim());
        }
        let client = builder
            .build()
            .map_err(|err| format!("failed to create rig openai client: {err}"))?
            .completions_api();

        let batch_extractor = client
            .extractor::<TranslationBatchExtraction>(model)
            .preamble(&build_translate_system_prompt())
            .retries(2)
            .build();
        let batch_pipeline = pipeline::new().extract(batch_extractor);
        let extracted = batch_pipeline
            .try_call(prompt)
            .await
            .map_err(|err| format!("translate pipeline failed: {err}"))?;
        Ok((index, extracted))
    });
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
                group: entry.group.trim().to_string(),
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
