use serde_json::json;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use voxtrans_core::subtitle::segmenter::WordToken;
use voxtrans_core::subtitle::text_rules::{
    has_break_terminal_punctuation, should_split_after_terminal_token, strip_trailing_closers,
};

use crate::services::task_log::TaskLogger;
use crate::services::translate::adapters::rig_node::{
    JsonResponseValidator, RigNodeClient, RigNodeConfig, RigNodeJsonTask,
};
use crate::services::translate::prompt::{
    PunctuationPromptInput, build_punctuation_system_prompt, build_punctuation_user_prompt,
};

const SENTENCE_GAP_SEC: f64 = 2.0;
const MIN_WORDS_TO_OPTIMIZE: usize = 3;
const CONTEXT_MAX_WORDS: usize = 12;
const LONG_SENTENCE_WORD_THRESHOLD: usize = 45;
const LONG_SENTENCE_MAX_SPLIT_DEPTH: usize = 2;
const MIN_SPLIT_CHUNK_WORDS: usize = 12;
const SPLIT_GAP_CANDIDATE_SEC: f64 = 0.25;

#[derive(Debug, Clone)]
pub struct PunctuationConfig {
    pub enabled: bool,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub llm_concurrency: u32,
}

#[derive(Debug, Clone)]
struct SentenceSpan {
    start_idx: usize,
    end_idx: usize,
    text: String,
    allow_leading_case_change: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct PunctuationExtraction {
    punctuated_text: String,
}

pub async fn optimize_words_with_rig_node(
    task_id: &str,
    media_path: &str,
    words: Vec<WordToken>,
    config: &PunctuationConfig,
) -> Result<Vec<WordToken>, String> {
    let logger = TaskLogger::main_with_media(task_id.to_string(), media_path.to_string());
    if !config.enabled {
        logger.event(
            "transcribe.punctuation.skip",
            Some(&json!({
                "reason": "disabled"
            })),
        );
        return Ok(words);
    }
    if config.api_key.trim().is_empty() {
        let message = "punctuation failed: missing API key".to_string();
        logger.event("transcribe.punctuation.error", Some(&json!({ "reason": "missing_api_key" })));
        return Err(message);
    }
    if config.model.trim().is_empty() {
        let message = "punctuation failed: missing model".to_string();
        logger.event("transcribe.punctuation.error", Some(&json!({ "reason": "missing_model" })));
        return Err(message);
    }
    if config.base_url.trim().is_empty() {
        let message = "punctuation failed: missing base URL".to_string();
        logger.event("transcribe.punctuation.error", Some(&json!({ "reason": "missing_base_url" })));
        return Err(message);
    }

    let spans = build_sentence_spans(&words);
    let suspicious_indexes: Vec<usize> = spans
        .iter()
        .enumerate()
        .filter(|(_, span)| should_optimize_sentence(&words, span))
        .map(|(idx, _)| idx)
        .collect();

    if suspicious_indexes.is_empty() {
        logger.event(
            "transcribe.punctuation.skip",
            Some(&json!({
                "reason": "no_eligible_sentence",
                "sentenceTotal": spans.len()
            })),
        );
        return Ok(words);
    }
    logger.event(
        "transcribe.punctuation.suspicious_detected",
        Some(&json!({
            "sentenceTotal": spans.len(),
            "suspiciousSentenceTotal": suspicious_indexes.len()
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
                "transcribe.punctuation.client_error",
                Some(&json!({ "error": err.to_string() })),
            );
            return Err(format!("punctuation failed: {}", err));
        }
    };
    let system_prompt = build_punctuation_system_prompt();
    let concurrency = config.llm_concurrency.clamp(1, 16) as usize;
    let mut prompt_tasks: Vec<(usize, String)> = Vec::new();
    for span_idx in suspicious_indexes.iter().copied() {
        let span = match spans.get(span_idx) {
            Some(s) => s,
            None => continue,
        };
        let prev = spans
            .get(span_idx.saturating_sub(1))
            .map(|s| clip_context_words(&s.text, CONTEXT_MAX_WORDS))
            .unwrap_or_default();
        let next = spans
            .get(span_idx + 1)
            .map(|s| clip_context_words(&s.text, CONTEXT_MAX_WORDS))
            .unwrap_or_default();
        let user_prompt = build_punctuation_user_prompt(&PunctuationPromptInput {
            previous_text: prev,
            current_text: span.text.clone(),
            next_text: next,
        });
        prompt_tasks.push((span_idx, user_prompt));
    }
    let validator = JsonResponseValidator::with_required_keys(&["punctuatedText"]);
    let tasks = prompt_tasks
        .iter()
        .enumerate()
        .map(|(idx, (_, user_prompt))| RigNodeJsonTask {
            id: idx,
            system_prompt: system_prompt.clone(),
            user_prompt: user_prompt.clone(),
            response_validator: Some(validator.clone()),
        })
        .collect::<Vec<_>>();
    let extraction_result = rig_client
        .call_batch(task_id, Some(media_path), "punctuate", tasks, concurrency)
        .await;

    let mut optimized = words.clone();
    let mut changed_tokens = 0usize;
    let mut llm_error_total = 0usize;
    let mut empty_result_total = 0usize;
    let mut applied_total = 0usize;
    for (result_idx, result) in extraction_result {
        let Some((span_idx, _)) = prompt_tasks.get(result_idx) else {
            llm_error_total += 1;
            continue;
        };
        let json = match result {
            Ok(ok) => ok.json,
            Err(err) => {
                return Err(format!("punctuation failed: {}", err.message));
            }
        };
        let extraction = match serde_json::from_value::<PunctuationExtraction>(json) {
            Ok(v) => v,
            Err(err) => {
                return Err(format!("punctuation parse failed: {err}"));
            }
        };
        let punctuated = extraction.punctuated_text.trim().to_string();
        if punctuated.is_empty() {
            empty_result_total += 1;
            continue;
        }
        let span = match spans.get(*span_idx) {
            Some(v) => v,
            None => continue,
        };
        if let Some(slice) = optimized.get_mut(span.start_idx..=span.end_idx) {
            changed_tokens += apply_style_from_suggestion(
                slice,
                &punctuated,
                span.allow_leading_case_change,
            );
            applied_total += 1;
        }
    }

    logger.event(
        "transcribe.punctuation.completed",
        Some(&json!({
            "sentenceTotal": spans.len(),
            "optimizedSentenceTotal": suspicious_indexes.len(),
            "appliedSentenceTotal": applied_total,
            "emptyResultTotal": empty_result_total,
            "llmErrorTotal": llm_error_total,
            "changedTokenTotal": changed_tokens,
            "llmMetrics": {
                "concurrency": concurrency,
                "requestTotal": suspicious_indexes.len(),
                "backend": "rig_node_client"
            }
        })),
    );
    Ok(optimized)
}

fn build_sentence_spans(words: &[WordToken]) -> Vec<SentenceSpan> {
    if words.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::new();
    let mut start = 0usize;

    for idx in 0..words.len() {
        let is_last = idx + 1 == words.len();
        let has_terminal = should_split_after_word(words, idx);
        let gap_break = if is_last {
            false
        } else {
            (words[idx + 1].start - words[idx].end).max(0.0) >= SENTENCE_GAP_SEC
        };
        if !(is_last || has_terminal || gap_break) {
            continue;
        }

        let end = idx;
        out.extend(split_overlong_span(words, start, end, 0, true));
        start = idx + 1;
    }

    out
}

fn split_overlong_span(
    words: &[WordToken],
    start_idx: usize,
    end_idx: usize,
    depth: usize,
    allow_leading_case_change: bool,
) -> Vec<SentenceSpan> {
    if start_idx > end_idx || end_idx >= words.len() {
        return Vec::new();
    }

    let word_count = end_idx - start_idx + 1;
    if word_count <= LONG_SENTENCE_WORD_THRESHOLD || depth >= LONG_SENTENCE_MAX_SPLIT_DEPTH {
        return vec![build_span(
            words,
            start_idx,
            end_idx,
            allow_leading_case_change,
        )];
    }

    let split_idx = match find_best_split_index(words, start_idx, end_idx) {
        Some(v) => v,
        None => {
            return vec![build_span(
                words,
                start_idx,
                end_idx,
                allow_leading_case_change,
            )]
        }
    };

    let mut out = split_overlong_span(
        words,
        start_idx,
        split_idx,
        depth + 1,
        allow_leading_case_change,
    );
    out.extend(split_overlong_span(
        words,
        split_idx + 1,
        end_idx,
        depth + 1,
        false,
    ));
    out
}

fn build_span(
    words: &[WordToken],
    start_idx: usize,
    end_idx: usize,
    allow_leading_case_change: bool,
) -> SentenceSpan {
    let text = words[start_idx..=end_idx]
        .iter()
        .map(|w| w.word.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    SentenceSpan {
        start_idx,
        end_idx,
        text,
        allow_leading_case_change,
    }
}

fn find_best_split_index(words: &[WordToken], start_idx: usize, end_idx: usize) -> Option<usize> {
    if end_idx <= start_idx {
        return None;
    }

    let center = start_idx + (end_idx - start_idx) / 2;
    let mut best_score = f64::NEG_INFINITY;
    let mut best_idx: Option<usize> = None;

    for idx in start_idx..end_idx {
        let left_count = idx - start_idx + 1;
        let right_count = end_idx - idx;
        if left_count < MIN_SPLIT_CHUNK_WORDS || right_count < MIN_SPLIT_CHUNK_WORDS {
            continue;
        }

        let mut score = 0.0_f64;
        let distance = (idx as isize - center as isize).abs() as f64;
        score -= distance * 1.8;

        let gap_sec = gap_after(words, idx);
        if gap_sec >= SPLIT_GAP_CANDIDATE_SEC {
            score += 14.0 + (gap_sec * 6.0).min(10.0);
        }

        if ends_with_split_punctuation(&words[idx].word) {
            score += 9.0;
        }
        if starts_with_connector(&words[idx + 1].word) {
            score += 8.0;
        }
        if ends_with_soft_clause_marker(&words[idx].word) {
            score += 4.0;
        }

        if left_count <= 5 || right_count <= 5 {
            score -= 20.0;
        }

        if score > best_score {
            best_score = score;
            best_idx = Some(idx);
        }
    }

    if best_score < -5.0 {
        return None;
    }
    best_idx
}

fn gap_after(words: &[WordToken], idx: usize) -> f64 {
    if idx + 1 >= words.len() {
        return 0.0;
    }
    (words[idx + 1].start - words[idx].end).max(0.0)
}

fn ends_with_split_punctuation(token: &str) -> bool {
    let trimmed = strip_trailing_closers(token.trim());
    trimmed
        .chars()
        .last()
        .map(|c| matches!(c, ',' | '，' | ';' | '；' | ':' | '：'))
        .unwrap_or(false)
}

fn ends_with_soft_clause_marker(token: &str) -> bool {
    let trimmed = strip_trailing_closers(token.trim());
    trimmed
        .chars()
        .last()
        .map(|c| matches!(c, '-' | '—'))
        .unwrap_or(false)
}

fn starts_with_connector(token: &str) -> bool {
    const CONNECTORS: &[&str] = &[
        "and",
        "or",
        "but",
        "so",
        "because",
        "which",
        "that",
        "if",
        "when",
        "while",
        "however",
        "therefore",
        "meanwhile",
        "then",
        "also",
    ];
    let normalized = normalize_token_for_match(token);
    CONNECTORS.contains(&normalized.as_str())
}

fn normalize_token_for_match(token: &str) -> String {
    strip_trailing_closers(token.trim())
        .trim_matches(|c: char| !c.is_alphanumeric() && c != '\'' && c != '-')
        .to_ascii_lowercase()
}

fn should_optimize_sentence(_words: &[WordToken], span: &SentenceSpan) -> bool {
    let word_count = span.end_idx.saturating_sub(span.start_idx) + 1;
    if word_count < MIN_WORDS_TO_OPTIMIZE {
        return false;
    }

    let normalized = normalize_whitespace_text(&span.text);
    if normalized.is_empty() {
        return false;
    }

    let effective_word_count = count_effective_words(&normalized);
    if effective_word_count < MIN_WORDS_TO_OPTIMIZE {
        return false;
    }

    let sentence_punctuation = count_sentence_punctuation(&normalized);
    let first_is_lowercase = first_alpha_is_lowercase(&normalized);
    let has_uppercase = has_any_uppercase_alpha(&normalized);

    (effective_word_count >= 20 && sentence_punctuation == 0)
        || (effective_word_count >= 35 && sentence_punctuation <= 1)
        || (effective_word_count >= 50 && sentence_punctuation <= 2)
        || (effective_word_count >= 12 && first_is_lowercase && !has_uppercase)
}

fn normalize_whitespace_text(input: &str) -> String {
    input.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn count_effective_words(text: &str) -> usize {
    text.split_whitespace()
        .filter(|token| token.chars().any(|ch| ch.is_ascii_alphanumeric()))
        .count()
}

fn count_sentence_punctuation(text: &str) -> usize {
    text.split_whitespace()
        .filter(|token| has_break_terminal_punctuation(token.trim()))
        .count()
}

fn first_alpha_is_lowercase(text: &str) -> bool {
    text.chars()
        .find(|ch| ch.is_alphabetic())
        .map(|ch| ch.is_lowercase())
        .unwrap_or(false)
}

fn has_any_uppercase_alpha(text: &str) -> bool {
    text.chars().any(|ch| ch.is_alphabetic() && ch.is_uppercase())
}

fn should_split_after_word(words: &[WordToken], idx: usize) -> bool {
    let current_word = match words.get(idx) {
        Some(w) => w.word.as_str(),
        None => return false,
    };
    let next_word = words.get(idx + 1).map(|w| w.word.as_str());
    should_split_after_terminal_token(current_word, next_word)
}

fn clip_context_words(text: &str, max_words: usize) -> String {
    if max_words == 0 {
        return String::new();
    }
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.len() <= max_words {
        return text.to_string();
    }
    words[..max_words].join(" ")
}

fn apply_style_from_suggestion(
    words: &mut [WordToken],
    suggestion: &str,
    allow_leading_case_change: bool,
) -> usize {
    let suggested_tokens: Vec<&str> = suggestion.split_whitespace().collect();
    if words.is_empty() || suggested_tokens.is_empty() {
        return 0;
    }

    let original_norms: Vec<String> = words
        .iter()
        .map(|w| {
            let (base, _) = split_trailing_punctuation(&w.word);
            normalized_word(&base)
        })
        .collect();
    let suggested_norms: Vec<String> = suggested_tokens
        .iter()
        .map(|token| {
            let (base, _) = split_trailing_punctuation(token);
            normalized_word(&base)
        })
        .collect();
    let alignment = align_token_indexes(&original_norms, &suggested_norms);

    let mut changed = 0usize;
    for (orig_idx, maybe_sugg_idx) in alignment.into_iter().enumerate() {
        let sugg_idx = match maybe_sugg_idx {
            Some(v) => v,
            None => continue,
        };
        let suggested_token = match suggested_tokens.get(sugg_idx) {
            Some(v) => *v,
            None => continue,
        };

        let original = words[orig_idx].word.clone();
        let (orig_base, _) = split_trailing_punctuation(&original);
        let (sugg_base, sugg_suffix) = split_trailing_punctuation(suggested_token);
        let mut next_base = orig_base.clone();

        if normalized_word(&orig_base) == normalized_word(&sugg_base) {
            if orig_idx == 0 && !allow_leading_case_change {
                next_base = orig_base.clone();
            } else {
                next_base = apply_case_pattern(&orig_base, &sugg_base);
            }
        }

        let candidate = format!("{next_base}{sugg_suffix}");
        if candidate != original {
            words[orig_idx].word = candidate;
            changed += 1;
        }
    }

    changed
}

fn align_token_indexes(original_norms: &[String], suggested_norms: &[String]) -> Vec<Option<usize>> {
    let original_nonempty: Vec<(usize, &str)> = original_norms
        .iter()
        .enumerate()
        .filter_map(|(idx, norm)| {
            if norm.is_empty() {
                None
            } else {
                Some((idx, norm.as_str()))
            }
        })
        .collect();
    let suggested_nonempty: Vec<(usize, &str)> = suggested_norms
        .iter()
        .enumerate()
        .filter_map(|(idx, norm)| {
            if norm.is_empty() {
                None
            } else {
                Some((idx, norm.as_str()))
            }
        })
        .collect();

    let m = original_nonempty.len();
    let n = suggested_nonempty.len();
    let mut dp = vec![vec![0usize; n + 1]; m + 1];
    for i in 0..m {
        for (j, (_, sugg_norm)) in suggested_nonempty.iter().enumerate().take(n) {
            if original_nonempty[i].1 == *sugg_norm {
                dp[i + 1][j + 1] = dp[i][j] + 1;
            } else {
                dp[i + 1][j + 1] = dp[i + 1][j].max(dp[i][j + 1]);
            }
        }
    }

    let mut mapped = vec![None; original_norms.len()];
    let mut i = m;
    let mut j = n;
    while i > 0 && j > 0 {
        let (orig_word_idx, orig_norm) = original_nonempty[i - 1];
        let (sugg_word_idx, sugg_norm) = suggested_nonempty[j - 1];
        if orig_norm == sugg_norm {
            mapped[orig_word_idx] = Some(sugg_word_idx);
            i -= 1;
            j -= 1;
            continue;
        }
        if dp[i - 1][j] >= dp[i][j - 1] {
            i -= 1;
        } else {
            j -= 1;
        }
    }

    mapped
}

fn split_trailing_punctuation(token: &str) -> (String, String) {
    let chars: Vec<char> = token.chars().collect();
    if chars.is_empty() {
        return (String::new(), String::new());
    }
    let mut split_at = chars.len();
    while split_at > 0 && is_punctuation(chars[split_at - 1]) {
        split_at -= 1;
    }
    let base: String = chars[..split_at].iter().collect();
    let suffix: String = chars[split_at..].iter().collect();
    (base, suffix)
}

fn is_punctuation(ch: char) -> bool {
    matches!(
        ch,
        '.' | ',' | '!' | '?' | ';' | ':' | '，' | '。' | '！' | '？' | '；' | '：'
    )
}

fn normalized_word(input: &str) -> String {
    input
        .chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

fn apply_case_pattern(original: &str, suggested: &str) -> String {
    let suggested_alpha: String = suggested.chars().filter(|c| c.is_alphabetic()).collect();
    if suggested_alpha.is_empty() {
        return original.to_string();
    }

    if suggested_alpha.chars().all(|c| !c.is_lowercase()) {
        return original.to_ascii_uppercase();
    }
    if suggested_alpha.chars().all(|c| !c.is_uppercase()) {
        return original.to_ascii_lowercase();
    }

    capitalize_first_alpha(original)
}

fn capitalize_first_alpha(input: &str) -> String {
    let mut chars: Vec<char> = input.chars().collect();
    for ch in &mut chars {
        if ch.is_alphabetic() {
            *ch = ch.to_ascii_uppercase();
            break;
        }
    }
    chars.into_iter().collect()
}
