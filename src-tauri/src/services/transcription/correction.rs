use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use voxtrans_core::subtitle::segmenter::WordToken;
use voxtrans_core::subtitle::text_rules::has_break_terminal_punctuation;

use crate::services::task_log::TaskLogger;
use crate::services::translate::adapters::rig_node::{
    JsonResponseValidator, RigNodeClient, RigNodeConfig,
};

const SENTENCE_GAP_SEC: f64 = 2.0;
const BATCH_SENTENCE_SIZE: usize = 100;
const MIN_CONFIDENCE_TO_APPLY: f64 = 0.55;

#[derive(Debug, Clone)]
pub struct CorrectionTerminologyEntry {
    pub source: String,
    pub target: String,
    pub note: String,
}

#[derive(Debug, Clone)]
pub struct CorrectionConfig {
    pub source_lang: String,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub terminology_entries: Vec<CorrectionTerminologyEntry>,
}

#[derive(Debug, Clone)]
struct SentenceUnit {
    index: usize,
    words: Vec<WordToken>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CorrectionExtraction {
    items: Vec<CorrectionOutputItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CorrectionOutputItem {
    index: usize,
    #[serde(default)]
    corrections: Vec<CorrectionPair>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CorrectionPair {
    before: String,
    after: String,
    confidence: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PreviousBatchItem {
    index: usize,
    corrected_text: String,
}

pub async fn correct_words_with_rig_node(
    task_id: &str,
    media_path: &str,
    words: Vec<WordToken>,
    config: &CorrectionConfig,
) -> Vec<WordToken> {
    let logger = TaskLogger::main_with_media(task_id.to_string(), media_path.to_string());
    if words.is_empty() {
        logger.event(
            "transcribe.correction.skip",
            Some(&json!({ "reason": "empty_words" })),
        );
        return words;
    }
    if !is_english_priority(&config.source_lang) {
        logger.event(
            "transcribe.correction.skip",
            Some(&json!({
                "reason": "source_lang_not_supported",
                "sourceLang": config.source_lang
            })),
        );
        return words;
    }
    if config.api_key.trim().is_empty()
        || config.base_url.trim().is_empty()
        || config.model.trim().is_empty()
    {
        logger.event(
            "transcribe.correction.skip",
            Some(&json!({ "reason": "missing_llm_config" })),
        );
        return words;
    }

    let mut sentences = split_sentences(&words);
    if sentences.is_empty() {
        logger.event(
            "transcribe.correction.skip",
            Some(&json!({ "reason": "empty_sentences" })),
        );
        return words;
    }

    let batch_ranges = split_batch_ranges(sentences.len(), BATCH_SENTENCE_SIZE);
    logger.event(
        "transcribe.correction.started",
        Some(&json!({
            "sentenceTotal": sentences.len(),
            "batchTotal": batch_ranges.len(),
            "batchSize": BATCH_SENTENCE_SIZE,
            "terminologyTotal": config.terminology_entries.len(),
            "model": config.model,
        })),
    );

    let rig_client = match RigNodeClient::new(RigNodeConfig::new(
        config.base_url.clone(),
        config.api_key.clone(),
        config.model.clone(),
    )) {
        Ok(client) => client,
        Err(err) => {
            logger.event(
                "transcribe.correction.error",
                Some(&json!({
                    "reason": "create_client_failed",
                    "error": err
                })),
            );
            return words;
        }
    };

    let validator = JsonResponseValidator::with_required_keys(&["items"]);
    let system_prompt = build_correction_system_prompt();
    let mut previous_batch_corrected: Vec<PreviousBatchItem> = Vec::new();

    let mut succeeded_batch_total = 0usize;
    let mut fallback_batch_total = 0usize;
    let mut parse_error_total = 0usize;
    let mut changed_sentence_total = 0usize;
    let mut changed_token_total = 0usize;
    let mut low_confidence_skip_total = 0usize;
    let mut replace_miss_total = 0usize;

    for (batch_idx, (start, end)) in batch_ranges.iter().copied().enumerate() {
        let batch = &sentences[start..end];
        let user_prompt = build_correction_user_prompt(
            &config.source_lang,
            &config.terminology_entries,
            batch,
            &previous_batch_corrected,
            batch_idx + 1,
            batch_ranges.len(),
        );
        let response = rig_client
            .call(
                task_id,
                Some(media_path),
                &system_prompt,
                &user_prompt,
                Some(&validator),
            )
            .await;

        let raw_json = match response {
            Ok(ok) => ok.json,
            Err(err) => {
                fallback_batch_total += 1;
                logger.event(
                    "transcribe.correction.error",
                    Some(&json!({
                        "reason": "llm_batch_failed",
                        "batchIndex": batch_idx + 1,
                        "error": err.message
                    })),
                );
                previous_batch_corrected = batch
                    .iter()
                    .map(|sentence| PreviousBatchItem {
                        index: sentence.index,
                        corrected_text: sentence_text(&sentence.words),
                    })
                    .collect();
                continue;
            }
        };

        let extracted = match serde_json::from_value::<CorrectionExtraction>(raw_json) {
            Ok(v) => v,
            Err(err) => {
                fallback_batch_total += 1;
                parse_error_total += 1;
                logger.event(
                    "transcribe.correction.error",
                    Some(&json!({
                        "reason": "parse_failed",
                        "batchIndex": batch_idx + 1,
                        "error": err.to_string()
                    })),
                );
                previous_batch_corrected = batch
                    .iter()
                    .map(|sentence| PreviousBatchItem {
                        index: sentence.index,
                        corrected_text: sentence_text(&sentence.words),
                    })
                    .collect();
                continue;
            }
        };

        let mut by_index: HashMap<usize, Vec<CorrectionPair>> = HashMap::new();
        let mut invalid_index = false;
        for item in extracted.items {
            if item.index < start || item.index >= end {
                invalid_index = true;
                break;
            }
            by_index.insert(item.index, item.corrections);
        }
        if invalid_index {
            fallback_batch_total += 1;
            parse_error_total += 1;
            logger.event(
                "transcribe.correction.error",
                Some(&json!({
                    "reason": "invalid_index",
                    "batchIndex": batch_idx + 1
                })),
            );
            previous_batch_corrected = batch
                .iter()
                .map(|sentence| PreviousBatchItem {
                    index: sentence.index,
                    corrected_text: sentence_text(&sentence.words),
                })
                .collect();
            continue;
        }

        succeeded_batch_total += 1;
        let mut batch_memory: Vec<PreviousBatchItem> = Vec::with_capacity(end - start);

        for sentence in &mut sentences[start..end] {
            if let Some(corrections) = by_index.get(&sentence.index) {
                let (changed_any, changed_token_in_sentence, low_confidence_skips, replace_miss) =
                    apply_corrections(&mut sentence.words, corrections);
                if changed_any {
                    changed_sentence_total += 1;
                    changed_token_total += changed_token_in_sentence;
                }
                low_confidence_skip_total += low_confidence_skips;
                replace_miss_total += replace_miss;
            }

            let corrected_text = sentence_text(&sentence.words);
            batch_memory.push(PreviousBatchItem {
                index: sentence.index,
                corrected_text,
            });
        }

        previous_batch_corrected = batch_memory;
    }

    logger.event(
        "transcribe.correction.completed",
        Some(&json!({
            "sentenceTotal": sentences.len(),
            "batchTotal": batch_ranges.len(),
            "succeededBatchTotal": succeeded_batch_total,
            "fallbackBatchTotal": fallback_batch_total,
            "parseErrorTotal": parse_error_total,
            "changedSentenceTotal": changed_sentence_total,
            "changedTokenTotal": changed_token_total,
            "lowConfidenceSkipTotal": low_confidence_skip_total,
            "replaceMissTotal": replace_miss_total,
            "minConfidenceToApply": MIN_CONFIDENCE_TO_APPLY
        })),
    );

    flatten_sentences(sentences)
}

fn is_english_priority(source_lang: &str) -> bool {
    let normalized = source_lang.trim().to_lowercase();
    normalized.is_empty()
        || normalized == "auto"
        || normalized.starts_with("en")
        || normalized.contains("english")
}

fn split_sentences(words: &[WordToken]) -> Vec<SentenceUnit> {
    if words.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::new();
    let mut start = 0usize;
    for idx in 0..words.len() {
        let is_last = idx + 1 == words.len();
        let has_terminal = has_break_terminal_punctuation(words[idx].word.trim());
        let gap_break = if is_last {
            false
        } else {
            (words[idx + 1].start - words[idx].end).max(0.0) >= SENTENCE_GAP_SEC
        };
        if !(is_last || has_terminal || gap_break) {
            continue;
        }
        let end = idx;
        if start <= end {
            out.push(SentenceUnit {
                index: out.len(),
                words: words[start..=end].to_vec(),
            });
        }
        start = idx + 1;
    }

    out
}

fn split_batch_ranges(total: usize, size: usize) -> Vec<(usize, usize)> {
    if total == 0 || size == 0 {
        return Vec::new();
    }

    let mut ranges = Vec::new();
    let mut cursor = 0usize;
    while cursor < total {
        let end = (cursor + size).min(total);
        ranges.push((cursor, end));
        cursor = end;
    }
    ranges
}

fn flatten_sentences(sentences: Vec<SentenceUnit>) -> Vec<WordToken> {
    let total = sentences.iter().map(|sentence| sentence.words.len()).sum();
    let mut out = Vec::with_capacity(total);
    for mut sentence in sentences {
        out.append(&mut sentence.words);
    }
    out
}

fn sentence_text(words: &[WordToken]) -> String {
    words
        .iter()
        .map(|w| w.word.as_str())
        .collect::<Vec<_>>()
        .join(" ")
}

fn build_correction_system_prompt() -> String {
    "You are an ASR correction assistant. Only correct obvious recognition mistakes. \
Do not rewrite meaning. \
Return strict JSON only: {\"items\":[{\"index\":0,\"corrections\":[{\"before\":\"...\",\"after\":\"...\",\"confidence\":0.0}]}]}. \
Only include changed sentences in items. \
Each corrections item must include before/after/confidence. \
confidence must be in [0,1].".to_string()
}

fn build_correction_user_prompt(
    source_lang: &str,
    terminology_entries: &[CorrectionTerminologyEntry],
    batch: &[SentenceUnit],
    previous_batch_corrected: &[PreviousBatchItem],
    batch_index: usize,
    batch_total: usize,
) -> String {
    let terms = terminology_entries
        .iter()
        .map(|entry| {
            json!({
                "source": entry.source,
                "target": entry.target,
                "note": entry.note
            })
        })
        .collect::<Vec<_>>();

    let items = batch
        .iter()
        .map(|sentence| {
            json!({
                "index": sentence.index,
                "sourceText": sentence_text(&sentence.words)
            })
        })
        .collect::<Vec<_>>();

    json!({
        "task": "asr_correction",
        "sourceLang": source_lang,
        "batch": {
            "index": batch_index,
            "total": batch_total
        },
        "terminology": terms,
        "previousBatchCorrected": previous_batch_corrected,
        "items": items,
        "requirements": [
            "Only fix ASR mistakes, no semantic rewrite",
            "Return only changed sentences in items",
            "For each changed sentence, return corrections list with before/after/confidence",
            "confidence must be in [0,1]"
        ],
        "output": {
            "jsonOnly": true,
            "schema": {
                "items": [
                    {
                        "index": "number",
                        "corrections": [
                            {
                                "before": "string",
                                "after": "string",
                                "confidence": "number(0..1)"
                            }
                        ]
                    }
                ]
            }
        }
    })
    .to_string()
}

fn apply_corrections(words: &mut Vec<WordToken>, corrections: &[CorrectionPair]) -> (bool, usize, usize, usize) {
    let mut changed_any = false;
    let mut changed_token_total = 0usize;
    let mut low_confidence_skip_total = 0usize;
    let mut replace_miss_total = 0usize;
    let mut cursor = 0usize;

    for pair in corrections {
        let confidence = pair.confidence.clamp(0.0, 1.0);
        if confidence < MIN_CONFIDENCE_TO_APPLY {
            low_confidence_skip_total += 1;
            continue;
        }

        let before_tokens = tokenize_phrase(&pair.before);
        let after_tokens = tokenize_phrase(&pair.after);
        if before_tokens.is_empty() || after_tokens.is_empty() {
            replace_miss_total += 1;
            continue;
        }

        let Some((start, end)) = find_subsequence(words, &before_tokens, cursor) else {
            replace_miss_total += 1;
            continue;
        };

        let source_slice = words[start..=end].to_vec();
        let replacement = build_replacement_tokens(&source_slice, &after_tokens);
        let source_len = source_slice.len();
        let replacement_len = replacement.len();
        words.splice(start..=end, replacement);
        cursor = start + replacement_len;
        changed_any = true;
        changed_token_total += source_len.max(replacement_len);
    }

    (
        changed_any,
        changed_token_total,
        low_confidence_skip_total,
        replace_miss_total,
    )
}

fn tokenize_phrase(input: &str) -> Vec<String> {
    input
        .split_whitespace()
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(|token| token.to_string())
        .collect()
}

fn normalized_token(input: &str) -> String {
    input
        .chars()
        .filter(|ch| ch.is_alphanumeric())
        .flat_map(|ch| ch.to_lowercase())
        .collect()
}

fn find_subsequence(words: &[WordToken], before_tokens: &[String], cursor: usize) -> Option<(usize, usize)> {
    if before_tokens.is_empty() || words.is_empty() {
        return None;
    }
    if before_tokens.len() > words.len() {
        return None;
    }
    let before_norms = before_tokens
        .iter()
        .map(|token| normalized_token(token))
        .collect::<Vec<_>>();

    if before_norms.iter().any(|norm| norm.is_empty()) {
        return None;
    }

    let start_from = cursor.min(words.len());
    for start in start_from..words.len() {
        let end = start + before_tokens.len();
        if end > words.len() {
            break;
        }
        let mut matched = true;
        for idx in 0..before_tokens.len() {
            if normalized_token(&words[start + idx].word) != before_norms[idx] {
                matched = false;
                break;
            }
        }
        if matched {
            return Some((start, end - 1));
        }
    }
    None
}

fn build_replacement_tokens(source_slice: &[WordToken], after_tokens: &[String]) -> Vec<WordToken> {
    if source_slice.is_empty() || after_tokens.is_empty() {
        return source_slice.to_vec();
    }

    let start = source_slice[0].start;
    let end = source_slice[source_slice.len() - 1].end.max(start);
    let source_len = source_slice.len();
    let target_len = after_tokens.len();

    if source_len == 1 && target_len == 1 {
        return vec![WordToken {
            start: source_slice[0].start,
            end: source_slice[0].end,
            word: after_tokens[0].clone(),
        }];
    }

    if target_len == 1 {
        return vec![WordToken {
            start,
            end,
            word: after_tokens[0].clone(),
        }];
    }

    let weights = after_tokens
        .iter()
        .map(|token| token_weight(token))
        .collect::<Vec<_>>();
    let slots = split_interval_by_weight(start, end, &weights);

    after_tokens
        .iter()
        .enumerate()
        .map(|(idx, token)| {
            let (slot_start, slot_end) = slots
                .get(idx)
                .copied()
                .unwrap_or((start, end));
            WordToken {
                start: slot_start,
                end: slot_end.max(slot_start),
                word: token.clone(),
            }
        })
        .collect()
}

fn token_weight(token: &str) -> f64 {
    let count = token
        .chars()
        .filter(|ch| ch.is_alphanumeric())
        .count();
    if count == 0 {
        1.0
    } else {
        count as f64
    }
}

fn split_interval_by_weight(start: f64, end: f64, weights: &[f64]) -> Vec<(f64, f64)> {
    if weights.is_empty() {
        return Vec::new();
    }
    let safe_start = if start.is_finite() { start } else { 0.0 };
    let mut safe_end = if end.is_finite() { end } else { safe_start };
    if safe_end < safe_start {
        safe_end = safe_start;
    }
    let total = weights.iter().copied().sum::<f64>().max(1.0);
    let span = (safe_end - safe_start).max(0.0);

    let mut out = Vec::with_capacity(weights.len());
    let mut cursor = safe_start;
    for (idx, weight) in weights.iter().copied().enumerate() {
        let is_last = idx + 1 == weights.len();
        let slot_end = if is_last {
            safe_end
        } else {
            cursor + span * (weight.max(0.0) / total)
        };
        out.push((cursor, slot_end.max(cursor)));
        cursor = slot_end.max(cursor);
    }
    if let Some(last) = out.last_mut() {
        last.1 = safe_end.max(last.0);
    }
    out
}
