#![allow(dead_code)]

use crate::services::transcribe::WordTokenDto;
use pinyin::ToPinyin;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HotwordLang {
    Auto,
    Zh,
    NonZh,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HotwordEntry {
    pub word: String,
    pub aliases: Vec<String>,
    pub lang: HotwordLang,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NormalizedHotword {
    pub word: String,
    pub aliases: Vec<String>,
    pub generated_aliases: Vec<String>,
    pub pinyin: String,
    pub first_letters: String,
    pub lang: HotwordLang,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HotwordCandidate {
    pub id: String,
    pub start_index: usize,
    pub end_index: usize,
    pub source_text: String,
    pub target: String,
    pub source_kind: String,
    pub context: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HotwordDecision {
    pub candidate_id: String,
    pub replace: bool,
    pub target: String,
    pub reason: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildHotwordCorrectionRequest {
    pub task_id: String,
    pub media_path: String,
    pub source_lang: String,
    pub words: Vec<WordTokenDto>,
    pub hotwords: Vec<HotwordEntry>,
    pub enabled: bool,
    pub translate_api_key: String,
    pub translate_base_url: String,
    pub translate_model: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HotwordCorrection {
    pub candidate_id: String,
    pub source_text: String,
    pub target: String,
    pub start_index: usize,
    pub end_index: usize,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildHotwordCorrectionResponse {
    pub task_id: String,
    pub media_path: String,
    pub source_lang: String,
    pub enabled: bool,
    pub hotwords: Vec<NormalizedHotword>,
    pub candidates: Vec<HotwordCandidate>,
    pub decisions: Vec<HotwordDecision>,
    pub corrections: Vec<HotwordCorrection>,
    pub words: Vec<WordTokenDto>,
}

pub fn build_hotword_correction(
    request: BuildHotwordCorrectionRequest,
) -> BuildHotwordCorrectionResponse {
    let normalized_hotwords = normalize_hotwords(&request.hotwords);
    if !request.enabled || normalized_hotwords.is_empty() || request.words.is_empty() {
        return BuildHotwordCorrectionResponse {
            task_id: request.task_id,
            media_path: request.media_path,
            source_lang: request.source_lang,
            enabled: false,
            hotwords: normalized_hotwords,
            candidates: Vec::new(),
            decisions: Vec::new(),
            corrections: Vec::new(),
            words: request.words,
        };
    }

    let candidates = recall_hotword_candidates(&request.words, &request.hotwords);
    if candidates.is_empty() {
        return BuildHotwordCorrectionResponse {
            task_id: request.task_id,
            media_path: request.media_path,
            source_lang: request.source_lang,
            enabled: true,
            hotwords: normalized_hotwords,
            candidates,
            decisions: Vec::new(),
            corrections: Vec::new(),
            words: request.words,
        };
    }

    let llm_available = !request.translate_api_key.trim().is_empty()
        && !request.translate_base_url.trim().is_empty()
        && !request.translate_model.trim().is_empty();
    let decisions = candidates
        .iter()
        .map(|candidate| {
            let (reason, error) = if llm_available {
                (
                    Some("llm_decision_not_connected_yet".to_string()),
                    Some("llm_decision_not_connected_yet".to_string()),
                )
            } else {
                (Some(String::new()), Some("llm_unavailable".to_string()))
            };
            HotwordDecision {
                candidate_id: candidate.id.clone(),
                replace: false,
                target: candidate.target.clone(),
                reason,
                error,
            }
        })
        .collect::<Vec<_>>();
    let words = apply_hotword_corrections(&request.words, &candidates, &decisions);
    let corrections = decisions
        .iter()
        .filter(|decision| decision.replace)
        .filter_map(|decision| {
            candidates
                .iter()
                .find(|candidate| candidate.id == decision.candidate_id)
                .map(|candidate| HotwordCorrection {
                    candidate_id: candidate.id.clone(),
                    source_text: candidate.source_text.clone(),
                    target: decision.target.clone(),
                    start_index: candidate.start_index,
                    end_index: candidate.end_index,
                })
        })
        .collect::<Vec<_>>();

    BuildHotwordCorrectionResponse {
        task_id: request.task_id,
        media_path: request.media_path,
        source_lang: request.source_lang,
        enabled: true,
        hotwords: normalized_hotwords,
        candidates,
        decisions,
        corrections,
        words,
    }
}

pub fn recall_hotword_candidates(
    words: &[WordTokenDto],
    hotwords: &[HotwordEntry],
) -> Vec<HotwordCandidate> {
    let normalized = normalize_hotwords(hotwords);
    let mut candidates = Vec::new();
    let mut seen = HashSet::new();

    for hotword in &normalized {
        if matches!(hotword.lang, HotwordLang::Zh) {
            recall_direct_matches(
                words,
                hotword,
                "alias",
                &hotword.aliases,
                &mut candidates,
                &mut seen,
            );
            recall_chinese_phonetic(words, hotword, &mut candidates, &mut seen);
        } else {
            recall_direct_matches(
                words,
                hotword,
                "word",
                &[hotword.word.clone()],
                &mut candidates,
                &mut seen,
            );
            recall_direct_matches(
                words,
                hotword,
                "alias",
                &hotword.aliases,
                &mut candidates,
                &mut seen,
            );
            recall_direct_matches(
                words,
                hotword,
                "generated_alias",
                &hotword.generated_aliases,
                &mut candidates,
                &mut seen,
            );
        }
    }

    candidates
}

pub fn apply_hotword_corrections(
    words: &[WordTokenDto],
    candidates: &[HotwordCandidate],
    decisions: &[HotwordDecision],
) -> Vec<WordTokenDto> {
    let accepted_ids: HashSet<&str> = decisions
        .iter()
        .filter(|decision| decision.replace && decision.error.is_none())
        .map(|decision| decision.candidate_id.as_str())
        .collect();
    let decision_targets: Vec<(&str, &str)> = decisions
        .iter()
        .map(|decision| (decision.candidate_id.as_str(), decision.target.as_str()))
        .collect();
    let mut accepted: Vec<_> = candidates
        .iter()
        .filter(|candidate| accepted_ids.contains(candidate.id.as_str()))
        .collect();
    accepted.sort_by_key(|candidate| {
        (
            candidate.start_index,
            std::cmp::Reverse(candidate.end_index - candidate.start_index),
        )
    });

    let mut accepted_non_overlapping = Vec::new();
    let mut covered_until = 0;
    for candidate in accepted {
        if candidate.start_index >= covered_until && candidate.end_index < words.len() {
            covered_until = candidate.end_index + 1;
            accepted_non_overlapping.push(candidate);
        }
    }

    let mut out = Vec::new();
    let mut index = 0;
    while index < words.len() {
        if let Some(candidate) = accepted_non_overlapping
            .iter()
            .find(|candidate| candidate.start_index == index)
        {
            let target = decision_targets
                .iter()
                .find(|(id, _)| *id == candidate.id)
                .map(|(_, target)| *target)
                .filter(|target| !target.is_empty())
                .unwrap_or(candidate.target.as_str());
            out.push(WordTokenDto {
                start: words[candidate.start_index].start,
                end: words[candidate.end_index].end,
                word: target.to_string(),
            });
            index = candidate.end_index + 1;
        } else {
            out.push(words[index].clone());
            index += 1;
        }
    }

    out
}

fn normalize_hotwords(hotwords: &[HotwordEntry]) -> Vec<NormalizedHotword> {
    hotwords
        .iter()
        .map(normalize_hotword)
        .filter(|hotword| !hotword.word.is_empty())
        .collect()
}

fn normalize_hotword(entry: &HotwordEntry) -> NormalizedHotword {
    let lang = match entry.lang {
        HotwordLang::Auto => {
            if contains_chinese(&entry.word) {
                HotwordLang::Zh
            } else {
                HotwordLang::NonZh
            }
        }
        ref lang => lang.clone(),
    };
    let aliases: Vec<_> = entry
        .aliases
        .iter()
        .map(|alias| alias.trim().to_string())
        .filter(|alias| !alias.is_empty())
        .collect();
    let (pinyin, first_letters) = if matches!(lang, HotwordLang::Zh) {
        let pinyin = chinese_pinyin(&entry.word);
        let first_letters = chinese_first_letters(&entry.word);
        (pinyin, first_letters)
    } else {
        (String::new(), String::new())
    };
    let generated_aliases = if matches!(lang, HotwordLang::NonZh) {
        generated_aliases(&entry.word)
    } else {
        Vec::new()
    };

    NormalizedHotword {
        word: entry.word.trim().to_string(),
        aliases,
        generated_aliases,
        pinyin,
        first_letters,
        lang,
    }
}

fn recall_direct_matches(
    words: &[WordTokenDto],
    hotword: &NormalizedHotword,
    source_kind: &str,
    sources: &[String],
    candidates: &mut Vec<HotwordCandidate>,
    seen: &mut HashSet<(usize, usize, String)>,
) {
    for source in sources {
        let source_tokens = normalized_tokens(source);
        if source_tokens.is_empty() {
            continue;
        }
        for start in 0..words.len() {
            let end = start + source_tokens.len() - 1;
            if end >= words.len() {
                continue;
            }
            let window = normalized_window_tokens(words, start, end);
            if window == source_tokens {
                push_candidate(words, start, end, hotword, source_kind, candidates, seen);
            }
        }
    }
}

fn recall_chinese_phonetic(
    words: &[WordTokenDto],
    hotword: &NormalizedHotword,
    candidates: &mut Vec<HotwordCandidate>,
    seen: &mut HashSet<(usize, usize, String)>,
) {
    let target_len = hotword.word.chars().count();
    if target_len == 0 {
        return;
    }
    let allow_first_letters = target_len >= 2 && hotword.first_letters.len() >= 2;

    for start in 0..words.len() {
        for end in start..words.len() {
            let text = source_text(words, start, end);
            let compact = text.replace(' ', "");
            if allow_first_letters
                && start == end
                && is_first_letter_token(&words[start].word)
                && compact.eq_ignore_ascii_case(&hotword.first_letters)
            {
                push_candidate(
                    words,
                    start,
                    end,
                    hotword,
                    "first_letters",
                    candidates,
                    seen,
                );
                continue;
            }
            if compact.chars().count() != target_len || !contains_chinese(&compact) {
                continue;
            }
            let pinyin = chinese_pinyin(&compact);
            if !pinyin.is_empty() && pinyin == hotword.pinyin {
                push_candidate(words, start, end, hotword, "pinyin", candidates, seen);
            }
        }
    }
}

fn push_candidate(
    words: &[WordTokenDto],
    start: usize,
    end: usize,
    hotword: &NormalizedHotword,
    source_kind: &str,
    candidates: &mut Vec<HotwordCandidate>,
    seen: &mut HashSet<(usize, usize, String)>,
) {
    let key = (start, end, hotword.word.clone());
    if !seen.insert(key) {
        return;
    }
    let source_text = source_text(words, start, end);
    candidates.push(HotwordCandidate {
        id: format!("hotword:{}:{}:{}:{}", start, end, hotword.word, source_kind),
        start_index: start,
        end_index: end,
        source_text,
        target: hotword.word.clone(),
        source_kind: source_kind.to_string(),
        context: context_text(words, start, end),
    });
}

fn source_text(words: &[WordTokenDto], start: usize, end: usize) -> String {
    let parts: Vec<_> = words[start..=end]
        .iter()
        .map(|word| word.word.as_str())
        .collect();
    if parts.iter().all(|part| contains_chinese(part)) {
        parts.join("")
    } else {
        parts.join(" ")
    }
}

fn context_text(words: &[WordTokenDto], start: usize, end: usize) -> String {
    let context_start = start.saturating_sub(3);
    let context_end = (end + 3).min(words.len().saturating_sub(1));
    source_text(words, context_start, context_end)
}

fn normalized_window_tokens(words: &[WordTokenDto], start: usize, end: usize) -> Vec<String> {
    words[start..=end]
        .iter()
        .flat_map(|word| normalized_tokens(&word.word))
        .collect()
}

fn normalized_tokens(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(normalize_ascii_token)
        .filter(|token| !token.is_empty())
        .collect()
}

fn normalize_ascii_token(text: &str) -> String {
    text.chars()
        .filter(|ch| ch.is_alphanumeric() || contains_chinese_char(*ch))
        .flat_map(char::to_lowercase)
        .collect()
}

fn is_first_letter_token(text: &str) -> bool {
    let trimmed = text.trim();
    (2..=6).contains(&trimmed.len()) && trimmed.chars().all(|ch| ch.is_ascii_alphabetic())
}

fn generated_aliases(word: &str) -> Vec<String> {
    let tokens = normalized_tokens(word);
    let mut variants: Vec<Vec<String>> = Vec::new();
    for token in &tokens {
        let token_variants = match token.as_str() {
            "claude" => vec![
                "claude".to_string(),
                "cloud".to_string(),
                "clod".to_string(),
            ],
            "code" => vec!["code".to_string(), "cod".to_string()],
            _ => vec![token.clone()],
        };
        variants.push(token_variants);
    }

    let exact = tokens.join(" ");
    let mut out = Vec::new();
    build_alias_combinations(&variants, 0, &mut Vec::new(), &exact, &mut out);
    out
}

fn build_alias_combinations(
    variants: &[Vec<String>],
    index: usize,
    current: &mut Vec<String>,
    exact: &str,
    out: &mut Vec<String>,
) {
    if index == variants.len() {
        let alias = current.join(" ");
        if alias != exact && !out.contains(&alias) {
            out.push(alias);
        }
        return;
    }
    for variant in &variants[index] {
        current.push(variant.clone());
        build_alias_combinations(variants, index + 1, current, exact, out);
        current.pop();
    }
}

fn chinese_pinyin(text: &str) -> String {
    text.to_pinyin()
        .filter_map(|pinyin| pinyin.map(|p| p.plain().to_string()))
        .collect::<Vec<_>>()
        .join("")
}

fn chinese_first_letters(text: &str) -> String {
    text.to_pinyin()
        .filter_map(|pinyin| pinyin.map(|p| p.first_letter().to_string()))
        .collect()
}

fn contains_chinese(text: &str) -> bool {
    text.chars().any(contains_chinese_char)
}

fn contains_chinese_char(ch: char) -> bool {
    ('\u{4e00}'..='\u{9fff}').contains(&ch)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(index: usize, text: &str) -> WordTokenDto {
        WordTokenDto {
            start: index as f64,
            end: index as f64 + 0.5,
            word: text.to_string(),
        }
    }

    fn hotword(text: &str, aliases: Vec<&str>, lang: HotwordLang) -> HotwordEntry {
        HotwordEntry {
            word: text.to_string(),
            aliases: aliases.into_iter().map(str::to_string).collect(),
            lang,
            note: None,
        }
    }

    fn candidate(id: &str, start_index: usize, end_index: usize, target: &str) -> HotwordCandidate {
        HotwordCandidate {
            id: id.to_string(),
            start_index,
            end_index,
            source_text: String::new(),
            target: target.to_string(),
            source_kind: "generated_alias".to_string(),
            context: String::new(),
        }
    }

    #[test]
    fn chinese_pinyin_recalls_homophone_without_alias() {
        let words = vec![word(0, "浩书")];
        let hotwords = vec![hotword("浩叔", vec![], HotwordLang::Zh)];

        let candidates = recall_hotword_candidates(&words, &hotwords);

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].target, "浩叔");
        assert_eq!(candidates[0].source_text, "浩书");
        assert_eq!(candidates[0].source_kind, "pinyin");
    }

    #[test]
    fn chinese_first_letters_recalls_short_ascii_abbreviation() {
        let words = vec![word(0, "hs")];
        let hotwords = vec![hotword("浩叔", vec![], HotwordLang::Zh)];

        let candidates = recall_hotword_candidates(&words, &hotwords);

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].target, "浩叔");
        assert_eq!(candidates[0].source_text, "hs");
        assert_eq!(candidates[0].source_kind, "first_letters");
    }

    #[test]
    fn chinese_first_letters_ignores_one_character_hotword() {
        let words = vec![word(0, "a")];
        let hotwords = vec![hotword("爱", vec![], HotwordLang::Zh)];

        let candidates = recall_hotword_candidates(&words, &hotwords);

        assert!(candidates.is_empty());
    }

    #[test]
    fn chinese_first_letters_ignores_multi_token_abbreviation() {
        let words = vec![word(0, "h"), word(1, "s")];
        let hotwords = vec![hotword("浩叔", vec![], HotwordLang::Zh)];

        let candidates = recall_hotword_candidates(&words, &hotwords);

        assert!(candidates.is_empty());
    }

    #[test]
    fn chinese_first_letters_ignores_long_normal_word() {
        let words = vec![word(0, "abcdefg")];
        let hotwords = vec![hotword("爱不才的饿飞个", vec![], HotwordLang::Zh)];

        let candidates = recall_hotword_candidates(&words, &hotwords);

        assert!(candidates.is_empty());
    }

    #[test]
    fn chinese_alias_recalls_direct_match() {
        let words = vec![word(0, "皓叔")];
        let hotwords = vec![hotword("浩叔", vec!["皓叔"], HotwordLang::Zh)];

        let candidates = recall_hotword_candidates(&words, &hotwords);

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].target, "浩叔");
        assert_eq!(candidates[0].source_text, "皓叔");
        assert_eq!(candidates[0].source_kind, "alias");
    }

    #[test]
    fn non_chinese_generated_alias_recalls_when_alias_is_missing() {
        let words = vec![word(0, "cloud"), word(1, "code")];
        let hotwords = vec![hotword("Claude Code", vec![], HotwordLang::NonZh)];

        let candidates = recall_hotword_candidates(&words, &hotwords);

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].target, "Claude Code");
        assert_eq!(candidates[0].source_text, "cloud code");
        assert_eq!(candidates[0].source_kind, "generated_alias");
    }

    #[test]
    fn accepted_multi_token_correction_merges_timing() {
        let words = vec![
            word(0, "open"),
            word(1, "cloud"),
            word(2, "code"),
            word(3, "now"),
        ];
        let hotwords = vec![hotword("Claude Code", vec![], HotwordLang::NonZh)];
        let candidates = recall_hotword_candidates(&words, &hotwords);
        let decisions = vec![HotwordDecision {
            candidate_id: candidates[0].id.clone(),
            replace: true,
            target: "Claude Code".to_string(),
            reason: None,
            error: None,
        }];

        let corrected = apply_hotword_corrections(&words, &candidates, &decisions);

        assert_eq!(corrected.len(), 3);
        assert_eq!(corrected[1].word, "Claude Code");
        assert_eq!(corrected[1].start, words[1].start);
        assert_eq!(corrected[1].end, words[2].end);
        assert_eq!(corrected[2].word, "now");
    }

    #[test]
    fn accepted_overlapping_correction_prefers_longer_span_for_same_start() {
        let words = vec![word(0, "cloud"), word(1, "code"), word(2, "now")];
        let candidates = vec![
            candidate("short", 0, 0, "Cloud"),
            candidate("long", 0, 1, "Claude Code"),
        ];
        let decisions = vec![
            HotwordDecision {
                candidate_id: "short".to_string(),
                replace: true,
                target: "Cloud".to_string(),
                reason: None,
                error: None,
            },
            HotwordDecision {
                candidate_id: "long".to_string(),
                replace: true,
                target: "Claude Code".to_string(),
                reason: None,
                error: None,
            },
        ];

        let corrected = apply_hotword_corrections(&words, &candidates, &decisions);

        assert_eq!(corrected.len(), 2);
        assert_eq!(corrected[0].word, "Claude Code");
        assert_eq!(corrected[0].start, words[0].start);
        assert_eq!(corrected[0].end, words[1].end);
        assert_eq!(corrected[1].word, "now");
    }

    #[test]
    fn disabled_hotwords_pass_through_words() {
        let words = vec![word(0, "cloud"), word(1, "code")];

        let response = build_hotword_correction(BuildHotwordCorrectionRequest {
            task_id: "task-1".to_string(),
            media_path: "media.wav".to_string(),
            source_lang: "en".to_string(),
            words: words.clone(),
            hotwords: Vec::new(),
            enabled: false,
            translate_api_key: String::new(),
            translate_base_url: String::new(),
            translate_model: String::new(),
        });

        assert!(!response.enabled);
        assert_eq!(response.words.len(), words.len());
        assert_eq!(response.words[0].word, words[0].word);
        assert_eq!(response.words[1].word, words[1].word);
        assert!(response.candidates.is_empty());
        assert!(response.decisions.is_empty());
        assert!(response.corrections.is_empty());
    }

    #[test]
    fn llm_unavailable_records_skipped_decision_and_keeps_words() {
        let words = vec![word(0, "cloud"), word(1, "code")];
        let hotwords = vec![hotword("Claude Code", vec![], HotwordLang::NonZh)];

        let response = build_hotword_correction(BuildHotwordCorrectionRequest {
            task_id: "task-1".to_string(),
            media_path: "media.wav".to_string(),
            source_lang: "en".to_string(),
            words: words.clone(),
            hotwords,
            enabled: true,
            translate_api_key: String::new(),
            translate_base_url: String::new(),
            translate_model: String::new(),
        });

        assert!(response.enabled);
        assert_eq!(response.candidates.len(), 1);
        assert_eq!(response.decisions.len(), 1);
        assert_eq!(
            response.decisions[0].candidate_id,
            response.candidates[0].id
        );
        assert!(!response.decisions[0].replace);
        assert_eq!(response.decisions[0].target, "Claude Code");
        assert_eq!(
            response.decisions[0].error.as_deref(),
            Some("llm_unavailable")
        );
        assert_eq!(response.words.len(), words.len());
        assert_eq!(response.words[0].word, words[0].word);
        assert_eq!(response.words[1].word, words[1].word);
        assert!(response.corrections.is_empty());
    }
}
