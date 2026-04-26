use std::collections::HashMap;
use std::sync::Arc;

use serde_json::Value;

use crate::services::llm::batch::run_indexed_concurrent_with_progress;
use crate::services::llm::client::{LlmSemanticValidationError, OpenAiCompatLlmClient};
use crate::services::llm::port::{LlmCallContext, LlmConfig, LlmJsonTask, next_llm_request_id};

const DEFAULT_BATCH_SIZE: usize = 20;
const MAX_BATCH_SIZE: usize = 40;
const CONTEXT_LINE_LIMIT: usize = 6;
const MAX_TERMS_PER_BATCH: usize = 16;

#[derive(Debug, Clone)]
pub struct TranslationToken {
    pub text: String,
    pub start: f64,
    pub end: f64,
}

#[derive(Debug, Clone)]
pub struct TranslationSegmentInput {
    pub segment: String,
    pub start: f64,
    pub end: f64,
    pub tokens: Vec<TranslationToken>,
}

#[derive(Debug, Clone)]
pub struct TranslationTerminologyEntry {
    pub source: String,
    pub target: String,
    pub note: String,
}

#[derive(Debug, Clone)]
pub struct BuildTranslationLayerRequest {
    pub task_id: String,
    pub media_path: String,
    pub source_lang: String,
    pub target_lang: String,
    pub segments: Vec<TranslationSegmentInput>,
    pub theme_summary: String,
    pub terminology_entries: Vec<TranslationTerminologyEntry>,
    pub translate_api_key: String,
    pub translate_base_url: String,
    pub translate_model: String,
    pub llm_concurrency: u32,
    pub batch_size: usize,
}

#[derive(Debug, Clone)]
pub struct TranslationSegmentOutput {
    pub segment_id: usize,
    pub start: f64,
    pub end: f64,
    pub source: String,
    pub translation: String,
    pub tokens: Vec<TranslationToken>,
}

#[derive(Debug, Clone)]
pub struct BuildTranslationLayerResponse {
    pub batch_size: usize,
    pub batch_total: usize,
    pub segment_total: usize,
    pub segments: Vec<TranslationSegmentOutput>,
}

#[derive(Debug, Clone)]
struct NormalizedSegment {
    segment_id: usize,
    start: f64,
    end: f64,
    source: String,
    tokens: Vec<TranslationToken>,
}

#[derive(Debug, Clone)]
struct BatchWindow {
    batch_id: usize,
    local_ids: Vec<usize>,
    local_to_global: Vec<usize>,
    prompt: String,
}

pub async fn build_translation_layer_with_progress(
    request: BuildTranslationLayerRequest,
    on_progress: Option<Arc<dyn Fn(usize, usize) + Send + Sync>>,
) -> Result<BuildTranslationLayerResponse, String> {
    validate_request(&request)?;

    let normalized_segments = merge_dangling_source_segments(normalize_segments(&request.segments));
    if normalized_segments.is_empty() {
        return Err("segments contain no translatable text".to_string());
    }

    let llm_client = OpenAiCompatLlmClient::new(LlmConfig::new(
        request.translate_base_url.clone(),
        request.translate_api_key.clone(),
        request.translate_model.clone(),
    ))
    .map_err(|err| err.message)?;

    let batch_size = request
        .batch_size
        .clamp(1, MAX_BATCH_SIZE)
        .max(DEFAULT_BATCH_SIZE.min(MAX_BATCH_SIZE));
    let batch_size = if request.batch_size == 0 {
        DEFAULT_BATCH_SIZE
    } else {
        batch_size
    };
    let windows = build_batch_windows(
        &normalized_segments,
        batch_size,
        &request.source_lang,
        &request.target_lang,
        &request.theme_summary,
        &request.terminology_entries,
    );
    if windows.is_empty() {
        return Err("failed to build translation batches".to_string());
    }

    let concurrency = request.llm_concurrency.max(1) as usize;
    let tasks = windows
        .iter()
        .map(|window| LlmJsonTask {
            id: window.batch_id,
            request_id: next_llm_request_id(),
            user_prompt: window.prompt.clone(),
            response_validator: None,
        })
        .collect::<Vec<_>>();

    let context = LlmCallContext {
        task_id: request.task_id.clone(),
        media_path: Some(request.media_path.clone()),
        phase: "step4_translate_batch".to_string(),
    };

    let windows_for_worker = windows.clone();
    let progress_callback = on_progress.clone();
    let results = run_indexed_concurrent_with_progress(
        tasks,
        concurrency,
        {
            let llm_client = llm_client.clone();
            let context = context.clone();
            move |task| {
                let llm_client = llm_client.clone();
                let context = context.clone();
                let windows = windows_for_worker.clone();
                async move {
                    let Some(window) = windows.get(task.id) else {
                        return Err(format!("missing batch window for index {}", task.id));
                    };
                    let llm_id = task.request_id.clone();
                    let call = llm_client
                        .call_json_validated(
                            &context,
                            &llm_id,
                            &task.user_prompt,
                            task.response_validator.as_ref(),
                            |value| parse_batch_translation(value, &window.local_ids),
                        )
                        .await
                        .map_err(|err| {
                            format!(
                                "step4 translate batch {} failed (llmId={}): {}",
                                window.batch_id + 1,
                                llm_id,
                                err.message
                            )
                        })?;
                    let mut translated_map = HashMap::<usize, String>::new();
                    for (local_id, translated) in call.value {
                        let idx = local_id.saturating_sub(1);
                        let Some(global_id) = window.local_to_global.get(idx).copied() else {
                            continue;
                        };
                        translated_map.insert(global_id, translated);
                    }
                    Ok((window.batch_id, translated_map))
                }
            }
        },
        |msg| msg,
        move |done, total| {
            if let Some(callback) = progress_callback.as_ref() {
                callback(done, total);
            }
        },
    )
    .await;

    let mut translated_by_id = HashMap::<usize, String>::new();
    for (_, item) in results {
        let (_, translated_map) = item?;
        for (id, translated) in translated_map {
            translated_by_id.insert(id, translated);
        }
    }

    let mut outputs = Vec::<TranslationSegmentOutput>::new();
    for segment in &normalized_segments {
        let translated = translated_by_id
            .remove(&segment.segment_id)
            .unwrap_or_default();
        outputs.push(TranslationSegmentOutput {
            segment_id: segment.segment_id,
            start: segment.start,
            end: segment.end,
            source: segment.source.clone(),
            translation: translated,
            tokens: segment.tokens.clone(),
        });
    }

    let incomplete_ids = outputs
        .iter()
        .filter(|segment| segment.translation.trim().is_empty())
        .map(|segment| segment.segment_id)
        .collect::<Vec<_>>();
    if !incomplete_ids.is_empty() {
        return Err(format!(
            "translation incomplete: missing non-empty translations for segment ids {:?}",
            incomplete_ids
        ));
    }

    Ok(BuildTranslationLayerResponse {
        batch_size,
        batch_total: windows.len(),
        segment_total: outputs.len(),
        segments: outputs,
    })
}

fn validate_request(request: &BuildTranslationLayerRequest) -> Result<(), String> {
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
    Ok(())
}

fn normalize_segments(segments: &[TranslationSegmentInput]) -> Vec<NormalizedSegment> {
    let mut out = Vec::<NormalizedSegment>::new();
    for (index, segment) in segments.iter().enumerate() {
        let source = normalize_inline_text(&segment.segment);
        let source = if source.is_empty() {
            let fallback = segment
                .tokens
                .iter()
                .map(|token| token.text.trim())
                .filter(|token| !token.is_empty())
                .collect::<Vec<_>>()
                .join(" ");
            normalize_inline_text(&fallback)
        } else {
            source
        };
        if source.is_empty() {
            continue;
        }
        out.push(NormalizedSegment {
            segment_id: index + 1,
            start: segment.start,
            end: segment.end.max(segment.start),
            source,
            tokens: segment.tokens.clone(),
        });
    }
    out
}

fn merge_dangling_source_segments(segments: Vec<NormalizedSegment>) -> Vec<NormalizedSegment> {
    if segments.len() < 2 {
        return segments;
    }

    let mut merged = Vec::<NormalizedSegment>::new();
    let mut index = 0usize;
    while index < segments.len() {
        let mut current = segments[index].clone();
        while index + 1 < segments.len()
            && can_merge_dangling_source_pair(&current, &segments[index + 1])
        {
            current = merge_source_pair(&current, &segments[index + 1]);
            index += 1;
        }
        merged.push(current);
        index += 1;
    }

    for (index, segment) in merged.iter_mut().enumerate() {
        segment.segment_id = index + 1;
    }
    merged
}

fn can_merge_dangling_source_pair(left: &NormalizedSegment, right: &NormalizedSegment) -> bool {
    let left_text = left.source.trim();
    let right_text = right.source.trim();
    if left_text.is_empty() || right_text.is_empty() {
        return false;
    }
    if left.end > right.start || right.start - left.end > 1.0 {
        return false;
    }

    let combined_words = count_ascii_words(left_text) + count_ascii_words(right_text);
    if combined_words > 38 {
        return false;
    }
    if right.end - left.start > 12.0 {
        return false;
    }

    let left_last = left_text.chars().last().unwrap_or_default();
    if is_hard_sentence_terminal(left_last) {
        return false;
    }

    if left_last == ',' || left_last == ';' || left_last == ':' {
        return starts_with_lowercase_or_connector(right_text)
            || starts_with_continuation_word(right_text);
    }

    if ends_with_dangling_source_word(left_text) {
        return true;
    }

    starts_with_subordinate_clause(left_text) && starts_with_lowercase_or_connector(right_text)
}

fn merge_source_pair(left: &NormalizedSegment, right: &NormalizedSegment) -> NormalizedSegment {
    let mut tokens = left.tokens.clone();
    tokens.extend(right.tokens.iter().cloned());
    NormalizedSegment {
        segment_id: left.segment_id,
        start: left.start,
        end: right.end.max(left.end),
        source: normalize_inline_text(&format!("{} {}", left.source, right.source)),
        tokens,
    }
}

fn is_hard_sentence_terminal(ch: char) -> bool {
    matches!(ch, '.' | '?' | '!' | '。' | '？' | '！')
}

fn count_ascii_words(text: &str) -> usize {
    text.split_whitespace()
        .filter(|token| token.chars().any(|ch| ch.is_ascii_alphanumeric()))
        .count()
}

fn last_ascii_word_lower(text: &str) -> String {
    text.split_whitespace()
        .rev()
        .find_map(|token| {
            let cleaned = token
                .trim_matches(|ch: char| !ch.is_ascii_alphanumeric() && ch != '\'')
                .to_ascii_lowercase();
            if cleaned.is_empty() {
                None
            } else {
                Some(cleaned)
            }
        })
        .unwrap_or_default()
}

fn first_ascii_word_lower(text: &str) -> String {
    text.split_whitespace()
        .find_map(|token| {
            let cleaned = token
                .trim_matches(|ch: char| !ch.is_ascii_alphanumeric() && ch != '\'')
                .to_ascii_lowercase();
            if cleaned.is_empty() {
                None
            } else {
                Some(cleaned)
            }
        })
        .unwrap_or_default()
}

fn starts_with_lowercase_or_connector(text: &str) -> bool {
    text.chars()
        .next()
        .map(|ch| ch.is_ascii_lowercase())
        .unwrap_or(false)
        || starts_with_continuation_word(text)
}

fn starts_with_continuation_word(text: &str) -> bool {
    let first = first_ascii_word_lower(text);
    matches!(
        first.as_str(),
        "and"
            | "or"
            | "but"
            | "so"
            | "then"
            | "because"
            | "which"
            | "that"
            | "to"
            | "for"
            | "with"
            | "plus"
    )
}

fn ends_with_dangling_source_word(text: &str) -> bool {
    let last = last_ascii_word_lower(text);
    matches!(
        last.as_str(),
        "a" | "an"
            | "the"
            | "this"
            | "that"
            | "these"
            | "those"
            | "my"
            | "your"
            | "his"
            | "her"
            | "their"
            | "our"
            | "of"
            | "to"
            | "for"
            | "with"
            | "and"
            | "or"
            | "but"
            | "because"
            | "if"
            | "when"
            | "which"
            | "who"
            | "you"
            | "i"
            | "we"
            | "they"
    )
}

fn starts_with_subordinate_clause(text: &str) -> bool {
    let first = first_ascii_word_lower(text);
    matches!(
        first.as_str(),
        "if" | "when" | "because" | "although" | "while" | "once" | "unless"
    )
}

fn build_batch_windows(
    segments: &[NormalizedSegment],
    batch_size: usize,
    source_lang: &str,
    target_lang: &str,
    theme_summary: &str,
    terminology_entries: &[TranslationTerminologyEntry],
) -> Vec<BatchWindow> {
    if segments.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::<BatchWindow>::new();
    let mut batch_start = 0usize;
    while batch_start < segments.len() {
        let batch_end = (batch_start + batch_size).min(segments.len());
        let current = &segments[batch_start..batch_end];

        let prev_start = batch_start.saturating_sub(CONTEXT_LINE_LIMIT);
        let prev = &segments[prev_start..batch_start];

        let next_end = (batch_end + CONTEXT_LINE_LIMIT).min(segments.len());
        let next = &segments[batch_end..next_end];

        let terms = select_batch_terms(current, terminology_entries, MAX_TERMS_PER_BATCH);
        let prompt = build_batch_translate_prompt(
            source_lang,
            target_lang,
            theme_summary,
            prev,
            current,
            next,
            &terms,
        );

        out.push(BatchWindow {
            batch_id: out.len(),
            local_ids: (1..=current.len()).collect(),
            local_to_global: current.iter().map(|segment| segment.segment_id).collect(),
            prompt,
        });

        batch_start = batch_end;
    }

    out
}

fn select_batch_terms(
    current_segments: &[NormalizedSegment],
    entries: &[TranslationTerminologyEntry],
    max_terms: usize,
) -> Vec<TranslationTerminologyEntry> {
    if entries.is_empty() {
        return Vec::new();
    }

    let batch_text = current_segments
        .iter()
        .map(|segment| segment.source.as_str())
        .collect::<Vec<_>>()
        .join("\n")
        .to_lowercase();

    let mut matched = entries
        .iter()
        .filter(|entry| {
            let source = entry.source.trim().to_lowercase();
            !source.is_empty() && batch_text.contains(&source)
        })
        .take(max_terms)
        .cloned()
        .collect::<Vec<_>>();

    if matched.len() >= max_terms {
        return matched;
    }

    for entry in entries {
        if matched.len() >= max_terms {
            break;
        }
        if matched
            .iter()
            .any(|existing| existing.source.eq_ignore_ascii_case(&entry.source))
        {
            continue;
        }
        matched.push(entry.clone());
    }

    matched
}

fn build_batch_translate_prompt(
    source_lang: &str,
    target_lang: &str,
    theme_summary: &str,
    prev: &[NormalizedSegment],
    current: &[NormalizedSegment],
    next: &[NormalizedSegment],
    terms: &[TranslationTerminologyEntry],
) -> String {
    let prev_lines = prev
        .iter()
        .map(|segment| segment.source.clone())
        .collect::<Vec<_>>();
    let current_lines = current
        .iter()
        .enumerate()
        .map(|(index, segment)| {
            serde_json::json!({
                "id": index + 1,
                "text": segment.source,
            })
        })
        .collect::<Vec<_>>();
    let next_lines = next
        .iter()
        .map(|segment| segment.source.clone())
        .collect::<Vec<_>>();

    let prompt_terms = terms
        .iter()
        .map(|term| {
            serde_json::json!({
                "source": term.source,
                "target": term.target,
                "note": term.note,
            })
        })
        .collect::<Vec<_>>();

    serde_json::json!({
        "task": "translate_segment_batch_with_context",
        "rule": "Return JSON only.",
        "sourceLanguage": source_lang,
        "targetLanguage": target_lang,
        "theme": theme_summary,
        "context": {
            "previousLines": prev_lines,
            "currentLines": current_lines,
            "nextLines": next_lines,
        },
        "terminology": prompt_terms,
        "constraints": [
            "Translate only currentLines.",
            "Preserve batch-local line id (1..N).",
            "Keep meaning faithful and natural.",
            "Apply provided terminology when relevant.",
            "Prefer one translation line per input line.",
            "No extra explanations."
        ],
        "output": {
            "translations": [
                { "id": 1, "text": "translated text" }
            ]
        }
    })
    .to_string()
}

fn parse_batch_translation(
    value: Value,
    expected_ids: &[usize],
) -> Result<HashMap<usize, String>, LlmSemanticValidationError> {
    let mut out = HashMap::<usize, String>::new();

    if let Some(items) = value.get("translations").and_then(|v| v.as_array()) {
        for item in items {
            let Some(obj) = item.as_object() else {
                return Err(LlmSemanticValidationError::retryable(
                    "translations item must be object",
                ));
            };
            let id = obj
                .get("id")
                .and_then(|v| v.as_u64())
                .map(|v| v as usize)
                .ok_or_else(|| {
                    LlmSemanticValidationError::retryable("translation id is required")
                })?;
            let text = obj
                .get("text")
                .or_else(|| obj.get("translation"))
                .or_else(|| obj.get("translatedText"))
                .and_then(|v| v.as_str())
                .map(normalize_inline_text)
                .unwrap_or_default();
            if !expected_ids.contains(&id) {
                continue;
            }
            if text.is_empty() {
                return Err(LlmSemanticValidationError::retryable(format!(
                    "translation id {id} must be non-empty"
                )));
            }
            if out.insert(id, text).is_some() {
                return Err(LlmSemanticValidationError::retryable(format!(
                    "duplicate translation id {id}"
                )));
            }
        }
    } else if let Some(obj) = value.as_object() {
        for (key, item) in obj {
            let id = key.parse::<usize>().map_err(|_| {
                LlmSemanticValidationError::retryable("translation map key must be numeric id")
            })?;
            let text = item
                .get("text")
                .or_else(|| item.get("translation"))
                .or_else(|| item.get("translatedText"))
                .and_then(|v| v.as_str())
                .map(normalize_inline_text)
                .unwrap_or_default();
            if !expected_ids.contains(&id) {
                continue;
            }
            if text.is_empty() {
                return Err(LlmSemanticValidationError::retryable(format!(
                    "translation id {id} must be non-empty"
                )));
            }
            if out.insert(id, text).is_some() {
                return Err(LlmSemanticValidationError::retryable(format!(
                    "duplicate translation id {id}"
                )));
            }
        }
    } else {
        return Err(LlmSemanticValidationError::retryable(
            "translation response root must be object",
        ));
    }

    for expected_id in expected_ids {
        if !out.contains_key(expected_id) {
            return Err(LlmSemanticValidationError::retryable(format!(
                "missing translation id {expected_id}"
            )));
        }
    }

    Ok(out)
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

#[cfg(test)]
mod tests {
    use super::{
        TranslationSegmentInput, TranslationTerminologyEntry, build_batch_windows,
        merge_dangling_source_segments, normalize_segments, parse_batch_translation,
    };
    use serde_json::json;

    fn seg(text: &str) -> TranslationSegmentInput {
        TranslationSegmentInput {
            segment: text.to_string(),
            start: 0.0,
            end: 1.0,
            tokens: Vec::new(),
        }
    }

    fn timed_seg(index: usize, text: &str) -> TranslationSegmentInput {
        TranslationSegmentInput {
            segment: text.to_string(),
            start: index as f64,
            end: index as f64 + 1.0,
            tokens: Vec::new(),
        }
    }

    #[test]
    fn split_batches_respects_requested_size() {
        let normalized = normalize_segments(&[seg("a"), seg("b"), seg("c"), seg("d"), seg("e")]);
        let windows = build_batch_windows(
            &normalized,
            2,
            "en",
            "zh-CN",
            "theme",
            &Vec::<TranslationTerminologyEntry>::new(),
        );
        assert_eq!(windows.len(), 3);
        assert_eq!(windows[0].local_ids, vec![1, 2]);
        assert_eq!(windows[1].local_ids, vec![1, 2]);
        assert_eq!(windows[2].local_ids, vec![1]);
        assert_eq!(windows[0].local_to_global, vec![1, 2]);
        assert_eq!(windows[1].local_to_global, vec![3, 4]);
        assert_eq!(windows[2].local_to_global, vec![5]);
    }

    #[test]
    fn parse_batch_translation_rejects_missing_expected_id() {
        let value = json!({
            "translations": [
                { "id": 1, "text": "first" }
            ]
        });

        let err = parse_batch_translation(value, &[1, 2]).expect_err("should reject missing id");
        assert!(format!("{err:?}").contains("missing translation id 2"));
    }

    #[test]
    fn parse_batch_translation_rejects_empty_translation_text() {
        let value = json!({
            "translations": [
                { "id": 1, "text": "" }
            ]
        });

        let err =
            parse_batch_translation(value, &[1]).expect_err("should reject empty translation");
        assert!(format!("{err:?}").contains("translation id 1 must be non-empty"));
    }

    #[test]
    fn parse_batch_translation_accepts_complete_non_empty_batch() {
        let value = json!({
            "translations": [
                { "id": 1, "text": "first" },
                { "id": 2, "text": "second" }
            ]
        });

        let out = parse_batch_translation(value, &[1, 2]).expect("should parse full batch");
        assert_eq!(out.get(&1).map(String::as_str), Some("first"));
        assert_eq!(out.get(&2).map(String::as_str), Some("second"));
    }

    #[test]
    fn merge_dangling_source_segments_keeps_translation_units_semantic() {
        let normalized = normalize_segments(&[
            timed_seg(
                0,
                "It's something I've been trying to do every week just to get a good idea of how I'm performing",
            ),
            timed_seg(1, "against the reference list of literally reviewing a"),
            timed_seg(2, "high quality example that I see"),
            timed_seg(3, "because sometimes your execution slips, you"),
            timed_seg(
                4,
                "might skip every high quality example due to hesitation or maybe you choose weaker examples because you're not thinking straight.",
            ),
            timed_seg(
                5,
                "And it's also just a good exercise to rebuild belief in the system.",
            ),
            timed_seg(
                6,
                "If maybe you had a bad week and you're like, oh, this doesn't work anymore,",
            ),
            timed_seg(
                7,
                "but you can just actually look back at all the proper examples and see how it actually would have performed.",
            ),
        ]);

        let merged = merge_dangling_source_segments(normalized);
        let sources = merged
            .iter()
            .map(|segment| segment.source.as_str())
            .collect::<Vec<_>>();

        assert!(sources.contains(
            &"against the reference list of literally reviewing a high quality example that I see"
        ));
        assert!(sources.contains(&"because sometimes your execution slips, you might skip every high quality example due to hesitation or maybe you choose weaker examples because you're not thinking straight."));
        assert!(sources.contains(&"If maybe you had a bad week and you're like, oh, this doesn't work anymore, but you can just actually look back at all the proper examples and see how it actually would have performed."));
        assert_eq!(merged.len(), 5);
    }
}
