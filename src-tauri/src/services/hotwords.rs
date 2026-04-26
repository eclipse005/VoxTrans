use crate::services::llm::client::{LlmSemanticValidationError, OpenAiCompatLlmClient};
use crate::services::llm::json_guard::JsonResponseValidator;
use crate::services::llm::port::{LlmCallContext, LlmConfig, next_llm_request_id};
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

#[cfg(test)]
fn build_hotword_correction(
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
    let corrections = build_applied_hotword_corrections(&request.words, &candidates, &decisions);
    let words = apply_hotword_corrections(&request.words, &candidates, &decisions);

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

pub async fn build_hotword_correction_async(
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
    let decisions = if llm_available {
        review_hotword_candidates_with_llm(&request, &candidates).await
    } else {
        unavailable_hotword_decisions(&candidates)
    };
    let corrections = build_applied_hotword_corrections(&request.words, &candidates, &decisions);
    let words = apply_hotword_corrections(&request.words, &candidates, &decisions);

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

async fn review_hotword_candidates_with_llm(
    request: &BuildHotwordCorrectionRequest,
    candidates: &[HotwordCandidate],
) -> Vec<HotwordDecision> {
    let client = match OpenAiCompatLlmClient::new(LlmConfig::new(
        request.translate_base_url.clone(),
        request.translate_api_key.clone(),
        request.translate_model.clone(),
    )) {
        Ok(client) => client,
        Err(err) => {
            return candidates
                .iter()
                .map(|candidate| errored_hotword_decision(candidate, err.message.clone()))
                .collect();
        }
    };
    let context = LlmCallContext {
        task_id: request.task_id.clone(),
        media_path: Some(request.media_path.clone()),
        phase: "step1_5_hotword_decision".to_string(),
    };
    let validator = JsonResponseValidator::with_required_keys(&["replace", "target", "reason"]);
    let mut decisions = Vec::with_capacity(candidates.len());

    for candidate in candidates {
        let prompt = build_hotword_decision_prompt(candidate);
        let llm_id = next_llm_request_id();
        let decision = match client
            .call_json_validated(&context, &llm_id, &prompt, Some(&validator), |value| {
                serde_json::from_value::<RawHotwordDecision>(value)
                    .map_err(|err| {
                        LlmSemanticValidationError::retryable(format!(
                            "hotword decision parse failed: {err}"
                        ))
                    })
                    .map(|raw| hotword_decision_from_raw(candidate, raw))
            })
            .await
        {
            Ok(result) => result.value,
            Err(err) => {
                errored_hotword_decision(candidate, format!("llm_error:{llm_id}:{}", err.message))
            }
        };
        decisions.push(decision);
    }

    decisions
}

fn unavailable_hotword_decisions(candidates: &[HotwordCandidate]) -> Vec<HotwordDecision> {
    candidates
        .iter()
        .map(|candidate| HotwordDecision {
            candidate_id: candidate.id.clone(),
            replace: false,
            target: candidate.target.clone(),
            reason: Some(String::new()),
            error: Some("llm_unavailable".to_string()),
        })
        .collect()
}

fn errored_hotword_decision(candidate: &HotwordCandidate, error: String) -> HotwordDecision {
    HotwordDecision {
        candidate_id: candidate.id.clone(),
        replace: false,
        target: candidate.target.clone(),
        reason: Some(String::new()),
        error: Some(error),
    }
}

#[derive(Debug, Clone, Deserialize)]
struct RawHotwordDecision {
    replace: bool,
    #[serde(default)]
    target: String,
    #[serde(default)]
    reason: String,
}

#[cfg(test)]
fn parse_hotword_decision_json(
    candidate_id: &str,
    fallback_target: &str,
    raw: &str,
) -> HotwordDecision {
    match serde_json::from_str::<RawHotwordDecision>(raw.trim()) {
        Ok(parsed) => HotwordDecision {
            candidate_id: candidate_id.to_string(),
            replace: parsed.replace,
            target: normalize_decision_target(&parsed.target, fallback_target),
            reason: Some(parsed.reason.trim().to_string()),
            error: None,
        },
        Err(_) => HotwordDecision {
            candidate_id: candidate_id.to_string(),
            replace: false,
            target: fallback_target.to_string(),
            reason: Some(String::new()),
            error: Some("invalid_json".to_string()),
        },
    }
}

fn hotword_decision_from_raw(
    candidate: &HotwordCandidate,
    raw: RawHotwordDecision,
) -> HotwordDecision {
    HotwordDecision {
        candidate_id: candidate.id.clone(),
        replace: raw.replace,
        target: normalize_decision_target(&raw.target, &candidate.target),
        reason: Some(raw.reason.trim().to_string()),
        error: None,
    }
}

fn normalize_decision_target(raw: &str, fallback: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        trimmed.to_string()
    }
}

fn build_hotword_decision_prompt(candidate: &HotwordCandidate) -> String {
    format!(
        concat!(
            "You are judging whether one ASR phrase should be replaced by a configured hotword.\n",
            "Only decide this candidate. Do not edit grammar, punctuation, style, or surrounding text.\n",
            "Return only JSON: {{\"replace\": boolean, \"target\": string, \"reason\": string}}\n\n",
            "Context: {context}\n",
            "Candidate source text: {source_text}\n",
            "Target hotword: {target}\n",
            "Recall type: {source_kind}\n"
        ),
        context = candidate.context,
        source_text = candidate.source_text,
        target = candidate.target,
        source_kind = candidate.source_kind,
    )
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
            recall_non_chinese_fuzzy(words, hotword, &mut candidates, &mut seen);
        }
    }

    candidates
}

pub fn apply_hotword_corrections(
    words: &[WordTokenDto],
    candidates: &[HotwordCandidate],
    decisions: &[HotwordDecision],
) -> Vec<WordTokenDto> {
    let applied = select_applied_hotword_corrections(words, candidates, decisions);
    let mut out = Vec::new();
    let mut index = 0;
    while index < words.len() {
        if let Some(correction) = applied
            .iter()
            .find(|correction| correction.candidate.start_index == index)
        {
            let target = replacement_target_with_boundary_punctuation(
                &words[correction.candidate.start_index..=correction.candidate.end_index],
                correction.target,
            );
            out.push(WordTokenDto {
                start: words[correction.candidate.start_index].start,
                end: words[correction.candidate.end_index].end,
                word: target,
            });
            index = correction.candidate.end_index + 1;
        } else {
            out.push(words[index].clone());
            index += 1;
        }
    }

    out
}

fn replacement_target_with_boundary_punctuation(words: &[WordTokenDto], target: &str) -> String {
    let trimmed_target = target.trim();
    if words.is_empty() || trimmed_target.is_empty() {
        return trimmed_target.to_string();
    }
    let prefix = leading_boundary_punctuation(&words[0].word);
    let suffix = trailing_boundary_punctuation(&words[words.len() - 1].word);
    let mut out = String::new();
    if !trimmed_target.starts_with(prefix) {
        out.push_str(prefix);
    }
    out.push_str(trimmed_target);
    if !trimmed_target.ends_with(suffix) {
        out.push_str(suffix);
    }
    out
}

fn leading_boundary_punctuation(text: &str) -> &str {
    let end = text
        .char_indices()
        .find(|(_, ch)| !is_boundary_punctuation(*ch))
        .map(|(index, _)| index)
        .unwrap_or(text.len());
    &text[..end]
}

fn trailing_boundary_punctuation(text: &str) -> &str {
    let start = text
        .char_indices()
        .rev()
        .find(|(_, ch)| !is_boundary_punctuation(*ch))
        .map(|(index, ch)| index + ch.len_utf8())
        .unwrap_or(0);
    &text[start..]
}

fn build_applied_hotword_corrections(
    words: &[WordTokenDto],
    candidates: &[HotwordCandidate],
    decisions: &[HotwordDecision],
) -> Vec<HotwordCorrection> {
    select_applied_hotword_corrections(words, candidates, decisions)
        .iter()
        .map(|correction| HotwordCorrection {
            candidate_id: correction.candidate.id.clone(),
            source_text: correction.candidate.source_text.clone(),
            target: correction.target.to_string(),
            start_index: correction.candidate.start_index,
            end_index: correction.candidate.end_index,
        })
        .collect()
}

struct AppliedHotwordCorrection<'a> {
    candidate: &'a HotwordCandidate,
    target: &'a str,
}

fn select_applied_hotword_corrections<'a>(
    words: &[WordTokenDto],
    candidates: &'a [HotwordCandidate],
    decisions: &'a [HotwordDecision],
) -> Vec<AppliedHotwordCorrection<'a>> {
    let accepted_ids: HashSet<&str> = decisions
        .iter()
        .filter(|decision| decision.replace && decision.error.is_none())
        .map(|decision| decision.candidate_id.as_str())
        .collect();
    let decision_targets: Vec<(&str, &str)> = decisions
        .iter()
        .filter(|decision| decision.replace && decision.error.is_none())
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
            let target = decision_targets
                .iter()
                .find(|(id, _)| *id == candidate.id)
                .map(|(_, target)| *target)
                .filter(|target| !target.is_empty())
                .unwrap_or(candidate.target.as_str());
            accepted_non_overlapping.push(AppliedHotwordCorrection { candidate, target });
        }
    }

    accepted_non_overlapping
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
            if window == source_tokens && !tokens_equal_hotword_target(&window, hotword) {
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
            if compact == hotword.word {
                continue;
            }
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

fn recall_non_chinese_fuzzy(
    words: &[WordTokenDto],
    hotword: &NormalizedHotword,
    candidates: &mut Vec<HotwordCandidate>,
    seen: &mut HashSet<(usize, usize, String)>,
) {
    let target_tokens = normalized_tokens(&hotword.word);
    if target_tokens.is_empty() {
        return;
    }
    let target_collapsed = normalized_collapsed_token(&hotword.word);
    if target_collapsed.is_empty() {
        return;
    }
    let target_collapsed_len = target_collapsed.len();
    let min_collapsed_len = if target_tokens.len() == 1 {
        target_collapsed_len
    } else {
        target_collapsed_len.saturating_sub(1)
    };
    let max_span_tokens = if target_tokens.len() == 1 {
        (target_collapsed_len + 1).min(8).max(1)
    } else {
        target_tokens.len().min(8) + 1
    };
    let target_is_short_acronym = is_short_acronym_hotword(&hotword.word);

    for start in 0..words.len() {
        for token_len in 1..=max_span_tokens {
            let end = start + token_len - 1;
            if end >= words.len() {
                continue;
            }
            if target_is_short_acronym && !source_window_looks_like_acronym(words, start, end) {
                continue;
            }
            let window = normalized_window_tokens(words, start, end);
            let collapsed_window = normalized_collapsed_window_tokens(words, start, end);
            if target_is_short_acronym {
                if window == target_tokens {
                    continue;
                }
                if collapsed_window.len() != target_collapsed_len {
                    continue;
                }
                if is_short_acronym_window_match(&collapsed_window, &target_collapsed) {
                    push_candidate(words, start, end, hotword, "fuzzy", candidates, seen);
                }
                continue;
            }
            if collapsed_window.len() < min_collapsed_len
                || collapsed_window.len() > target_collapsed_len + 1
            {
                continue;
            }
            if fuzzy_tokens_match(
                &window,
                &target_tokens,
                &target_collapsed,
                collapsed_window.len(),
            ) || fuzzy_collapsed_match(
                &collapsed_window,
                &target_collapsed,
                target_tokens.len() == 1 && window.len() > 1,
                target_collapsed_len,
            ) {
                push_candidate(words, start, end, hotword, "fuzzy", candidates, seen);
            }
        }
    }
}

fn fuzzy_tokens_match(
    source_tokens: &[String],
    target_tokens: &[String],
    target_collapsed: &str,
    source_collapsed_len: usize,
) -> bool {
    if source_tokens == target_tokens
        || source_tokens.is_empty()
        || target_tokens.is_empty()
        || source_tokens.len() != target_tokens.len()
    {
        return false;
    }
    if target_collapsed.len() < 4 {
        return false;
    }
    if source_tokens
        .iter()
        .zip(target_tokens.iter())
        .all(|(source, target)| fuzzy_ascii_token_match(source, target))
    {
        return true;
    }

    // Secondary fallback: keep one short tokenization tolerant if combined string is near target.
    if source_tokens.len() <= 1 || source_collapsed_len < 4 {
        return false;
    }
    let source_collapsed = source_tokens.join("");
    if source_collapsed.len().abs_diff(target_collapsed.len()) > 1 {
        return false;
    }
    levenshtein_distance_at_most(&source_collapsed, target_collapsed, 1)
}

fn fuzzy_collapsed_match(
    source_collapsed: &str,
    target_collapsed: &str,
    allow_exact_spanning_mismatch: bool,
    target_collapsed_len: usize,
) -> bool {
    if source_collapsed.is_empty() || target_collapsed_len < 4 {
        return false;
    }
    if !allow_exact_spanning_mismatch && source_collapsed.len() == target_collapsed.len() {
        return false;
    }
    let length_delta = source_collapsed.len().abs_diff(target_collapsed_len);
    if length_delta == 0 {
        source_collapsed == target_collapsed
    } else if length_delta <= 1 {
        levenshtein_distance_at_most(source_collapsed, target_collapsed, 1)
    } else {
        false
    }
}

fn fuzzy_ascii_token_match(source: &str, target: &str) -> bool {
    if source.len() < 4 || target.len() < 4 {
        return false;
    }
    if source == target {
        return true;
    }
    if !source.is_ascii() || !target.is_ascii() {
        return false;
    }
    let length_delta = source.len().abs_diff(target.len());
    if length_delta > 2 {
        return false;
    }
    levenshtein_distance_at_most(source, target, 2)
}

fn normalized_collapsed_window_tokens(words: &[WordTokenDto], start: usize, end: usize) -> String {
    let mut out = String::new();
    for word in &words[start..=end] {
        out.push_str(&normalize_ascii_token(&word.word));
    }
    out
}

fn normalized_collapsed_token(text: &str) -> String {
    normalize_ascii_token(text)
}

fn is_short_acronym_hotword(text: &str) -> bool {
    let normalized = normalize_ascii_token(text);
    let has_alpha = normalized.chars().any(|ch| ch.is_ascii_alphabetic());
    let has_digit = normalized.chars().any(|ch| ch.is_ascii_digit());
    ((3..=6).contains(&normalized.len()) || (normalized.len() == 2 && has_alpha && has_digit))
        && has_alpha
        && normalized.chars().all(|ch| ch.is_ascii_alphanumeric())
}

fn source_window_looks_like_acronym(words: &[WordTokenDto], start: usize, end: usize) -> bool {
    if start > end || end >= words.len() {
        return false;
    }
    words[start..=end]
        .iter()
        .all(|word| token_looks_like_acronym_piece(&word.word))
}

fn token_looks_like_acronym_piece(text: &str) -> bool {
    let trimmed = trim_boundary_punctuation(text);
    if trimmed.is_empty() {
        return false;
    }
    let mut has_alpha = false;
    let mut has_alnum = false;
    for ch in trimmed.chars() {
        if ch.is_ascii_alphabetic() {
            has_alpha = true;
            has_alnum = true;
            continue;
        }
        if ch.is_ascii_digit() {
            has_alnum = true;
            continue;
        }
        return false;
    }
    let token_len = trimmed.chars().count();
    if !has_alnum || (!has_alpha && token_len > 1) {
        return false;
    }
    token_len <= 3 || trimmed == trimmed.to_ascii_uppercase()
}

fn is_short_acronym_window_match(source_collapsed: &str, target_collapsed: &str) -> bool {
    if source_collapsed.is_empty() || target_collapsed.is_empty() {
        return false;
    }
    if source_collapsed.len().abs_diff(target_collapsed.len()) > 1 {
        return false;
    }
    if source_collapsed.eq_ignore_ascii_case(target_collapsed) {
        return true;
    }
    if source_collapsed.len() < 2 || target_collapsed.len() < 2 {
        return false;
    }
    let source_lower = source_collapsed.to_ascii_lowercase();
    let target_lower = target_collapsed.to_ascii_lowercase();
    levenshtein_distance_at_most(&source_lower, &target_lower, 2)
}

fn levenshtein_distance_at_most(left: &str, right: &str, max_distance: usize) -> bool {
    let left_chars = left.chars().collect::<Vec<_>>();
    let right_chars = right.chars().collect::<Vec<_>>();
    if left_chars.len().abs_diff(right_chars.len()) > max_distance {
        return false;
    }

    let mut previous = (0..=right_chars.len()).collect::<Vec<_>>();
    let mut current = vec![0usize; right_chars.len() + 1];
    for (left_index, left_ch) in left_chars.iter().enumerate() {
        current[0] = left_index + 1;
        let mut row_min = current[0];
        for (right_index, right_ch) in right_chars.iter().enumerate() {
            let cost = usize::from(left_ch != right_ch);
            current[right_index + 1] = (previous[right_index + 1] + 1)
                .min(current[right_index] + 1)
                .min(previous[right_index] + cost);
            row_min = row_min.min(current[right_index + 1]);
        }
        if row_min > max_distance {
            return false;
        }
        std::mem::swap(&mut previous, &mut current);
    }

    previous[right_chars.len()] <= max_distance
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
    let source_text = candidate_source_text(words, start, end);
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

fn candidate_source_text(words: &[WordTokenDto], start: usize, end: usize) -> String {
    trim_boundary_punctuation(&source_text(words, start, end))
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

fn tokens_equal_hotword_target(tokens: &[String], hotword: &NormalizedHotword) -> bool {
    tokens == normalized_tokens(&hotword.word)
}

fn trim_boundary_punctuation(text: &str) -> String {
    text.trim_matches(is_boundary_punctuation).to_string()
}

fn is_boundary_punctuation(ch: char) -> bool {
    !ch.is_alphanumeric() && !contains_chinese_char(ch)
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
        let token_variants = token_fuzzy_variants(token);
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

fn token_fuzzy_variants(token: &str) -> Vec<String> {
    let normalized = token.to_string();
    let mut variants = vec![normalized.clone()];
    if normalized.len() < 4 {
        return dedupe_strings(variants);
    }

    let chars: Vec<char> = normalized.chars().collect();
    for i in 0..chars.len() {
        let mut deleted = chars.clone();
        deleted.remove(i);
        if deleted.len() >= 4 {
            variants.push(deleted.iter().collect::<String>());
        }
    }
    for i in 0..chars.len() - 1 {
        let mut swapped = chars.clone();
        swapped.swap(i, i + 1);
        variants.push(swapped.iter().collect::<String>());
    }
    dedupe_strings(variants)
}

fn dedupe_strings(values: Vec<String>) -> Vec<String> {
    let mut out = Vec::<String>::new();
    for value in values {
        if !out.contains(&value) {
            out.push(value);
        }
    }
    out
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
    fn alias_candidate_source_text_omits_boundary_punctuation() {
        let words = vec![word(0, "SysD?")];
        let hotwords = vec![hotword("CISD", vec!["SysD"], HotwordLang::NonZh)];

        let candidates = recall_hotword_candidates(&words, &hotwords);

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].source_text, "SysD");
    }

    #[test]
    fn non_chinese_generated_alias_recalls_when_alias_is_missing() {
        let words = vec![word(0, "cloud"), word(1, "code")];
        let hotwords = vec![hotword("Claude Code", vec![], HotwordLang::NonZh)];

        let candidates = recall_hotword_candidates(&words, &hotwords);

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].target, "Claude Code");
        assert_eq!(candidates[0].source_text, "cloud code");
        assert_eq!(candidates[0].source_kind, "fuzzy");
    }

    #[test]
    fn exact_non_chinese_hotword_does_not_recall_candidate() {
        let words = vec![word(0, "CISD")];
        let hotwords = vec![hotword("CISD", vec![], HotwordLang::NonZh)];

        let candidates = recall_hotword_candidates(&words, &hotwords);

        assert!(candidates.is_empty());
    }

    #[test]
    fn non_chinese_fuzzy_recalls_single_edit_acronym() {
        let words = vec![word(0, "SISD")];
        let hotwords = vec![hotword("CISD", vec![], HotwordLang::NonZh)];

        let candidates = recall_hotword_candidates(&words, &hotwords);

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].source_text, "SISD");
        assert_eq!(candidates[0].target, "CISD");
        assert_eq!(candidates[0].source_kind, "fuzzy");
    }

    #[test]
    fn non_chinese_fuzzy_skips_plain_words_for_short_acronym_hotword() {
        let words = vec![
            word(0, "case"),
            word(1, "kind"),
            word(2, "mind"),
            word(3, "SISD"),
            word(4, "SIST"),
        ];
        let hotwords = vec![hotword("CISD", vec![], HotwordLang::NonZh)];

        let candidates = recall_hotword_candidates(&words, &hotwords);
        let sources = candidates
            .iter()
            .map(|candidate| candidate.source_text.as_str())
            .collect::<Vec<_>>();

        assert_eq!(sources, vec!["SISD", "SIST"]);
    }

    #[test]
    fn non_chinese_fuzzy_recalls_two_edit_candidate() {
        let words = vec![word(0, "CISY")];
        let hotwords = vec![hotword("SIST", vec![], HotwordLang::NonZh)];

        let candidates = recall_hotword_candidates(&words, &hotwords);

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].source_text, "CISY");
        assert_eq!(candidates[0].target, "SIST");
        assert_eq!(candidates[0].source_kind, "fuzzy");
    }

    #[test]
    fn non_chinese_fuzzy_recalls_split_acronym_tokens() {
        let words = vec![word(0, "C"), word(1, "I"), word(2, "S"), word(3, "D")];
        let hotwords = vec![hotword("CISD", vec![], HotwordLang::NonZh)];

        let candidates = recall_hotword_candidates(&words, &hotwords);

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].source_text, "C I S D");
        assert_eq!(candidates[0].target, "CISD");
        assert_eq!(candidates[0].source_kind, "fuzzy");
    }

    #[test]
    fn non_chinese_fuzzy_recalls_split_acronym_tokens_lowercase() {
        let words = vec![word(0, "f"), word(1, "v"), word(2, "g")];
        let hotwords = vec![hotword("FVG", vec![], HotwordLang::NonZh)];

        let candidates = recall_hotword_candidates(&words, &hotwords);

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].source_text, "f v g");
        assert_eq!(candidates[0].target, "FVG");
        assert_eq!(candidates[0].source_kind, "fuzzy");
    }

    #[test]
    fn non_chinese_fuzzy_recalls_split_number_letter_acronym() {
        let words = vec![word(0, "3"), word(1, "r")];
        let hotwords = vec![hotword("3R", vec![], HotwordLang::NonZh)];

        let candidates = recall_hotword_candidates(&words, &hotwords);

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].source_text, "3 r");
        assert_eq!(candidates[0].target, "3R");
        assert_eq!(candidates[0].source_kind, "fuzzy");
    }

    #[test]
    fn non_chinese_fuzzy_skips_short_hotwords() {
        let words = vec![word(0, "BI")];
        let hotwords = vec![hotword("AI", vec![], HotwordLang::NonZh)];

        let candidates = recall_hotword_candidates(&words, &hotwords);

        assert!(candidates.is_empty());
    }

    #[test]
    fn exact_chinese_hotword_does_not_recall_candidate() {
        let words = vec![word(0, "浩叔")];
        let hotwords = vec![hotword("浩叔", vec![], HotwordLang::Zh)];

        let candidates = recall_hotword_candidates(&words, &hotwords);

        assert!(candidates.is_empty());
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
    fn accepted_single_token_correction_preserves_boundary_punctuation() {
        let words = vec![word(0, "SysD?")];
        let candidates = vec![candidate("sysd", 0, 0, "CISD")];
        let decisions = vec![HotwordDecision {
            candidate_id: "sysd".to_string(),
            replace: true,
            target: "CISD".to_string(),
            reason: None,
            error: None,
        }];

        let corrected = apply_hotword_corrections(&words, &candidates, &decisions);

        assert_eq!(corrected.len(), 1);
        assert_eq!(corrected[0].word, "CISD?");
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
    fn correction_records_match_applied_non_overlapping_corrections() {
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
        let corrections = build_applied_hotword_corrections(&words, &candidates, &decisions);

        assert_eq!(corrected.len(), 2);
        assert_eq!(corrected[0].word, "Claude Code");
        assert_eq!(corrections.len(), 1);
        assert_eq!(corrections[0].candidate_id, "long");
        assert_eq!(corrections[0].target, corrected[0].word);
        assert_eq!(corrections[0].start_index, 0);
        assert_eq!(corrections[0].end_index, 1);
    }

    #[test]
    fn errored_replace_decisions_do_not_create_correction_records() {
        let words = vec![word(0, "cloud"), word(1, "code")];
        let candidates = vec![candidate("long", 0, 1, "Claude Code")];
        let decisions = vec![HotwordDecision {
            candidate_id: "long".to_string(),
            replace: true,
            target: "Claude Code".to_string(),
            reason: None,
            error: Some("llm_unavailable".to_string()),
        }];

        let corrected = apply_hotword_corrections(&words, &candidates, &decisions);
        let corrections = build_applied_hotword_corrections(&words, &candidates, &decisions);

        assert_eq!(corrected.len(), words.len());
        assert_eq!(corrected[0].word, words[0].word);
        assert!(corrections.is_empty());
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

    #[test]
    fn parse_hotword_decision_accepts_strict_json() {
        let decision = parse_hotword_decision_json(
            "c1",
            "Claude Code",
            r#"{"replace":true,"target":"Claude Code","reason":"product name"}"#,
        );

        assert!(decision.replace);
        assert_eq!(decision.candidate_id, "c1");
        assert_eq!(decision.target, "Claude Code");
        assert_eq!(decision.reason.as_deref(), Some("product name"));
        assert!(decision.error.is_none());
    }

    #[test]
    fn parse_hotword_decision_rejects_invalid_json() {
        let decision = parse_hotword_decision_json("c1", "Claude Code", "yes");

        assert!(!decision.replace);
        assert_eq!(decision.target, "Claude Code");
        assert_eq!(decision.error.as_deref(), Some("invalid_json"));
    }

    #[test]
    fn parse_hotword_decision_uses_fallback_target_when_empty() {
        let decision = parse_hotword_decision_json(
            "c1",
            "Claude Code",
            r#"{"replace":true,"target":"","reason":"target omitted"}"#,
        );

        assert!(decision.replace);
        assert_eq!(decision.target, "Claude Code");
        assert!(decision.error.is_none());
    }
}
