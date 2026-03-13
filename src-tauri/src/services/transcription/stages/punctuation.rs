use serde_json::Value;
use sqlx::SqlitePool;

use crate::services::llm::LlmInteractRequest;
use crate::prompt_builder::BuildPunctuationRestorePromptRequest;
use crate::services::preferences::LlmSettings;
use crate::services::transcribe::WordTokenDto;

use crate::services::transcription::domain::{PunctuationStats, StageResult, TemporarySentence};

pub async fn run_stage(
    words: &mut [WordTokenDto],
    threads: u32,
    llm: &LlmSettings,
    usage_pool: Option<&SqlitePool>,
    log_ctx: Option<(&str, &str)>,
) -> Result<StageResult<PunctuationStats>, String> {
    if llm.api_key.trim().is_empty() || llm.api_model.trim().is_empty() || words.is_empty() {
        return Ok(StageResult::skipped());
    }
    let sentences = split_temporary_sentences(words);
    let suspicious = sentences
        .iter()
        .filter(|s| is_suspicious_sentence(&s.text))
        .cloned()
        .collect::<Vec<_>>();
    let mut stats = PunctuationStats {
        sentence_total: sentences.len(),
        suspicious_count: suspicious.len(),
        ..Default::default()
    };
    if suspicious.is_empty() {
        return Ok(StageResult::skipped_with(stats));
    }

    let max_threads = threads.clamp(1, 16) as usize;
    for batch in suspicious.chunks(max_threads) {
        for sentence in batch {
            let prompt = crate::prompt_builder::build_punctuation_restore_prompt(
                BuildPunctuationRestorePromptRequest {
                    text: sentence.text.clone(),
                },
            )?;
            let response = crate::services::llm::llm_interact(LlmInteractRequest {
                api_key: llm.api_key.clone(),
                model: llm.api_model.clone(),
                base_url: if llm.api_base.trim().is_empty() {
                    None
                } else {
                    Some(llm.api_base.clone())
                },
                system_prompt: Some(prompt.system_prompt),
                prompt: Some(prompt.user_prompt),
                messages: None,
                mode: Some("chat".to_string()),
                tools: None,
                tool_results: None,
                tool_choice: None,
                temperature: None,
                max_tokens: None,
                timeout_secs: Some(120),
                max_retries: Some(2),
                log_task_id: log_ctx.map(|v| v.0.to_string()),
                log_media_path: log_ctx.map(|v| v.1.to_string()),
                log_stage: Some("punctuation".to_string()),
                usage_pool: usage_pool.cloned(),
            })
            .await?;

            let restored = parse_restore_text(response.message.unwrap_or_default());
            if restored.is_empty() {
                stats.rejected_count += 1;
                continue;
            }
            stats.restored_count += 1;

            let original_tokens = words[sentence.start_word..sentence.end_word_exclusive]
                .iter()
                .map(|w| w.word.clone())
                .collect::<Vec<_>>();
            let Some(projected) =
                project_restored_text_to_word_tokens(&restored, original_tokens.len())
            else {
                stats.rejected_count += 1;
                continue;
            };
            if !same_lexical_tokens(&original_tokens, &projected) {
                stats.rejected_count += 1;
                continue;
            }

            for (offset, token) in projected.iter().enumerate() {
                words[sentence.start_word + offset].word = token.clone();
            }
            stats.accepted_count += 1;
        }
    }

    Ok(StageResult::executed(stats))
}

fn split_temporary_sentences(words: &[WordTokenDto]) -> Vec<TemporarySentence> {
    let mut out = Vec::new();
    let mut start = 0usize;
    for (idx, word) in words.iter().enumerate() {
        let token = word.word.trim();
        if token.is_empty() || !is_sentence_end_token(token) || is_abbreviation_token(token) {
            continue;
        }
        out.push(TemporarySentence {
            start_word: start,
            end_word_exclusive: idx + 1,
            text: join_word_texts(
                &words[start..=idx]
                    .iter()
                    .map(|w| w.word.clone())
                    .collect::<Vec<_>>(),
            ),
        });
        start = idx + 1;
    }
    if start < words.len() {
        out.push(TemporarySentence {
            start_word: start,
            end_word_exclusive: words.len(),
            text: join_word_texts(
                &words[start..]
                    .iter()
                    .map(|w| w.word.clone())
                    .collect::<Vec<_>>(),
            ),
        });
    }
    out.into_iter()
        .filter(|s| !s.text.trim().is_empty())
        .collect()
}

fn is_sentence_end_token(token: &str) -> bool {
    let trimmed = token.trim_end_matches([')', '"', '\'', ']', '】']);
    matches!(
        trimmed.chars().last(),
        Some('.') | Some('!') | Some('?') | Some('。') | Some('！') | Some('？')
    )
}

fn is_abbreviation_token(token: &str) -> bool {
    let normalized = token
        .trim_matches(['(', '"', '\'', '[', ')', ']', '】'])
        .to_ascii_lowercase();
    const ABBR: &[&str] = &[
        "mr.", "mrs.", "ms.", "dr.", "prof.", "sr.", "jr.", "st.", "vs.", "etc.", "e.g.",
        "i.e.", "u.s.", "u.k.",
    ];
    if ABBR.contains(&normalized.as_str()) {
        return true;
    }
    let chars = normalized.chars().collect::<Vec<_>>();
    chars.len() == 2 && chars[0].is_ascii_alphabetic() && chars[1] == '.'
}

fn is_suspicious_sentence(text: &str) -> bool {
    let trimmed = text.trim();
    !trimmed.is_empty() && !should_skip_sentence(trimmed) && !is_sentence_end_token(trimmed)
}

fn should_skip_sentence(text: &str) -> bool {
    if text.split_whitespace().filter(|v| !v.is_empty()).count() <= 2 {
        return true;
    }
    if text
        .chars()
        .all(|c| c.is_ascii_digit() || c.is_whitespace() || [':', '.', '/', '-'].contains(&c))
    {
        return true;
    }
    let lower = text.to_ascii_lowercase();
    lower.contains("http://")
        || lower.contains("https://")
        || lower.contains("www.")
        || lower.contains(":\\")
        || lower.contains('\\')
        || lower.contains('/')
}

fn parse_restore_text(raw: String) -> String {
    let trimmed = raw.trim().to_string();
    if trimmed.is_empty() {
        return String::new();
    }
    let parsed = serde_json::from_str::<Value>(&trimmed).ok().or_else(|| {
        let fenced_start = trimmed.find("```")?;
        let fenced_end = trimmed[fenced_start + 3..].find("```")?;
        let body = &trimmed[fenced_start + 3..fenced_start + 3 + fenced_end];
        let body = body.strip_prefix("json").unwrap_or(body).trim();
        serde_json::from_str::<Value>(body).ok()
    });
    parsed
        .and_then(|v| v.get("text").and_then(|v| v.as_str()).map(str::to_string))
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn project_restored_text_to_word_tokens(text: &str, expected_count: usize) -> Option<Vec<String>> {
    if expected_count == 0 {
        return Some(Vec::new());
    }
    let raw = text
        .split_whitespace()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    if raw.is_empty() {
        return None;
    }
    let merged = merge_standalone_punctuation(&raw);
    if merged.len() != expected_count {
        return None;
    }
    Some(merged)
}

fn merge_standalone_punctuation(tokens: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for token in tokens {
        if token.chars().all(|ch| !ch.is_alphanumeric()) && !out.is_empty() {
            if let Some(last) = out.last_mut() {
                last.push_str(token);
            }
        } else {
            out.push(token.clone());
        }
    }
    out
}

fn same_lexical_tokens(a: &[String], b: &[String]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter()
        .zip(b.iter())
        .all(|(x, y)| normalize_lexical(x) == normalize_lexical(y))
}

fn normalize_lexical(token: &str) -> String {
    token
        .trim_matches(|c: char| !c.is_alphanumeric())
        .to_ascii_lowercase()
}

fn join_word_texts(words: &[String]) -> String {
    words
        .iter()
        .map(|v| v.trim())
        .filter(|v| !v.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

