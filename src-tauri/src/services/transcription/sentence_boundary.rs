use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::services::llm::batch::run_indexed_concurrent;
use crate::services::llm::client::{LlmSemanticValidationError, OpenAiCompatLlmClient};
use crate::services::llm::json_guard::JsonResponseValidator;
use crate::services::llm::port::{LlmCallContext, LlmConfig, next_llm_request_id};
use crate::services::transcribe::WordTokenDto;
use voxtrans_core::subtitle::beautify::beautify_words_for_subtitle;
use voxtrans_core::subtitle::segmenter::WordToken;
use voxtrans_core::subtitle::srt::{SrtCue, to_srt_from_cues};

const HARD_SPLIT_GAP_MS: u64 = 2_000;
const ATOM_MAX_WORDS: usize = 1;
const CJK_ATOM_MAX_TOKENS: usize = 12;
const CJK_ATOM_MIN_TOKENS: usize = 3;
const CJK_ATOM_MAX_DURATION_MS: u64 = 2_600;
const CJK_SOFT_SPLIT_GAP_MS: u64 = 600;
const DP_MAX_ATOMS_PER_SENTENCE: usize = 40;
const LLM_WINDOW_ATOMS: usize = 40;
const LLM_WINDOW_STEP: usize = 20;
const HIGH_CONF_PUNCT_SPLIT_MIN_WORDS: usize = 1;
const BOUNDARY_CONTEXT_RADIUS: usize = 8;
const UNKNOWN_BOUNDARY_CONFIDENCE: f64 = 0.5;
const LLM_ERROR_FALLBACK_CONFIDENCE: f64 = 0.2;
const LARGE_PENALTY: f64 = 1_000_000.0;
const REFINE_SHORT_SENTENCE_MAX_TOKENS: usize = 4;
const REFINE_LONG_SENTENCE_MIN_TOKENS: usize = 20;
const MAX_REFINE_ROUNDS: usize = 2;
const LONG_SENTENCE_WINDOW_CHUNKS: usize = 12;
const LONG_SENTENCE_WINDOW_STEP: usize = 6;
const SHORT_SENTENCE_REVIEW_MIN_TOKENS: usize = 5;
const SHORT_SENTENCE_REVIEW_MAX_TOKENS: usize = 10;
const FOCUS_WINDOW_MIN_ATOMS: usize = 8;
const FOCUS_REVIEW_CONFIDENCE_THRESHOLD: f64 = 0.78;
const SEMANTIC_GROUP_CORE_MAX_WORDS: usize = 256;
const SEMANTIC_GROUP_CORE_MAX_CHARS: usize = 340;
const SEMANTIC_GROUP_CONTEXT_MAX_WORDS: usize = 128;
const SEMANTIC_GROUP_CONTEXT_MAX_CHARS: usize = 140;
const LLM_RESTORE_MIN_RUN_WORDS: usize = 24;
const LLM_RESTORE_MIN_RUN_DURATION_MS: u64 = 4_500;
const LONG_SPAN_RESTORE_MIN_DURATION_MS: u64 = 14_000;
const LONG_SPAN_RESTORE_MIN_WORDS: usize = 70;
const LONG_SPAN_RESTORE_CORE_MAX_WORDS: usize = 120;
const LONG_SPAN_RESTORE_CORE_MAX_CHARS: usize = 240;
const LONG_SPAN_RESTORE_CONTEXT_MAX_WORDS: usize = 48;
const LONG_SPAN_RESTORE_CONTEXT_MAX_CHARS: usize = 96;
const LONG_SPAN_TARGET_SENTENCE_DURATION_MS: u64 = 9_000;
const LONG_SPAN_DENSE_CORE_MAX_WORDS: usize = 80;
const LONG_SPAN_DENSE_CORE_MAX_CHARS: usize = 140;
const LONG_SPAN_DENSE_CONTEXT_MAX_WORDS: usize = 32;
const LONG_SPAN_DENSE_CONTEXT_MAX_CHARS: usize = 64;

#[derive(Debug, Clone)]
pub struct SentenceBoundaryRequest {
    pub task_id: String,
    pub media_path: String,
    pub source_lang: String,
    pub words: Vec<WordTokenDto>,
    pub translate_api_key: String,
    pub translate_base_url: String,
    pub translate_model: String,
    pub llm_concurrency: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceSentenceStep2 {
    pub task_id: String,
    pub media_path: String,
    pub source_lang: String,
    pub hard_split_gap_ms: u64,
    pub micro_chunk_total: usize,
    pub boundary_total: usize,
    pub sentence_total: usize,
    pub micro_chunks: Vec<MicroChunk>,
    pub boundaries: Vec<BoundaryDecision>,
    pub translation_sentences: Vec<SourceSentence>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MicroChunk {
    pub chunk_id: usize,
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
    pub word_start: usize,
    pub word_end: usize,
    pub gap_before_ms: u64,
    pub gap_after_ms: u64,
    pub hard_split_before: bool,
    pub hard_split_after: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BoundaryDecision {
    pub left_chunk_id: usize,
    pub right_chunk_id: usize,
    pub gap_ms: u64,
    pub rule_decision: BoundaryDecisionKind,
    pub llm_decision: BoundaryDecisionKind,
    pub final_decision: BoundaryDecisionKind,
    pub confidence: f64,
    pub reason_tag: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceSentence {
    pub sentence_id: usize,
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
    pub word_start: usize,
    pub word_end: usize,
    pub chunk_start: usize,
    pub chunk_end: usize,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BoundaryDecisionKind {
    HardSplit,
    Split,
    Merge,
    Unsure,
    Unknown,
}

#[derive(Debug, Clone)]
struct BoundaryWindowTask {
    region_index: usize,
    window_index: usize,
    chunk_start_index: usize,
    chunks: Vec<MicroChunk>,
    source_lang: String,
    mode: BoundaryWindowMode,
    focus_boundary_index: Option<usize>,
}

#[derive(Debug, Clone)]
struct SentenceRefineTask {
    region_index: usize,
    chunk_start_index: usize,
    chunk_end_index: usize,
    chunks: Vec<MicroChunk>,
    draft_sentences: Vec<SourceSentence>,
    focus_sentence_ids: Vec<usize>,
    source_lang: String,
    short_review: bool,
}

#[derive(Debug, Clone)]
struct LongSentenceSplitTask {
    sentence_index: usize,
    sentence: SourceSentence,
    chunks: Vec<MicroChunk>,
    source_lang: String,
}

#[derive(Debug, Clone)]
struct BoundaryPreference {
    left_chunk_id: usize,
    right_chunk_id: usize,
    gap_ms: u64,
    rule_decision: BoundaryDecisionKind,
    llm_decision: BoundaryDecisionKind,
    confidence: f64,
    reason_tag: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct RestoredSentenceTextExtraction {
    restored_text: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct SplitSentenceArrayExtraction {
    sentences: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct RestoredTokenArrayExtraction {
    tokens: Vec<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum RestoredPunctuation {
    None,
    Period,
    Question,
    Exclamation,
}

#[derive(Debug, Clone)]
struct BoundaryVote {
    boundary_index: usize,
    decision: BoundaryDecisionKind,
    confidence: f64,
    punctuation: RestoredPunctuation,
    weight: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BoundaryWindowMode {
    Sweep,
    Focus,
}

impl BoundaryWindowMode {
    fn phase_label(self) -> &'static str {
        match self {
            BoundaryWindowMode::Sweep => "sweep",
            BoundaryWindowMode::Focus => "focus",
        }
    }
}

#[derive(Debug, Clone)]
struct BoundaryVoteSummary {
    decision: BoundaryDecisionKind,
    confidence: f64,
    reason_tag: String,
    split_score: f64,
    merge_score: f64,
    unsure_score: f64,
    total_weight: f64,
}

#[derive(Debug, Clone, Copy)]
struct SemanticGroupWindow {
    core_start: usize,
    core_end: usize,
    prompt_start: usize,
    prompt_end: usize,
}

#[derive(Debug, Clone)]
struct RestoreEndPuncTask {
    phase: String,
    window: SemanticGroupWindow,
    prompt: String,
    prompt_words: Vec<WordTokenDto>,
    parse_error_prefix: String,
}

#[derive(Debug, Clone, Copy)]
struct LongSpanRecoveryTask {
    span_index: usize,
    span_start: usize,
    span_end: usize,
}

pub async fn build_source_sentences_from_words_with_progress(
    request: SentenceBoundaryRequest,
    on_progress: Option<Arc<dyn Fn(usize, usize) + Send + Sync>>,
) -> Result<SourceSentenceStep2, String> {
    if request.words.is_empty() {
        return Err("words is empty".to_string());
    }
    let total = 4usize;
    if let Some(callback) = on_progress.as_ref() {
        callback(0, total);
    }

    let normalized_words = from_core_words(beautify_words_for_subtitle(to_core_words(
        request.words.clone(),
    )));
    if let Some(callback) = on_progress.as_ref() {
        callback(1, total);
    }
    let micro_chunks = build_micro_chunks_for_source_lang(&normalized_words, false);
    if micro_chunks.is_empty() {
        return Err("failed to build micro chunks".to_string());
    }
    if let Some(callback) = on_progress.as_ref() {
        callback(2, total);
    }

    let llm_client = OpenAiCompatLlmClient::new(LlmConfig::new(
        request.translate_base_url.clone(),
        request.translate_api_key.clone(),
        request.translate_model.clone(),
    ))
    .map_err(|err| err.message)?;

    let semantic_spans =
        build_semantic_sentence_spans_with_llm(&request, &normalized_words, &llm_client).await?;
    if semantic_spans.is_empty() {
        return Err("sentence boundary recovery returned empty spans".to_string());
    }
    if let Some(callback) = on_progress.as_ref() {
        callback(3, total);
    }
    let forced_spans = enforce_hard_pause_spans(&normalized_words, &semantic_spans);
    let translation_sentences = build_sentences_from_word_spans(&normalized_words, &forced_spans);
    let boundaries = build_boundaries_from_spans(&micro_chunks, &forced_spans);
    if let Some(callback) = on_progress.as_ref() {
        callback(4, total);
    }

    Ok(SourceSentenceStep2 {
        task_id: request.task_id,
        media_path: request.media_path,
        source_lang: request.source_lang,
        hard_split_gap_ms: HARD_SPLIT_GAP_MS,
        micro_chunk_total: micro_chunks.len(),
        boundary_total: boundaries.len(),
        sentence_total: translation_sentences.len(),
        micro_chunks,
        boundaries,
        translation_sentences,
    })
}

pub fn source_sentences_to_srt(step2: &SourceSentenceStep2) -> String {
    let cues = step2
        .translation_sentences
        .iter()
        .map(|sentence| SrtCue {
            index: sentence.sentence_id,
            start_ms: sentence.start_ms,
            end_ms: sentence.end_ms,
            text: sentence.text.clone(),
        })
        .collect::<Vec<_>>();
    to_srt_from_cues(&cues)
}

async fn build_semantic_sentence_spans_with_llm(
    request: &SentenceBoundaryRequest,
    words: &[WordTokenDto],
    llm_client: &OpenAiCompatLlmClient,
) -> Result<Vec<(usize, usize)>, String> {
    if words.is_empty() {
        return Ok(Vec::new());
    }

    let mut boundary_ends = collect_existing_terminal_boundary_ends(words);
    let runs = build_no_terminal_runs(words);
    let mut restore_tasks = Vec::<RestoreEndPuncTask>::new();

    for (run_index, (run_start, run_end)) in runs.into_iter().enumerate() {
        let run_words = run_end.saturating_sub(run_start) + 1;
        let run_duration_ms = gap_ms(words[run_start].start, words[run_end].end);
        if run_words < LLM_RESTORE_MIN_RUN_WORDS
            || run_duration_ms < LLM_RESTORE_MIN_RUN_DURATION_MS
        {
            continue;
        }
        let windows = build_semantic_group_windows_for_region(words, run_start, run_end);
        for (window_index, window) in windows.into_iter().enumerate() {
            let prompt_words = words[window.prompt_start..=window.prompt_end].to_vec();
            restore_tasks.push(RestoreEndPuncTask {
                phase: format!(
                    "restore_end_punc_run_{}_window_{}",
                    run_index + 1,
                    window_index + 1
                ),
                window,
                prompt: build_end_punctuation_restore_prompt(&request.source_lang, &prompt_words),
                prompt_words,
                parse_error_prefix: "restore end punctuation parse failed".to_string(),
            });
        }
    }
    boundary_ends.extend(
        collect_restore_end_punc_boundary_ends(request, llm_client, restore_tasks).await,
    );

    let mut spans = build_spans_from_boundary_ends(words.len(), &boundary_ends);
    if spans.is_empty() {
        return Err("failed to recover sentence boundaries".to_string());
    }

    for _ in 0..2 {
        let long_span_boundary_ends =
            collect_long_span_recovery_boundary_ends(request, words, &spans, llm_client).await;
        if long_span_boundary_ends.is_empty() {
            break;
        }
        let before = spans.clone();
        boundary_ends.extend(long_span_boundary_ends);
        spans = build_spans_from_boundary_ends(words.len(), &boundary_ends);
        if spans == before {
            break;
        }
    }

    Ok(spans)
}

async fn collect_long_span_recovery_boundary_ends(
    request: &SentenceBoundaryRequest,
    words: &[WordTokenDto],
    spans: &[(usize, usize)],
    llm_client: &OpenAiCompatLlmClient,
) -> Vec<usize> {
    let mut out = Vec::<usize>::new();
    if words.is_empty() || spans.is_empty() {
        return out;
    }

    let recovery_tasks = spans
        .iter()
        .copied()
        .enumerate()
        .filter_map(|(span_index, (span_start, span_end))| {
            if span_start >= words.len() || span_end >= words.len() || span_start >= span_end {
                return None;
            }
            let word_total = span_end - span_start + 1;
            let duration_ms = gap_ms(words[span_start].start, words[span_end].end);
            let needs_retry = duration_ms >= LONG_SPAN_RESTORE_MIN_DURATION_MS
                || word_total >= LONG_SPAN_RESTORE_MIN_WORDS;
            if !needs_retry {
                return None;
            }
            Some(LongSpanRecoveryTask {
                span_index,
                span_start,
                span_end,
            })
        })
        .collect::<Vec<_>>();

    if recovery_tasks.is_empty() {
        return out;
    }

    let request_for_tasks = request.clone();
    let llm_client_for_tasks = llm_client.clone();
    let words_for_tasks = Arc::new(words.to_vec());
    let results = run_indexed_concurrent(
        recovery_tasks,
        request.llm_concurrency.max(1) as usize,
        move |task| {
            let request = request_for_tasks.clone();
            let llm_client = llm_client_for_tasks.clone();
            let words = Arc::clone(&words_for_tasks);
            async move {
                Ok(collect_long_span_recovery_boundary_ends_for_span(
                    &request,
                    words.as_ref().as_slice(),
                    task.span_index,
                    task.span_start,
                    task.span_end,
                    &llm_client,
                )
                .await)
            }
        },
        |message| message,
    )
    .await;

    for (_, result) in results {
        if let Ok(span_boundary_ends) = result {
            out.extend(span_boundary_ends);
        }
    }

    out
}

async fn collect_long_span_recovery_boundary_ends_for_span(
    request: &SentenceBoundaryRequest,
    words: &[WordTokenDto],
    span_index: usize,
    span_start: usize,
    span_end: usize,
    llm_client: &OpenAiCompatLlmClient,
) -> Vec<usize> {
    if span_start >= words.len() || span_end >= words.len() || span_start >= span_end {
        return Vec::new();
    }

    let duration_ms = gap_ms(words[span_start].start, words[span_end].end);
    let mut span_boundary_ends = Vec::<usize>::new();
    let windows = build_semantic_group_windows_for_region_with_limits(
        words,
        span_start,
        span_end,
        LONG_SPAN_RESTORE_CORE_MAX_WORDS,
        LONG_SPAN_RESTORE_CORE_MAX_CHARS,
        LONG_SPAN_RESTORE_CONTEXT_MAX_WORDS,
        LONG_SPAN_RESTORE_CONTEXT_MAX_CHARS,
    );
    for (window_index, window) in windows.into_iter().enumerate() {
        let prompt_words = &words[window.prompt_start..=window.prompt_end];
        let prompt = build_end_punctuation_restore_prompt_with_mode(
            &request.source_lang,
            prompt_words,
            true,
            false,
        );
        let context = LlmCallContext {
            task_id: request.task_id.clone(),
            media_path: Some(request.media_path.clone()),
            phase: format!(
                "restore_end_punc_long_span_{}_window_{}",
                span_index + 1,
                window_index + 1
            ),
        };
        let llm_id = next_llm_request_id();
        let result = llm_client
            .call_json_validated(&context, &llm_id, &prompt, None, |value| {
                let parsed = parse_restored_token_array_extraction(value).map_err(|err| {
                    LlmSemanticValidationError::retryable(format!(
                        "restore long span end punctuation parse failed: {err}"
                    ))
                })?;
                validate_restored_tokens_for_words(prompt_words, &parsed)
                    .map_err(LlmSemanticValidationError::retryable)?;
                Ok(parsed)
            })
            .await;
        let Ok(result) = result else {
            continue;
        };
        span_boundary_ends.extend(collect_restored_boundary_ends_in_core(
            window,
            &result.value.tokens,
        ));
    }

    let target_sentence_count =
        ((duration_ms + LONG_SPAN_TARGET_SENTENCE_DURATION_MS - 1)
            / LONG_SPAN_TARGET_SENTENCE_DURATION_MS) as usize;
    let target_split_count = target_sentence_count.saturating_sub(1);
    if span_boundary_ends.len() < target_split_count {
        let dense_windows = build_semantic_group_windows_for_region_with_limits(
            words,
            span_start,
            span_end,
            LONG_SPAN_DENSE_CORE_MAX_WORDS,
            LONG_SPAN_DENSE_CORE_MAX_CHARS,
            LONG_SPAN_DENSE_CONTEXT_MAX_WORDS,
            LONG_SPAN_DENSE_CONTEXT_MAX_CHARS,
        );
        for (window_index, window) in dense_windows.into_iter().enumerate() {
            let prompt_words = &words[window.prompt_start..=window.prompt_end];
            let prompt = build_end_punctuation_restore_prompt_with_mode(
                &request.source_lang,
                prompt_words,
                true,
                true,
            );
            let context = LlmCallContext {
                task_id: request.task_id.clone(),
                media_path: Some(request.media_path.clone()),
                phase: format!(
                    "restore_end_punc_long_span_dense_{}_window_{}",
                    span_index + 1,
                    window_index + 1
                ),
            };
            let llm_id = next_llm_request_id();
            let result = llm_client
                .call_json_validated(&context, &llm_id, &prompt, None, |value| {
                    let parsed = parse_restored_token_array_extraction(value).map_err(|err| {
                        LlmSemanticValidationError::retryable(format!(
                            "restore long span dense end punctuation parse failed: {err}"
                        ))
                    })?;
                    validate_restored_tokens_for_words(prompt_words, &parsed)
                        .map_err(LlmSemanticValidationError::retryable)?;
                    Ok(parsed)
                })
                .await;
            let Ok(result) = result else {
                continue;
            };
            span_boundary_ends.extend(collect_restored_boundary_ends_in_core(
                window,
                &result.value.tokens,
            ));
        }
    }

    if span_boundary_ends.len() < target_split_count {
        span_boundary_ends.extend(
            collect_sentence_array_boundary_ends_for_span(
                request, words, span_index, span_start, span_end, llm_client,
            )
            .await,
        );
    }

    span_boundary_ends
}

async fn collect_restore_end_punc_boundary_ends(
    request: &SentenceBoundaryRequest,
    llm_client: &OpenAiCompatLlmClient,
    tasks: Vec<RestoreEndPuncTask>,
) -> Vec<usize> {
    if tasks.is_empty() {
        return Vec::new();
    }

    let request_for_tasks = request.clone();
    let llm_client_for_tasks = llm_client.clone();
    let results = run_indexed_concurrent(
        tasks,
        request.llm_concurrency.max(1) as usize,
        move |task| {
            let request = request_for_tasks.clone();
            let llm_client = llm_client_for_tasks.clone();
            async move {
                let RestoreEndPuncTask {
                    phase,
                    window,
                    prompt,
                    prompt_words,
                    parse_error_prefix,
                } = task;

                let context = LlmCallContext {
                    task_id: request.task_id.clone(),
                    media_path: Some(request.media_path.clone()),
                    phase,
                };
                let llm_id = next_llm_request_id();
                let result = llm_client
                    .call_json_validated(&context, &llm_id, &prompt, None, move |value| {
                        let parsed =
                            parse_restored_token_array_extraction(value).map_err(|err| {
                                LlmSemanticValidationError::retryable(format!(
                                    "{parse_error_prefix}: {err}"
                                ))
                            })?;
                        validate_restored_tokens_for_words(&prompt_words, &parsed)
                            .map_err(LlmSemanticValidationError::retryable)?;
                        Ok(parsed)
                    })
                    .await
                    .map_err(|err| err.message)?;

                Ok(collect_restored_boundary_ends_in_core(
                    window,
                    &result.value.tokens,
                ))
            }
        },
        |message| message,
    )
    .await;

    let mut out = Vec::<usize>::new();
    for (_, result) in results {
        if let Ok(boundary_ends) = result {
            out.extend(boundary_ends);
        }
    }
    out
}

async fn collect_sentence_array_boundary_ends_for_span(
    request: &SentenceBoundaryRequest,
    words: &[WordTokenDto],
    span_index: usize,
    span_start: usize,
    span_end: usize,
    llm_client: &OpenAiCompatLlmClient,
) -> Vec<usize> {
    if span_start >= words.len() || span_end >= words.len() || span_start >= span_end {
        return Vec::new();
    }
    let span_words = &words[span_start..=span_end];
    let text = join_words(span_words.iter().map(|word| word.word.as_str()));
    let prompt = build_semantic_group_prompt(&request.source_lang, &text);
    let context = LlmCallContext {
        task_id: request.task_id.clone(),
        media_path: Some(request.media_path.clone()),
        phase: format!("restore_end_punc_long_span_array_{}", span_index + 1),
    };
    let llm_id = next_llm_request_id();
    let result = llm_client
        .call_json_validated(&context, &llm_id, &prompt, None, |value| {
            let parsed = parse_split_sentence_array_extraction(value).map_err(|err| {
                LlmSemanticValidationError::retryable(format!(
                    "long span sentence array parse failed: {err}"
                ))
            })?;
            let aligned = align_semantic_sentences_to_words(&parsed.sentences, span_words)
                .map_err(LlmSemanticValidationError::retryable)?;
            Ok(aligned)
        })
        .await;
    let Ok(result) = result else {
        return Vec::new();
    };

    let mut out = Vec::<usize>::new();
    for (_, local_end) in result.value.into_iter() {
        let global_end = span_start + local_end;
        if global_end < span_end {
            out.push(global_end);
        }
    }
    out
}

fn collect_existing_terminal_boundary_ends(words: &[WordTokenDto]) -> Vec<usize> {
    words
        .iter()
        .enumerate()
        .filter_map(|(index, word)| ends_with_terminal_punctuation(&word.word).then_some(index))
        .collect()
}

fn build_no_terminal_runs(words: &[WordTokenDto]) -> Vec<(usize, usize)> {
    let mut out = Vec::<(usize, usize)>::new();
    let mut start: Option<usize> = None;

    for (index, word) in words.iter().enumerate() {
        if ends_with_terminal_punctuation(&word.word) {
            if let Some(run_start) = start.take() {
                if run_start <= index.saturating_sub(1) {
                    out.push((run_start, index - 1));
                }
            }
            continue;
        }
        if start.is_none() {
            start = Some(index);
        }
    }
    if let Some(run_start) = start {
        out.push((run_start, words.len() - 1));
    }
    out
}

fn build_spans_from_boundary_ends(
    word_total: usize,
    boundary_ends: &[usize],
) -> Vec<(usize, usize)> {
    if word_total == 0 {
        return Vec::new();
    }

    let mut ends = boundary_ends
        .iter()
        .copied()
        .filter(|end| *end < word_total)
        .collect::<Vec<_>>();
    ends.sort_unstable();
    ends.dedup();

    let mut out = Vec::<(usize, usize)>::new();
    let mut cursor = 0usize;
    for end in ends {
        if end < cursor || end + 1 >= word_total {
            continue;
        }
        out.push((cursor, end));
        cursor = end + 1;
    }
    out.push((cursor, word_total - 1));
    out
}

fn build_end_punctuation_restore_prompt(source_lang: &str, words: &[WordTokenDto]) -> String {
    build_end_punctuation_restore_prompt_with_mode(source_lang, words, false, false)
}

fn build_end_punctuation_restore_prompt_with_mode(
    source_lang: &str,
    words: &[WordTokenDto],
    strict_mode: bool,
    dense_mode: bool,
) -> String {
    let tokens = words
        .iter()
        .map(|word| word.word.clone())
        .collect::<Vec<_>>();
    let mut policy = vec![
        "Keep the exact token order and token content.",
        "Output must contain exactly the same number of tokens as input.",
        "Do not add, remove, replace, or reorder tokens.",
        "You may only append sentence-ending punctuation to the end of a token: ., ?, !, 。, ？, ！",
        "Do not modify punctuation inside a token.",
        "Mark every clearly complete thought with sentence-ending punctuation.",
        "Avoid long run-on text. Prefer shorter complete sentences suitable for translation.",
        "For long windows, insert multiple sentence endings whenever a clause or utterance is complete.",
    ];
    if strict_mode {
        policy.push("This span was too long previously. You must add enough sentence-ending punctuation to break it into multiple complete sentences.");
        policy.push("Do not leave the entire span as a single sentence.");
    }
    if dense_mode {
        policy.push("Break long spoken text aggressively into short complete sentences to avoid long run-on output.");
        policy.push("For long speech spans, add sentence endings frequently at natural clause completion points.");
    }
    let payload = serde_json::json!({
        "task": "restore_sentence_end_punctuation",
        "sourceLanguage": source_lang,
        "rule": "Return JSON only.",
        "goal": "Restore missing sentence-ending punctuation so the text can be split into complete translation-friendly sentences.",
        "policy": policy,
        "tokens": tokens,
        "output": {
            "tokens": tokens
        }
    });
    payload.to_string()
}

fn validate_restored_tokens_for_words(
    words: &[WordTokenDto],
    extraction: &RestoredTokenArrayExtraction,
) -> Result<(), String> {
    if extraction.tokens.len() != words.len() {
        return Err(format!(
            "restored token count mismatch: {} != {}",
            extraction.tokens.len(),
            words.len()
        ));
    }

    for (source, restored) in words.iter().zip(extraction.tokens.iter()) {
        let expected = normalize_restored_token_for_compare(source.word.trim());
        let got = normalize_restored_token_for_compare(restored.trim());
        if expected != got {
            return Err("restored tokens must preserve exact token text order".to_string());
        }
    }
    Ok(())
}

fn parse_restored_token_array_extraction(
    value: serde_json::Value,
) -> Result<RestoredTokenArrayExtraction, String> {
    if let Ok(parsed) = serde_json::from_value::<RestoredTokenArrayExtraction>(value.clone()) {
        return Ok(parsed);
    }
    if let Some(tokens_value) = value.get("output").and_then(|output| output.get("tokens")) {
        let tokens = serde_json::from_value::<Vec<String>>(tokens_value.clone())
            .map_err(|err| format!("output.tokens parse failed: {err}"))?;
        return Ok(RestoredTokenArrayExtraction { tokens });
    }
    Err("missing tokens field".to_string())
}

fn parse_split_sentence_array_extraction(
    value: serde_json::Value,
) -> Result<SplitSentenceArrayExtraction, String> {
    if let Ok(parsed) = serde_json::from_value::<SplitSentenceArrayExtraction>(value.clone()) {
        if !parsed.sentences.is_empty() {
            return Ok(parsed);
        }
    }
    if let Some(sentences_value) = value
        .get("output")
        .and_then(|output| output.get("sentences"))
    {
        let sentences = serde_json::from_value::<Vec<String>>(sentences_value.clone())
            .map_err(|err| format!("output.sentences parse failed: {err}"))?;
        if !sentences.is_empty() {
            return Ok(SplitSentenceArrayExtraction { sentences });
        }
    }
    Err("missing sentences field".to_string())
}

fn collect_restored_boundary_ends_in_core(
    window: SemanticGroupWindow,
    restored_tokens: &[String],
) -> Vec<usize> {
    let mut out = Vec::<usize>::new();
    for (index, token) in restored_tokens.iter().enumerate() {
        let global = window.prompt_start + index;
        if global < window.core_start || global > window.core_end {
            continue;
        }
        if ends_with_terminal_punctuation(token) {
            out.push(global);
        }
    }
    out
}

fn build_semantic_group_windows(words: &[WordTokenDto]) -> Vec<SemanticGroupWindow> {
    if words.is_empty() {
        return Vec::new();
    }
    build_semantic_group_windows_for_region(words, 0, words.len() - 1)
}

fn build_semantic_group_windows_for_region(
    words: &[WordTokenDto],
    region_start: usize,
    region_end: usize,
) -> Vec<SemanticGroupWindow> {
    build_semantic_group_windows_for_region_with_limits(
        words,
        region_start,
        region_end,
        SEMANTIC_GROUP_CORE_MAX_WORDS,
        SEMANTIC_GROUP_CORE_MAX_CHARS,
        SEMANTIC_GROUP_CONTEXT_MAX_WORDS,
        SEMANTIC_GROUP_CONTEXT_MAX_CHARS,
    )
}

fn build_semantic_group_windows_for_region_with_limits(
    words: &[WordTokenDto],
    region_start: usize,
    region_end: usize,
    core_max_words: usize,
    core_max_chars: usize,
    context_max_words: usize,
    context_max_chars: usize,
) -> Vec<SemanticGroupWindow> {
    if words.is_empty() || region_start >= words.len() || region_start > region_end {
        return Vec::new();
    }
    let region_end = region_end.min(words.len() - 1);
    let core_max_words = core_max_words.max(1);
    let core_max_chars = core_max_chars.max(1);
    let context_max_words = context_max_words.max(1);
    let context_max_chars = context_max_chars.max(1);
    let mut out = Vec::<SemanticGroupWindow>::new();
    let mut cursor = region_start;
    while cursor <= region_end {
        let mut core_end = cursor;
        let mut core_words = 0usize;
        let mut core_chars = 0usize;
        while core_end <= region_end {
            let next_len = semantic_token_prompt_len(&words[core_end].word);
            let exceed_words = core_words >= core_max_words;
            let exceed_chars = core_words > 0 && core_chars + next_len + 1 > core_max_chars;
            if exceed_words || exceed_chars {
                break;
            }
            core_words += 1;
            core_chars += if core_words > 1 {
                next_len + 1
            } else {
                next_len
            };
            core_end += 1;
        }

        if core_end <= cursor {
            core_end = (cursor + 1).min(region_end + 1);
        }
        let core_end_inclusive = core_end - 1;

        let mut prompt_start = cursor;
        let mut left_words = 0usize;
        let mut left_chars = 0usize;
        while prompt_start > 0 {
            let next_len = semantic_token_prompt_len(&words[prompt_start - 1].word);
            let exceed_words = left_words >= context_max_words;
            let exceed_chars = left_words > 0 && left_chars + next_len + 1 > context_max_chars;
            if exceed_words || exceed_chars {
                break;
            }
            prompt_start -= 1;
            left_words += 1;
            left_chars += if left_words > 1 {
                next_len + 1
            } else {
                next_len
            };
        }

        let mut prompt_end = core_end_inclusive;
        let mut right_words = 0usize;
        let mut right_chars = 0usize;
        while prompt_end + 1 < words.len() {
            let next_len = semantic_token_prompt_len(&words[prompt_end + 1].word);
            let exceed_words = right_words >= context_max_words;
            let exceed_chars = right_words > 0 && right_chars + next_len + 1 > context_max_chars;
            if exceed_words || exceed_chars {
                break;
            }
            prompt_end += 1;
            right_words += 1;
            right_chars += if right_words > 1 {
                next_len + 1
            } else {
                next_len
            };
        }

        out.push(SemanticGroupWindow {
            core_start: cursor,
            core_end: core_end_inclusive,
            prompt_start,
            prompt_end,
        });
        cursor = core_end_inclusive + 1;
    }

    out
}

fn semantic_token_prompt_len(token: &str) -> usize {
    let trimmed = token.trim();
    if trimmed.is_empty() {
        return 1;
    }
    trimmed.chars().count().max(1)
}

fn project_window_ranges_to_core(
    window: SemanticGroupWindow,
    ranges: &[(usize, usize)],
) -> Vec<(usize, usize)> {
    let mut out = Vec::<(usize, usize)>::new();
    let mut cursor = window.core_start;

    for (_, local_end) in ranges.iter().copied() {
        let global_end = window.prompt_start + local_end;
        if global_end < window.core_start || global_end < cursor {
            continue;
        }
        let end = global_end.min(window.core_end);
        out.push((cursor, end));
        cursor = end.saturating_add(1);
        if cursor > window.core_end {
            break;
        }
    }

    if cursor <= window.core_end {
        out.push((cursor, window.core_end));
    }

    out
}

fn align_semantic_sentences_to_words(
    sentences: &[String],
    words: &[WordTokenDto],
) -> Result<Vec<(usize, usize)>, String> {
    if words.is_empty() {
        return Ok(Vec::new());
    }
    let normalized_sentences = sentences
        .iter()
        .map(|sentence| sentence.trim().to_string())
        .filter(|sentence| !sentence.is_empty())
        .collect::<Vec<_>>();
    if normalized_sentences.is_empty() {
        return Err("semantic grouping returned only empty sentences".to_string());
    }
    if normalized_sentences.len() > words.len() {
        return Err(format!(
            "semantic grouping returned too many sentences: {} > {} words",
            normalized_sentences.len(),
            words.len()
        ));
    }

    let source_chars = flatten_words_for_alignment(words);
    let sentence_total = normalized_sentences.len();
    let mut out = Vec::<(usize, usize)>::with_capacity(sentence_total);
    let mut cursor_word = 0usize;
    let mut char_cursor = 0usize;
    for (sentence_index, sentence) in normalized_sentences.iter().enumerate() {
        let remaining = sentence_total - sentence_index - 1;
        let max_end = words.len().saturating_sub(remaining + 1);
        let remain_words = words.len().saturating_sub(cursor_word);
        let slots = sentence_total - sentence_index;
        let estimated_take = ((remain_words as f64) / (slots as f64)).ceil().max(1.0) as usize;
        let estimated_end = (cursor_word + estimated_take - 1).min(max_end);

        if source_chars.is_empty() {
            out.push((cursor_word, estimated_end));
            cursor_word = estimated_end.saturating_add(1);
            continue;
        }

        let target = normalize_text_for_alignment(sentence);
        if target.is_empty() {
            out.push((cursor_word, estimated_end));
            cursor_word = estimated_end.saturating_add(1);
            continue;
        }

        let mut matched = 0usize;
        let mut last_match: Option<usize> = None;
        for idx in char_cursor..source_chars.len() {
            if source_chars[idx].0 != target[matched] {
                continue;
            }
            matched += 1;
            last_match = Some(idx);
            if matched >= target.len() {
                break;
            }
        }

        let end = if matched > 0 {
            let raw_end = source_chars[last_match.expect("matched > 0")].1;
            let resolved = raw_end.clamp(cursor_word, max_end);
            char_cursor = last_match.expect("matched > 0").saturating_add(1);
            resolved
        } else {
            while char_cursor < source_chars.len() && source_chars[char_cursor].1 <= estimated_end {
                char_cursor += 1;
            }
            estimated_end
        };

        out.push((cursor_word, end));
        cursor_word = end.saturating_add(1);
    }

    if cursor_word < words.len() {
        if let Some(last) = out.last_mut() {
            last.1 = words.len() - 1;
        }
    }
    Ok(out)
}

fn flatten_words_for_alignment(words: &[WordTokenDto]) -> Vec<(char, usize)> {
    let mut out = Vec::<(char, usize)>::new();
    for (word_index, word) in words.iter().enumerate() {
        for ch in normalize_text_for_alignment(&word.word) {
            out.push((ch, word_index));
        }
    }
    out
}

fn normalize_text_for_alignment(raw: &str) -> Vec<char> {
    let mut out = Vec::<char>::new();
    for ch in raw.chars() {
        if ch.is_whitespace() {
            continue;
        }
        if matches!(
            ch,
            ',' | '，'
                | '.'
                | '。'
                | '!'
                | '！'
                | '?'
                | '？'
                | ';'
                | '；'
                | ':'
                | '：'
                | '"'
                | '\''
        ) {
            continue;
        }
        if ch.is_ascii_alphabetic() {
            out.push(ch.to_ascii_lowercase());
            continue;
        }
        if ch.is_ascii_alphanumeric() || ch == '_' || is_cjk_char(ch) {
            out.push(ch);
            continue;
        }
        if ch.is_alphanumeric() {
            for lower in ch.to_lowercase() {
                out.push(lower);
            }
        }
    }
    out
}

fn enforce_hard_pause_spans(
    words: &[WordTokenDto],
    spans: &[(usize, usize)],
) -> Vec<(usize, usize)> {
    let mut out = Vec::<(usize, usize)>::new();
    for (start, end) in spans.iter().copied() {
        if start >= words.len() || end >= words.len() || start > end {
            continue;
        }
        let mut cursor = start;
        for index in start..end {
            let gap = gap_ms(words[index].end, words[index + 1].start);
            if gap < HARD_SPLIT_GAP_MS {
                continue;
            }
            out.push((cursor, index));
            cursor = index + 1;
        }
        out.push((cursor, end));
    }
    out
}

fn build_sentences_from_word_spans(
    words: &[WordTokenDto],
    spans: &[(usize, usize)],
) -> Vec<SourceSentence> {
    spans
        .iter()
        .enumerate()
        .map(|(index, (start, end))| SourceSentence {
            sentence_id: index + 1,
            start_ms: (words[*start].start.max(0.0) * 1000.0).round() as u64,
            end_ms: (words[*end].end.max(words[*start].start).max(0.0) * 1000.0).round() as u64,
            text: join_words(words[*start..=*end].iter().map(|word| word.word.as_str())),
            word_start: *start,
            word_end: *end,
            chunk_start: *start + 1,
            chunk_end: *end + 1,
        })
        .collect()
}

fn build_boundaries_from_spans(
    micro_chunks: &[MicroChunk],
    spans: &[(usize, usize)],
) -> Vec<BoundaryDecision> {
    if micro_chunks.len() < 2 {
        return Vec::new();
    }
    let mut split_after = vec![false; micro_chunks.len().saturating_sub(1)];
    for (_, end) in spans.iter().copied() {
        if end < split_after.len() {
            split_after[end] = true;
        }
    }

    let mut out = Vec::<BoundaryDecision>::with_capacity(micro_chunks.len() - 1);
    for index in 0..micro_chunks.len() - 1 {
        let left = &micro_chunks[index];
        let right = &micro_chunks[index + 1];
        let hard_pause = left.gap_after_ms >= HARD_SPLIT_GAP_MS;
        let final_decision = if hard_pause {
            BoundaryDecisionKind::HardSplit
        } else if split_after[index] {
            BoundaryDecisionKind::Split
        } else {
            BoundaryDecisionKind::Merge
        };
        out.push(BoundaryDecision {
            left_chunk_id: left.chunk_id,
            right_chunk_id: right.chunk_id,
            gap_ms: left.gap_after_ms,
            rule_decision: if hard_pause {
                BoundaryDecisionKind::HardSplit
            } else {
                BoundaryDecisionKind::Unknown
            },
            llm_decision: if hard_pause {
                BoundaryDecisionKind::Unknown
            } else {
                final_decision
            },
            final_decision,
            confidence: if hard_pause { 1.0 } else { 0.88 },
            reason_tag: if hard_pause {
                "hard_pause".to_string()
            } else {
                "end_punctuation".to_string()
            },
        });
    }
    out
}

fn build_semantic_group_prompt(source_lang: &str, text: &str) -> String {
    let payload = serde_json::json!({
        "task": "group_into_complete_semantic_sentences",
        "sourceLanguage": source_lang,
        "rule": "Return JSON only.",
        "goal": "Split the text into complete semantic sentences suitable for downstream translation.",
        "policy": [
            "Keep original meaning and order.",
            "Try to keep wording close to source text.",
            "Do not output timestamps, indices, or commentary.",
            "For speech without punctuation, infer natural boundaries by meaning and clause completion.",
            "Avoid very long run-on sentences. Prefer concise complete sentences and split long thoughts into multiple sentences."
        ],
        "text": text,
        "output": {
            "sentences": [text]
        }
    });
    payload.to_string()
}

fn build_micro_chunks(words: &[WordTokenDto]) -> Vec<MicroChunk> {
    build_micro_chunks_for_source_lang(words, false)
}

fn build_micro_chunks_for_source_lang(
    words: &[WordTokenDto],
    is_no_whitespace_language: bool,
) -> Vec<MicroChunk> {
    if words.is_empty() {
        return Vec::new();
    }
    if is_no_whitespace_language {
        return build_cjk_micro_chunks(words);
    }
    words
        .iter()
        .enumerate()
        .map(|(index, word)| {
            let one = [word];
            build_micro_chunk(index + 1, &one, index, index, words)
        })
        .collect()
}

fn build_cjk_micro_chunks(words: &[WordTokenDto]) -> Vec<MicroChunk> {
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut chunk_id = 1usize;

    for end in 0..words.len() {
        let token_count = end.saturating_sub(start) + 1;
        let duration_ms = gap_ms(words[start].start, words[end].end);
        let token_text = words[end].word.trim();
        let has_terminal = ends_with_terminal_punctuation(token_text);
        let gap_after_ms = if end + 1 < words.len() {
            gap_ms(words[end].end, words[end + 1].start)
        } else {
            0
        };
        let is_last = end + 1 >= words.len();
        let should_split = is_last
            || gap_after_ms >= HARD_SPLIT_GAP_MS
            || token_count >= CJK_ATOM_MAX_TOKENS
            || duration_ms >= CJK_ATOM_MAX_DURATION_MS
            || (has_terminal && token_count >= CJK_ATOM_MIN_TOKENS)
            || (gap_after_ms >= CJK_SOFT_SPLIT_GAP_MS && token_count >= CJK_ATOM_MIN_TOKENS);
        if !should_split {
            continue;
        }

        let refs = words[start..=end].iter().collect::<Vec<_>>();
        out.push(build_micro_chunk(chunk_id, &refs, start, end, words));
        chunk_id += 1;
        start = end + 1;
    }

    out
}

fn build_micro_chunk(
    chunk_id: usize,
    words: &[&WordTokenDto],
    word_start: usize,
    word_end: usize,
    all_words: &[WordTokenDto],
) -> MicroChunk {
    let start_ms = (words.first().map(|word| word.start).unwrap_or(0.0) * 1000.0).round() as u64;
    let end_ms = (words.last().map(|word| word.end).unwrap_or(0.0) * 1000.0).round() as u64;
    let gap_before_ms = if word_start == 0 {
        0
    } else {
        gap_ms(all_words[word_start - 1].end, all_words[word_start].start)
    };
    let gap_after_ms = if word_end + 1 >= all_words.len() {
        0
    } else {
        gap_ms(all_words[word_end].end, all_words[word_end + 1].start)
    };
    MicroChunk {
        chunk_id,
        start_ms,
        end_ms,
        text: join_words(words.iter().map(|word| word.word.as_str())),
        word_start,
        word_end,
        gap_before_ms,
        gap_after_ms,
        hard_split_before: gap_before_ms >= HARD_SPLIT_GAP_MS,
        hard_split_after: gap_after_ms >= HARD_SPLIT_GAP_MS,
    }
}

async fn classify_boundaries(
    request: &SentenceBoundaryRequest,
    micro_chunks: &[MicroChunk],
) -> Result<Vec<BoundaryPreference>, String> {
    if micro_chunks.len() < 2 {
        return Ok(Vec::new());
    }

    let llm_client = OpenAiCompatLlmClient::new(LlmConfig::new(
        request.translate_base_url.clone(),
        request.translate_api_key.clone(),
        request.translate_model.clone(),
    ))
    .map_err(|err| err.message)?;

    let mut out = Vec::with_capacity(micro_chunks.len() - 1);

    for index in 0..micro_chunks.len() - 1 {
        let left = micro_chunks[index].clone();
        let right = micro_chunks[index + 1].clone();
        if left.gap_after_ms >= HARD_SPLIT_GAP_MS {
            out.push(BoundaryPreference {
                left_chunk_id: left.chunk_id,
                right_chunk_id: right.chunk_id,
                gap_ms: left.gap_after_ms,
                rule_decision: BoundaryDecisionKind::HardSplit,
                llm_decision: BoundaryDecisionKind::HardSplit,
                confidence: 1.0,
                reason_tag: "hard_pause".to_string(),
            });
            continue;
        }

        if is_high_confidence_punctuation_split(&left) {
            out.push(BoundaryPreference {
                left_chunk_id: left.chunk_id,
                right_chunk_id: right.chunk_id,
                gap_ms: left.gap_after_ms,
                rule_decision: BoundaryDecisionKind::Split,
                llm_decision: BoundaryDecisionKind::Unknown,
                confidence: 0.95,
                reason_tag: "terminal_punctuation".to_string(),
            });
            continue;
        }

        out.push(BoundaryPreference {
            left_chunk_id: left.chunk_id,
            right_chunk_id: right.chunk_id,
            gap_ms: left.gap_after_ms,
            rule_decision: BoundaryDecisionKind::Unknown,
            llm_decision: BoundaryDecisionKind::Unknown,
            confidence: UNKNOWN_BOUNDARY_CONFIDENCE,
            reason_tag: "llm_pending".to_string(),
        });
    }

    let sweep_tasks = build_boundary_window_tasks(request, micro_chunks, &out);
    if sweep_tasks.is_empty() {
        return Ok(out);
    }

    let request_for_tasks = request.clone();
    let llm_client_for_tasks = llm_client.clone();
    let llm_results = run_indexed_concurrent(
        sweep_tasks.clone(),
        request.llm_concurrency as usize,
        move |task| {
            let request = request_for_tasks.clone();
            let llm_client = llm_client_for_tasks.clone();
            async move { classify_boundary_window_with_llm(&request, &llm_client, task).await }
        },
        |message| message,
    )
    .await;

    let mut votes_per_boundary = vec![Vec::<BoundaryVote>::new(); micro_chunks.len() - 1];
    let mut errored_boundaries = std::collections::HashSet::<usize>::new();

    for (task_index, result) in llm_results {
        match result {
            Ok(votes) => {
                for vote in votes {
                    if let Some(slot) = votes_per_boundary.get_mut(vote.boundary_index) {
                        slot.push(vote);
                    }
                }
            }
            Err(message) => {
                let Some(task) = sweep_tasks.get(task_index) else {
                    continue;
                };
                for local_index in 0..task.chunks.len().saturating_sub(1) {
                    errored_boundaries.insert(task.chunk_start_index + local_index);
                }
                for index in errored_boundaries.iter().copied() {
                    if let Some(target) = out.get_mut(index) {
                        target.llm_decision = BoundaryDecisionKind::Unsure;
                        target.confidence = LLM_ERROR_FALLBACK_CONFIDENCE;
                        target.reason_tag = format!("llm_error:{message}");
                    }
                }
            }
        }
    }

    let focus_tasks = build_boundary_focus_tasks(request, micro_chunks, &out, &votes_per_boundary);
    if !focus_tasks.is_empty() {
        let request_for_tasks = request.clone();
        let llm_client_for_tasks = llm_client.clone();
        let focus_results = run_indexed_concurrent(
            focus_tasks.clone(),
            request.llm_concurrency as usize,
            move |task| {
                let request = request_for_tasks.clone();
                let llm_client = llm_client_for_tasks.clone();
                async move { classify_boundary_window_with_llm(&request, &llm_client, task).await }
            },
            |message| message,
        )
        .await;

        for (task_index, result) in focus_results {
            match result {
                Ok(votes) => {
                    for vote in votes {
                        if let Some(slot) = votes_per_boundary.get_mut(vote.boundary_index) {
                            slot.push(vote);
                        }
                    }
                }
                Err(message) => {
                    let Some(task) = focus_tasks.get(task_index) else {
                        continue;
                    };
                    if let Some(boundary_index) = task.focus_boundary_index {
                        if let Some(target) = out.get_mut(boundary_index) {
                            if !target.reason_tag.starts_with("llm_error:") {
                                target.llm_decision = BoundaryDecisionKind::Unsure;
                                target.confidence =
                                    target.confidence.min(FOCUS_REVIEW_CONFIDENCE_THRESHOLD);
                                target.reason_tag = format!("llm_focus_error:{message}");
                            }
                        }
                    }
                }
            }
        }
    }

    for boundary_index in collect_pending_boundary_indexes(&out) {
        let Some(target) = out.get_mut(boundary_index) else {
            continue;
        };
        if target.reason_tag.starts_with("llm_error:") {
            continue;
        }
        let votes = votes_per_boundary
            .get(boundary_index)
            .map(|items| items.as_slice())
            .unwrap_or(&[]);
        let summary = combine_boundary_votes(votes);
        target.llm_decision = summary.decision;
        target.confidence = summary.confidence;
        target.reason_tag = summary.reason_tag;
    }

    Ok(out)
}

async fn classify_boundary_window_with_llm(
    request: &SentenceBoundaryRequest,
    llm_client: &OpenAiCompatLlmClient,
    task: BoundaryWindowTask,
) -> Result<Vec<BoundaryVote>, String> {
    let prompt = build_boundary_window_prompt(&task);
    let context = LlmCallContext {
        task_id: request.task_id.clone(),
        media_path: Some(request.media_path.clone()),
        phase: format!(
            "sentence_boundary_{}_region_{}_window_{}",
            task.mode.phase_label(),
            task.region_index + 1,
            task.window_index + 1
        ),
    };
    let validator = JsonResponseValidator::with_required_keys(&["restoredText"]);
    let llm_id = next_llm_request_id();
    let result = llm_client
        .call_json_validated(&context, &llm_id, &prompt, Some(&validator), |value| {
            let parsed =
                serde_json::from_value::<RestoredSentenceTextExtraction>(value).map_err(|err| {
                    LlmSemanticValidationError::retryable(format!(
                        "boundary window parse failed: {err}"
                    ))
                })?;
            validate_restored_sentence_text_for_chunks(&task.chunks, &parsed)
                .map_err(LlmSemanticValidationError::retryable)
        })
        .await
        .map_err(|err| err.message)?;

    let extraction = result.value;
    let mut votes = Vec::with_capacity(task.chunks.len().saturating_sub(1));
    let sentence_end_lookup =
        extract_sentence_endings_from_restored_text(&task.chunks, &extraction.restored_text)?;

    for local_index in 0..task.chunks.len().saturating_sub(1) {
        let left_chunk = &task.chunks[local_index];
        let boundary_index = task.chunk_start_index + local_index;
        let ending = sentence_end_lookup.get(&left_chunk.chunk_id).copied();
        let (decision, confidence, punctuation, weight) = if let Some(ending) = ending {
            let confidence = match task.mode {
                BoundaryWindowMode::Sweep => 0.9,
                BoundaryWindowMode::Focus => 0.96,
            };
            let weight = match task.mode {
                BoundaryWindowMode::Sweep => 1.0,
                BoundaryWindowMode::Focus => 1.35,
            };
            (BoundaryDecisionKind::Split, confidence, ending, weight)
        } else {
            let confidence = match task.mode {
                BoundaryWindowMode::Sweep => UNKNOWN_BOUNDARY_CONFIDENCE,
                BoundaryWindowMode::Focus => 0.72,
            };
            let weight = match task.mode {
                BoundaryWindowMode::Sweep => 1.0,
                BoundaryWindowMode::Focus => 1.2,
            };
            (
                BoundaryDecisionKind::Merge,
                confidence,
                RestoredPunctuation::None,
                weight,
            )
        };
        votes.push(BoundaryVote {
            boundary_index,
            decision,
            confidence,
            punctuation,
            weight,
        });
    }
    Ok(votes)
}

async fn refine_boundary_preferences(
    request: &SentenceBoundaryRequest,
    micro_chunks: &[MicroChunk],
    preferences: &[BoundaryPreference],
    draft_sentences: &[SourceSentence],
) -> Result<Vec<BoundaryPreference>, String> {
    if micro_chunks.len() < 2 || draft_sentences.is_empty() {
        return Ok(preferences.to_vec());
    }

    let tasks = build_sentence_refine_tasks(request, micro_chunks, draft_sentences);
    if tasks.is_empty() {
        return Ok(preferences.to_vec());
    }

    let llm_client = OpenAiCompatLlmClient::new(LlmConfig::new(
        request.translate_base_url.clone(),
        request.translate_api_key.clone(),
        request.translate_model.clone(),
    ))
    .map_err(|err| err.message)?;

    let request_for_tasks = request.clone();
    let llm_client_for_tasks = llm_client.clone();
    let results = run_indexed_concurrent(
        tasks.clone(),
        request.llm_concurrency as usize,
        move |task| {
            let request = request_for_tasks.clone();
            let llm_client = llm_client_for_tasks.clone();
            async move { refine_sentence_window_with_llm(&request, &llm_client, task).await }
        },
        |message| message,
    )
    .await;

    let mut updated = preferences.to_vec();
    let mut votes_per_boundary = vec![Vec::<BoundaryVote>::new(); preferences.len()];
    for (task_index, result) in results {
        let Some(task) = tasks.get(task_index) else {
            continue;
        };
        let Ok(votes) = result else {
            continue;
        };
        for vote in votes {
            if vote.boundary_index < task.chunk_start_index
                || vote.boundary_index >= task.chunk_end_index
            {
                continue;
            }
            if let Some(slot) = votes_per_boundary.get_mut(vote.boundary_index) {
                slot.push(vote);
            }
        }
    }

    for (boundary_index, votes) in votes_per_boundary.iter().enumerate() {
        if votes.is_empty() {
            continue;
        }
        let Some(target) = updated.get_mut(boundary_index) else {
            continue;
        };
        let summary = combine_refine_boundary_votes(votes);
        target.llm_decision = summary.decision;
        target.confidence = summary.confidence.max(target.confidence).clamp(0.0, 1.0);
        target.reason_tag = summary.reason_tag;
    }

    Ok(updated)
}

async fn refine_sentence_window_with_llm(
    request: &SentenceBoundaryRequest,
    llm_client: &OpenAiCompatLlmClient,
    task: SentenceRefineTask,
) -> Result<Vec<BoundaryVote>, String> {
    let prompt = build_sentence_refine_prompt(&task);
    let context = LlmCallContext {
        task_id: request.task_id.clone(),
        media_path: Some(request.media_path.clone()),
        phase: format!("sentence_refine_region_{}", task.region_index + 1),
    };
    let validator = JsonResponseValidator::with_required_keys(&["restoredText"]);
    let llm_id = next_llm_request_id();
    let result = llm_client
        .call_json_validated(&context, &llm_id, &prompt, Some(&validator), |value| {
            let parsed =
                serde_json::from_value::<RestoredSentenceTextExtraction>(value).map_err(|err| {
                    LlmSemanticValidationError::retryable(format!(
                        "sentence refine parse failed: {err}"
                    ))
                })?;
            validate_restored_sentence_text_for_chunks(&task.chunks, &parsed)
                .map_err(LlmSemanticValidationError::retryable)
        })
        .await
        .map_err(|err| err.message)?;

    let extraction = result.value;
    let restored_endings =
        extract_sentence_endings_from_restored_text(&task.chunks, &extraction.restored_text)?;

    let mut votes = Vec::with_capacity(task.chunks.len().saturating_sub(1));
    for local_index in 0..task.chunks.len().saturating_sub(1) {
        let left_chunk = &task.chunks[local_index];
        let boundary_index = task.chunk_start_index + local_index;
        let punctuation = restored_endings
            .get(&left_chunk.chunk_id)
            .copied()
            .unwrap_or(RestoredPunctuation::None);
        let (confidence, punctuation) = if punctuation != RestoredPunctuation::None {
            (0.92, punctuation)
        } else {
            (0.7, RestoredPunctuation::None)
        };
        let decision = if punctuation == RestoredPunctuation::None {
            BoundaryDecisionKind::Merge
        } else {
            BoundaryDecisionKind::Split
        };
        votes.push(BoundaryVote {
            boundary_index,
            decision,
            confidence,
            punctuation,
            weight: 1.15,
        });
    }
    Ok(votes)
}

async fn split_long_sentences_with_llm(
    request: &SentenceBoundaryRequest,
    micro_chunks: &[MicroChunk],
    sentences: Vec<SourceSentence>,
) -> Result<Vec<SourceSentence>, String> {
    let llm_client = OpenAiCompatLlmClient::new(LlmConfig::new(
        request.translate_base_url.clone(),
        request.translate_api_key.clone(),
        request.translate_model.clone(),
    ))
    .map_err(|err| err.message)?;

    let mut current = sentences;
    for _round in 0..MAX_REFINE_ROUNDS {
        let tasks = build_long_sentence_split_tasks(request, micro_chunks, &current);
        if tasks.is_empty() {
            break;
        }

        let request_for_tasks = request.clone();
        let llm_client_for_tasks = llm_client.clone();
        let results = run_indexed_concurrent(
            tasks.clone(),
            request.llm_concurrency as usize,
            move |task| {
                let request = request_for_tasks.clone();
                let llm_client = llm_client_for_tasks.clone();
                async move { split_long_sentence_task_with_llm(&request, &llm_client, task).await }
            },
            |message| message,
        )
        .await;

        let mut replacements = std::collections::HashMap::<usize, Vec<SourceSentence>>::new();
        for (task_index, result) in results {
            let Some(task) = tasks.get(task_index) else {
                continue;
            };
            let Ok(next) = result else {
                continue;
            };
            if next.len() > 1 {
                replacements.insert(task.sentence_index, next);
            }
        }

        if replacements.is_empty() {
            break;
        }

        let mut next_sentences = Vec::new();
        for (index, sentence) in current.iter().enumerate() {
            if let Some(parts) = replacements.remove(&index) {
                next_sentences.extend(parts);
            } else {
                next_sentences.push(sentence.clone());
            }
        }
        for (index, sentence) in next_sentences.iter_mut().enumerate() {
            sentence.sentence_id = index + 1;
        }
        current = next_sentences;
    }
    Ok(current)
}

fn build_long_sentence_split_tasks(
    request: &SentenceBoundaryRequest,
    micro_chunks: &[MicroChunk],
    sentences: &[SourceSentence],
) -> Vec<LongSentenceSplitTask> {
    sentences
        .iter()
        .enumerate()
        .filter(|(_, sentence)| sentence_token_len(sentence) >= REFINE_LONG_SENTENCE_MIN_TOKENS)
        .filter_map(|(sentence_index, sentence)| {
            let start = sentence.chunk_start.saturating_sub(1);
            let end = sentence.chunk_end.saturating_sub(1);
            (start < micro_chunks.len() && end < micro_chunks.len() && start < end).then(|| {
                LongSentenceSplitTask {
                    sentence_index,
                    sentence: sentence.clone(),
                    chunks: micro_chunks[start..=end].to_vec(),
                    source_lang: request.source_lang.clone(),
                }
            })
        })
        .collect()
}

async fn split_long_sentence_task_with_llm(
    request: &SentenceBoundaryRequest,
    llm_client: &OpenAiCompatLlmClient,
    task: LongSentenceSplitTask,
) -> Result<Vec<SourceSentence>, String> {
    let endings = restore_long_sentence_endings_with_llm(request, llm_client, &task).await?;
    let initial = split_sentence_from_chunk_split_points(&task.sentence, &task.chunks, &endings);
    if initial.len() > 1 {
        return Ok(initial);
    }

    let fallback = split_long_sentence_by_array_with_llm(request, llm_client, &task).await?;
    if fallback.len() > 1 {
        Ok(fallback)
    } else {
        Ok(initial)
    }
}

async fn restore_long_sentence_endings_with_llm(
    request: &SentenceBoundaryRequest,
    llm_client: &OpenAiCompatLlmClient,
    task: &LongSentenceSplitTask,
) -> Result<std::collections::HashMap<usize, RestoredPunctuation>, String> {
    if task.chunks.len() <= LONG_SENTENCE_WINDOW_CHUNKS {
        let extraction = restore_text_for_chunk_window_with_llm(
            request,
            llm_client,
            &task.chunks,
            &format!("sentence_post_split_{}", task.sentence.sentence_id),
        )
        .await?;
        return extract_sentence_endings_from_restored_text(
            &task.chunks,
            &extraction.restored_text,
        );
    }

    let mut votes = std::collections::HashMap::<usize, Vec<RestoredPunctuation>>::new();
    let mut local_start = 0usize;
    let mut window_index = 0usize;
    while local_start < task.chunks.len() {
        let local_end = (local_start + LONG_SENTENCE_WINDOW_CHUNKS).min(task.chunks.len());
        let window_chunks = task.chunks[local_start..local_end].to_vec();
        let phase = format!(
            "sentence_post_split_{}_window_{}",
            task.sentence.sentence_id,
            window_index + 1
        );
        let extraction =
            restore_text_for_chunk_window_with_llm(request, llm_client, &window_chunks, &phase)
                .await?;
        let endings =
            extract_sentence_endings_from_restored_text(&window_chunks, &extraction.restored_text)?;
        for (chunk_id, punctuation) in endings {
            votes.entry(chunk_id).or_default().push(punctuation);
        }
        if local_end >= task.chunks.len() {
            break;
        }
        local_start += LONG_SENTENCE_WINDOW_STEP;
        window_index += 1;
    }

    Ok(votes
        .into_iter()
        .filter_map(|(chunk_id, marks)| {
            choose_majority_punctuation(&marks).map(|mark| (chunk_id, mark))
        })
        .collect())
}

async fn restore_text_for_chunk_window_with_llm(
    request: &SentenceBoundaryRequest,
    llm_client: &OpenAiCompatLlmClient,
    chunks: &[MicroChunk],
    phase: &str,
) -> Result<RestoredSentenceTextExtraction, String> {
    let prompt = build_long_sentence_window_prompt(chunks, &request.source_lang);
    let context = LlmCallContext {
        task_id: request.task_id.clone(),
        media_path: Some(request.media_path.clone()),
        phase: phase.to_string(),
    };
    let validator = JsonResponseValidator::with_required_keys(&["restoredText"]);
    let llm_id = next_llm_request_id();
    llm_client
        .call_json_validated(&context, &llm_id, &prompt, Some(&validator), |value| {
            let parsed =
                serde_json::from_value::<RestoredSentenceTextExtraction>(value).map_err(|err| {
                    LlmSemanticValidationError::retryable(format!(
                        "restore text parse failed: {err}"
                    ))
                })?;
            validate_restored_sentence_text_for_chunks(chunks, &parsed)
                .map_err(LlmSemanticValidationError::retryable)
        })
        .await
        .map(|result| result.value)
        .map_err(|err| err.message)
}

async fn split_long_sentence_by_array_with_llm(
    request: &SentenceBoundaryRequest,
    llm_client: &OpenAiCompatLlmClient,
    task: &LongSentenceSplitTask,
) -> Result<Vec<SourceSentence>, String> {
    let prompt = build_long_sentence_array_prompt(task, false);
    let context = LlmCallContext {
        task_id: request.task_id.clone(),
        media_path: Some(request.media_path.clone()),
        phase: format!("sentence_post_array_{}", task.sentence.sentence_id),
    };
    let validator = JsonResponseValidator::with_required_keys(&["sentences"]);
    let llm_id = next_llm_request_id();
    let result = llm_client
        .call_json_validated(&context, &llm_id, &prompt, Some(&validator), |value| {
            let parsed =
                serde_json::from_value::<SplitSentenceArrayExtraction>(value).map_err(|err| {
                    LlmSemanticValidationError::retryable(format!(
                        "split sentence array parse failed: {err}"
                    ))
                })?;
            validate_split_sentence_array_for_chunks(&task.chunks, &parsed)
                .map_err(LlmSemanticValidationError::retryable)
        })
        .await
        .map_err(|err| err.message)?;

    let baseline = split_sentence_array_to_source_sentences(&task.chunks, &result.value.sentences);
    if baseline.len() > 1 {
        return Ok(baseline);
    }

    let prompt = build_long_sentence_array_prompt(task, true);
    let context = LlmCallContext {
        task_id: request.task_id.clone(),
        media_path: Some(request.media_path.clone()),
        phase: format!(
            "sentence_post_array_aggressive_{}",
            task.sentence.sentence_id
        ),
    };
    let llm_id = next_llm_request_id();
    let aggressive = llm_client
        .call_json_validated(&context, &llm_id, &prompt, Some(&validator), |value| {
            let parsed =
                serde_json::from_value::<SplitSentenceArrayExtraction>(value).map_err(|err| {
                    LlmSemanticValidationError::retryable(format!(
                        "split sentence array parse failed: {err}"
                    ))
                })?;
            validate_split_sentence_array_for_chunks(&task.chunks, &parsed)
                .map_err(LlmSemanticValidationError::retryable)
        })
        .await
        .map_err(|err| err.message)?;
    Ok(split_sentence_array_to_source_sentences(
        &task.chunks,
        &aggressive.value.sentences,
    ))
}

fn build_boundary_window_tasks(
    request: &SentenceBoundaryRequest,
    micro_chunks: &[MicroChunk],
    preferences: &[BoundaryPreference],
) -> Vec<BoundaryWindowTask> {
    let mut tasks = Vec::<BoundaryWindowTask>::new();
    for (region_index, (region_start, region_end)) in
        build_regions(micro_chunks).into_iter().enumerate()
    {
        let region_len = region_end.saturating_sub(region_start) + 1;
        if region_len < 2 {
            continue;
        }
        let mut local_start = 0usize;
        let mut window_index = 0usize;
        while local_start + 1 < region_len {
            let local_end = (local_start + LLM_WINDOW_ATOMS).min(region_len);
            let global_start = region_start + local_start;
            let global_end_exclusive = region_start + local_end;
            let has_pending =
                (global_start..global_end_exclusive.saturating_sub(1)).any(|boundary_index| {
                    preferences[boundary_index].rule_decision == BoundaryDecisionKind::Unknown
                });
            if has_pending {
                tasks.push(BoundaryWindowTask {
                    region_index,
                    window_index,
                    chunk_start_index: global_start,
                    chunks: micro_chunks[global_start..global_end_exclusive].to_vec(),
                    source_lang: request.source_lang.clone(),
                    mode: BoundaryWindowMode::Sweep,
                    focus_boundary_index: None,
                });
            }
            if local_end >= region_len {
                break;
            }
            local_start += LLM_WINDOW_STEP;
            window_index += 1;
        }
    }
    tasks
}

fn build_boundary_focus_tasks(
    request: &SentenceBoundaryRequest,
    micro_chunks: &[MicroChunk],
    preferences: &[BoundaryPreference],
    votes_per_boundary: &[Vec<BoundaryVote>],
) -> Vec<BoundaryWindowTask> {
    if micro_chunks.len() < 2 {
        return Vec::new();
    }

    let regions = build_regions(micro_chunks);
    let mut tasks = Vec::<BoundaryWindowTask>::new();
    for boundary_index in collect_pending_boundary_indexes(preferences) {
        if !should_request_boundary_focus(boundary_index, votes_per_boundary) {
            continue;
        }
        let Some((region_index, region_start, region_end)) =
            find_region_for_boundary(&regions, boundary_index)
        else {
            continue;
        };
        if tasks.iter().any(|task: &BoundaryWindowTask| {
            task.region_index == region_index
                && boundary_index >= task.chunk_start_index
                && boundary_index < task.chunk_start_index + task.chunks.len().saturating_sub(1)
        }) {
            continue;
        }
        let (window_start, window_end) =
            build_focus_window_bounds(region_start, region_end, boundary_index);
        if window_end <= window_start {
            continue;
        }
        tasks.push(BoundaryWindowTask {
            region_index,
            window_index: tasks.len(),
            chunk_start_index: window_start,
            chunks: micro_chunks[window_start..=window_end].to_vec(),
            source_lang: request.source_lang.clone(),
            mode: BoundaryWindowMode::Focus,
            focus_boundary_index: Some(boundary_index),
        });
    }
    tasks
}

fn should_request_boundary_focus(
    boundary_index: usize,
    votes_per_boundary: &[Vec<BoundaryVote>],
) -> bool {
    let Some(votes) = votes_per_boundary.get(boundary_index) else {
        return false;
    };
    if votes.is_empty() {
        return true;
    }
    let summary = combine_boundary_votes(votes);
    summary.decision != BoundaryDecisionKind::Split
        || summary.confidence < FOCUS_REVIEW_CONFIDENCE_THRESHOLD
        || (summary.split_score - summary.merge_score).abs() <= 0.2
        || summary.total_weight < 1.5
}

fn find_region_for_boundary(
    regions: &[(usize, usize)],
    boundary_index: usize,
) -> Option<(usize, usize, usize)> {
    regions
        .iter()
        .copied()
        .enumerate()
        .find_map(|(region_index, (start, end))| {
            (boundary_index >= start && boundary_index < end).then_some((region_index, start, end))
        })
}

fn build_focus_window_bounds(
    region_start: usize,
    region_end: usize,
    boundary_index: usize,
) -> (usize, usize) {
    let mut window_start = boundary_index.saturating_sub(BOUNDARY_CONTEXT_RADIUS);
    let mut window_end = (boundary_index + 1 + BOUNDARY_CONTEXT_RADIUS).min(region_end);
    window_start = window_start.max(region_start);
    if window_end <= window_start {
        window_end = (window_start + 1).min(region_end);
    }

    while window_end.saturating_sub(window_start).saturating_add(1) < FOCUS_WINDOW_MIN_ATOMS {
        if window_start > region_start {
            window_start -= 1;
        }
        if window_end < region_end {
            window_end += 1;
        }
        if window_start == region_start && window_end == region_end {
            break;
        }
    }

    (window_start, window_end)
}

fn build_sentence_refine_tasks(
    request: &SentenceBoundaryRequest,
    micro_chunks: &[MicroChunk],
    draft_sentences: &[SourceSentence],
) -> Vec<SentenceRefineTask> {
    let suspicious = draft_sentences
        .iter()
        .enumerate()
        .filter_map(|(index, sentence)| is_suspicious_sentence(sentence).then_some(index))
        .collect::<Vec<_>>();
    if suspicious.is_empty() {
        return Vec::new();
    }

    let short_sentence_ranges = suspicious
        .iter()
        .copied()
        .filter(|&index| {
            let token_len = sentence_token_len(&draft_sentences[index]);
            (SHORT_SENTENCE_REVIEW_MIN_TOKENS..=SHORT_SENTENCE_REVIEW_MAX_TOKENS)
                .contains(&token_len)
                && !has_internal_sentence_punctuation(&draft_sentences[index].text)
        })
        .map(|index| {
            let start = index;
            let end = (index + 1).min(draft_sentences.len().saturating_sub(1));
            (start, end, true)
        })
        .collect::<std::collections::BTreeSet<_>>();

    let sentence_ranges = suspicious
        .into_iter()
        .filter(|index| {
            !short_sentence_ranges
                .iter()
                .any(|(start, end, _)| *index >= *start && *index <= *end)
        })
        .map(|index| {
            let start = index.saturating_sub(1);
            let end = (index + 1).min(draft_sentences.len().saturating_sub(1));
            (start, end, false)
        })
        .collect::<std::collections::BTreeSet<_>>();

    sentence_ranges
        .into_iter()
        .chain(short_sentence_ranges)
        .into_iter()
        .enumerate()
        .map(
            |(region_index, (sentence_start, sentence_end, short_review))| {
                let first_sentence = &draft_sentences[sentence_start];
                let last_sentence = &draft_sentences[sentence_end];
                let chunk_start_index = first_sentence.chunk_start.saturating_sub(1);
                let chunk_end_index = last_sentence.chunk_end.saturating_sub(1);
                SentenceRefineTask {
                    region_index,
                    chunk_start_index,
                    chunk_end_index,
                    chunks: micro_chunks[chunk_start_index..=chunk_end_index].to_vec(),
                    draft_sentences: draft_sentences[sentence_start..=sentence_end].to_vec(),
                    focus_sentence_ids: draft_sentences[sentence_start..=sentence_end]
                        .iter()
                        .filter(|sentence| is_suspicious_sentence(sentence))
                        .map(|sentence| sentence.sentence_id)
                        .collect(),
                    source_lang: request.source_lang.clone(),
                    short_review,
                }
            },
        )
        .collect()
}

fn sentence_chunk_len(sentence: &SourceSentence) -> usize {
    sentence
        .chunk_end
        .saturating_sub(sentence.chunk_start)
        .saturating_add(1)
}

fn sentence_token_len(sentence: &SourceSentence) -> usize {
    sentence
        .word_end
        .saturating_sub(sentence.word_start)
        .saturating_add(1)
}

fn is_suspicious_sentence(sentence: &SourceSentence) -> bool {
    let token_len = sentence_token_len(sentence);
    token_len <= REFINE_SHORT_SENTENCE_MAX_TOKENS
        || token_len >= REFINE_LONG_SENTENCE_MIN_TOKENS
        || sentence_chunk_len(sentence) <= 2
}

fn build_sentence_refine_prompt(task: &SentenceRefineTask) -> String {
    let atoms = task
        .chunks
        .iter()
        .map(|chunk| {
            serde_json::json!({
                "chunkId": chunk.chunk_id,
                "text": chunk.text,
                "gapAfterMs": chunk.gap_after_ms,
            })
        })
        .collect::<Vec<_>>();
    let draft_sentences = task
        .draft_sentences
        .iter()
        .map(|sentence| {
            serde_json::json!({
                "sentenceId": sentence.sentence_id,
                "text": sentence.text,
                "chunkStart": sentence.chunk_start,
                "chunkEnd": sentence.chunk_end,
                "tokenCount": sentence_token_len(sentence),
            })
        })
        .collect::<Vec<_>>();
    let focus = task.focus_sentence_ids.clone();
    let window_text = join_words(task.chunks.iter().map(|chunk| chunk.text.as_str()));
    let mut policy = vec![
        "Treat the text as plain speech text. Do not rely on punctuation, casing, or formatting.",
        "This is a refinement pass. You may keep, remove, or add sentence endings anywhere in the window.",
        "At least one focusSentenceId is likely over-split or over-merged. Correct it even if that requires inserting new sentence endings inside a draft sentence.",
        "Use semantic completeness as the primary criterion.",
        "Do not split a sentence just because it is long. Split only when the next chunk clearly starts a new complete sentence or utterance.",
        "Do not place a sentence end where the left side still feels unfinished or where the right side clearly continues the same clause.",
        "If a short draft sentence is actually attached to its neighbor, remove that split.",
        "Respect very long pauses: if a gapAfterMs is 2000 or more, that point must stay as a sentence end.",
        "Return restoredText only.",
        "restoredText must preserve the exact token order and token content from atoms.",
        "Do not add, remove, replace, or reorder tokens.",
        "Only attach sentence-ending punctuation directly to existing tokens.",
        "Use PERIOD by default. Use QUESTION only when the wording is clearly a question. Use EXCLAMATION only for unmistakably strong exclamations or commands.",
    ];
    if task.short_review {
        policy.insert(
            4,
            "One focus sentence may actually contain two short complete sentences or utterances.",
        );
        policy.insert(5, "If a very short left span already feels complete and the next words start another complete thought, split them even when the first sentence is only one to three words.");
        policy.insert(
            6,
            "Do not force two complete utterances to stay merged just because both are short.",
        );
    }
    let payload = serde_json::json!({
        "task": "refine_sentence_end_punctuation",
        "sourceLanguage": task.source_lang,
        "rule": "Return JSON only.",
        "goal": "Re-evaluate sentence endings in this local speech window so the result forms complete, translation-friendly sentences even when original punctuation or casing is missing.",
        "windowText": window_text,
        "focusSentenceIds": focus,
        "policy": policy,
        "atoms": atoms,
        "draftSentences": draft_sentences,
        "output": {
            "restoredText": window_text
        }
    });
    payload.to_string()
}

fn build_long_sentence_window_prompt(chunks: &[MicroChunk], source_lang: &str) -> String {
    build_restore_text_window_prompt(
        chunks,
        source_lang,
        "restore_sentence_end_punctuation_for_window",
        "Restore sentence-ending punctuation inside this short speech window while preserving exact tokens.",
        "Mark every token that clearly ends a complete sentence.",
    )
}

fn build_restore_text_window_prompt(
    chunks: &[MicroChunk],
    source_lang: &str,
    task_name: &str,
    goal: &str,
    boundary_policy: &str,
) -> String {
    let atoms = chunks
        .iter()
        .map(|chunk| {
            serde_json::json!({
                "chunkId": chunk.chunk_id,
                "text": chunk.text,
                "gapAfterMs": chunk.gap_after_ms,
            })
        })
        .collect::<Vec<_>>();
    let window_text = join_words(chunks.iter().map(|chunk| chunk.text.as_str()));
    let payload = serde_json::json!({
        "task": task_name,
        "sourceLanguage": source_lang,
        "rule": "Return JSON only.",
        "goal": goal,
        "windowText": window_text,
        "policy": [
            "Treat the text as plain speech text. Do not rely on casing or formatting.",
            "Preserve the exact token order and token content.",
            "Do not add, remove, replace, or reorder tokens.",
            "Only attach sentence-ending punctuation directly to existing tokens.",
            boundary_policy,
            "Do not place a sentence end where the left side still feels unfinished.",
            "Respect very long pauses: if a gapAfterMs is 2000 or more, that point must stay as a sentence end.",
            "Use PERIOD by default. Use QUESTION only when the wording is clearly a question. Use EXCLAMATION only for unmistakably strong exclamations or commands."
        ],
        "atoms": atoms,
        "output": {
            "restoredText": window_text
        }
    });
    payload.to_string()
}

fn build_long_sentence_array_prompt(task: &LongSentenceSplitTask, aggressive: bool) -> String {
    let atoms = task
        .chunks
        .iter()
        .map(|chunk| {
            serde_json::json!({
                "chunkId": chunk.chunk_id,
                "text": chunk.text,
            })
        })
        .collect::<Vec<_>>();
    let window_text = join_words(task.chunks.iter().map(|chunk| chunk.text.as_str()));
    let payload = serde_json::json!({
        "task": "split_into_complete_sentences",
        "sourceLanguage": task.source_lang,
        "rule": "Return JSON only.",
        "goal": "Split this draft text into complete semantic sentences for translation.",
        "windowText": window_text,
        "policy": [
            "Preserve the exact token order and token content.",
            "Do not add, remove, replace, or reorder tokens.",
            "Return each complete sentence as one array item.",
            "If the text is truly only one sentence, return one item.",
            if aggressive {
                "Prefer smaller complete translation-friendly sentences when multiple valid readings are possible."
            } else {
                "Prefer complete translation-friendly sentences over long run-on sentences."
            }
        ],
        "atoms": atoms,
        "output": {
            "sentences": [window_text]
        }
    });
    payload.to_string()
}

fn validate_restored_sentence_text_for_chunks(
    chunks: &[MicroChunk],
    extraction: &RestoredSentenceTextExtraction,
) -> Result<RestoredSentenceTextExtraction, String> {
    let expected = chunks
        .iter()
        .map(|chunk| normalize_restored_token_for_compare(&chunk.text))
        .collect::<Vec<_>>();
    let received = extraction
        .restored_text
        .split_whitespace()
        .map(normalize_restored_token_for_compare)
        .collect::<Vec<_>>();
    if expected != received {
        return Err(
            "restoredText must preserve the exact token order and content, only adding sentence-ending punctuation"
                .to_string(),
        );
    }
    Ok(extraction.clone())
}

fn validate_split_sentence_array_for_chunks(
    chunks: &[MicroChunk],
    extraction: &SplitSentenceArrayExtraction,
) -> Result<SplitSentenceArrayExtraction, String> {
    if extraction.sentences.is_empty() {
        return Err("sentences must not be empty".to_string());
    }
    let expected = chunks
        .iter()
        .map(|chunk| normalize_restored_token_for_compare(&chunk.text))
        .collect::<Vec<_>>();
    let received = extraction
        .sentences
        .iter()
        .flat_map(|sentence| sentence.split_whitespace())
        .map(normalize_restored_token_for_compare)
        .collect::<Vec<_>>();
    if expected != received {
        return Err("sentences must preserve the exact token order and content".to_string());
    }
    Ok(extraction.clone())
}

fn extract_sentence_endings_from_restored_text(
    chunks: &[MicroChunk],
    restored_text: &str,
) -> Result<std::collections::HashMap<usize, RestoredPunctuation>, String> {
    let restored_tokens = restored_text.split_whitespace().collect::<Vec<_>>();
    if restored_tokens.len() != chunks.len() {
        return Err("restoredText token count must match chunk count".to_string());
    }

    let mut out = std::collections::HashMap::new();
    for (chunk, token) in chunks.iter().zip(restored_tokens.iter()) {
        let punctuation = token
            .trim_end()
            .chars()
            .last()
            .and_then(restored_char_to_punctuation)
            .unwrap_or(RestoredPunctuation::None);
        if punctuation != RestoredPunctuation::None {
            out.insert(chunk.chunk_id, punctuation);
        }
    }
    Ok(out)
}

fn split_sentence_from_chunk_split_points(
    sentence: &SourceSentence,
    chunks: &[MicroChunk],
    endings: &std::collections::HashMap<usize, RestoredPunctuation>,
) -> Vec<SourceSentence> {
    let mut spans = Vec::<(usize, usize)>::new();
    let mut start = 0usize;
    for (index, chunk) in chunks.iter().enumerate() {
        let is_last = index + 1 == chunks.len();
        if endings.contains_key(&chunk.chunk_id) || is_last {
            spans.push((start, index));
            start = index + 1;
        }
    }

    if spans.len() <= 1 {
        return vec![sentence.clone()];
    }

    spans
        .into_iter()
        .filter(|(start, end)| start <= end)
        .map(|(start, end)| {
            let mut built = build_sentence_from_chunks(0, &chunks[start..=end]);
            if let Some(punctuation) = endings.get(&chunks[end].chunk_id).copied() {
                append_restored_punctuation(&mut built.text, punctuation);
            }
            built
        })
        .collect()
}

fn split_sentence_array_to_source_sentences(
    chunks: &[MicroChunk],
    sentences: &[String],
) -> Vec<SourceSentence> {
    let mut out = Vec::new();
    let mut cursor = 0usize;
    for sentence in sentences {
        let token_count = sentence.split_whitespace().count();
        if token_count == 0 {
            continue;
        }
        let end = cursor + token_count - 1;
        if end >= chunks.len() {
            return vec![build_sentence_from_chunks(0, chunks)];
        }
        let mut built = build_sentence_from_chunks(0, &chunks[cursor..=end]);
        if let Some(last) = sentence.trim_end().chars().last() {
            if let Some(punctuation) = restored_char_to_punctuation(last) {
                append_restored_punctuation(&mut built.text, punctuation);
            }
        }
        out.push(built);
        cursor = end + 1;
    }
    if cursor != chunks.len() || out.is_empty() {
        vec![build_sentence_from_chunks(0, chunks)]
    } else {
        out
    }
}

fn restored_char_to_punctuation(ch: char) -> Option<RestoredPunctuation> {
    match ch {
        '.' | '。' => Some(RestoredPunctuation::Period),
        '?' | '？' => Some(RestoredPunctuation::Question),
        '!' | '！' => Some(RestoredPunctuation::Exclamation),
        _ => None,
    }
}

fn normalize_restored_token_for_compare(token: &str) -> String {
    token
        .trim_end_matches(|ch: char| matches!(ch, '.' | '!' | '?' | '。' | '！' | '？'))
        .to_string()
}

fn append_restored_punctuation(text: &mut String, punctuation: RestoredPunctuation) {
    let mark = match punctuation {
        RestoredPunctuation::Period => ".",
        RestoredPunctuation::Question => "?",
        RestoredPunctuation::Exclamation => "!",
        RestoredPunctuation::None => return,
    };
    *text = text
        .trim_end_matches(|ch: char| matches!(ch, ',' | ';' | ':' | '.' | '!' | '?'))
        .to_string();
    text.push_str(mark);
}

fn collect_pending_boundary_indexes(preferences: &[BoundaryPreference]) -> Vec<usize> {
    preferences
        .iter()
        .enumerate()
        .filter_map(|(index, pref)| {
            (pref.rule_decision == BoundaryDecisionKind::Unknown).then_some(index)
        })
        .collect()
}

fn boundary_preferences_equivalent(
    left: &[BoundaryPreference],
    right: &[BoundaryPreference],
) -> bool {
    left.len() == right.len()
        && left.iter().zip(right.iter()).all(|(a, b)| {
            a.rule_decision == b.rule_decision
                && a.llm_decision == b.llm_decision
                && a.reason_tag == b.reason_tag
        })
}

fn choose_majority_punctuation(votes: &[RestoredPunctuation]) -> Option<RestoredPunctuation> {
    let mut period = 0usize;
    let mut question = 0usize;
    let mut exclamation = 0usize;
    for vote in votes {
        match vote {
            RestoredPunctuation::Period => period += 1,
            RestoredPunctuation::Question => question += 1,
            RestoredPunctuation::Exclamation => exclamation += 1,
            RestoredPunctuation::None => {}
        }
    }
    let best = period.max(question).max(exclamation);
    if best == 0 {
        None
    } else if question == best && question > period && question >= exclamation {
        Some(RestoredPunctuation::Question)
    } else if exclamation == best && exclamation > period && exclamation >= question {
        Some(RestoredPunctuation::Exclamation)
    } else {
        Some(RestoredPunctuation::Period)
    }
}

fn combine_boundary_votes(votes: &[BoundaryVote]) -> BoundaryVoteSummary {
    build_boundary_vote_summary(votes, "restored_punctuation")
}

fn combine_refine_boundary_votes(votes: &[BoundaryVote]) -> BoundaryVoteSummary {
    build_boundary_vote_summary(votes, "sentence_refine")
}

fn build_boundary_vote_summary(votes: &[BoundaryVote], reason_prefix: &str) -> BoundaryVoteSummary {
    if votes.is_empty() {
        return BoundaryVoteSummary {
            decision: BoundaryDecisionKind::Split,
            confidence: UNKNOWN_BOUNDARY_CONFIDENCE,
            reason_tag: format!("{reason_prefix}:NONE"),
            split_score: 0.0,
            merge_score: 0.0,
            unsure_score: 0.0,
            total_weight: 0.0,
        };
    }

    let mut split_score = 0.0;
    let mut merge_score = 0.0;
    let mut unsure_score = 0.0;
    let mut period_score = 0.0;
    let mut question_score = 0.0;
    let mut exclamation_score = 0.0;
    let mut total_weight = 0.0;
    for vote in votes {
        let scaled = vote.confidence.max(0.1) * vote.weight.max(0.1);
        total_weight += vote.weight.max(0.1);
        match vote.decision {
            BoundaryDecisionKind::Split => {
                split_score += scaled;
                match vote.punctuation {
                    RestoredPunctuation::Period => period_score += scaled,
                    RestoredPunctuation::Question => question_score += scaled,
                    RestoredPunctuation::Exclamation => exclamation_score += scaled,
                    RestoredPunctuation::None => {}
                }
            }
            BoundaryDecisionKind::Merge => merge_score += scaled,
            BoundaryDecisionKind::Unsure | BoundaryDecisionKind::Unknown => unsure_score += scaled,
            BoundaryDecisionKind::HardSplit => {}
        }
    }

    if split_score >= merge_score && split_score >= unsure_score {
        let punctuation = if question_score >= period_score && question_score >= exclamation_score {
            "QUESTION"
        } else if exclamation_score >= period_score && exclamation_score >= question_score {
            "EXCLAMATION"
        } else {
            "PERIOD"
        };
        BoundaryVoteSummary {
            decision: BoundaryDecisionKind::Split,
            confidence: (split_score / total_weight.max(1.0)).clamp(0.0, 1.0),
            reason_tag: format!("{reason_prefix}:{punctuation}"),
            split_score,
            merge_score,
            unsure_score,
            total_weight,
        }
    } else if merge_score > split_score && merge_score >= unsure_score + 0.15 {
        BoundaryVoteSummary {
            decision: BoundaryDecisionKind::Merge,
            confidence: (merge_score / total_weight.max(1.0)).clamp(0.0, 1.0),
            reason_tag: format!("{reason_prefix}:NONE"),
            split_score,
            merge_score,
            unsure_score,
            total_weight,
        }
    } else {
        BoundaryVoteSummary {
            decision: BoundaryDecisionKind::Split,
            confidence: UNKNOWN_BOUNDARY_CONFIDENCE,
            reason_tag: format!("{reason_prefix}:PERIOD"),
            split_score,
            merge_score,
            unsure_score,
            total_weight,
        }
    }
}

fn build_translation_sentences_from_dp(
    micro_chunks: &[MicroChunk],
    preferences: &[BoundaryPreference],
) -> Vec<SourceSentence> {
    if micro_chunks.is_empty() {
        return Vec::new();
    }

    let regions = build_regions(micro_chunks);
    let mut sentences = Vec::new();

    for (region_start, region_end) in regions {
        let region_chunks = &micro_chunks[region_start..=region_end];
        let region_preferences = if region_chunks.len() > 1 {
            &preferences[region_start..region_end]
        } else {
            &[]
        };
        let spans = solve_region_sentences(region_chunks, region_preferences);
        for (local_start, local_end) in spans {
            let global_start = region_start + local_start;
            let global_end = region_start + local_end;
            let base = build_sentence_from_chunks(0, &micro_chunks[global_start..=global_end]);
            sentences.push(restore_sentence_terminal_punctuation(
                base,
                micro_chunks,
                preferences,
            ));
        }
    }

    for (index, sentence) in sentences.iter_mut().enumerate() {
        sentence.sentence_id = index + 1;
    }
    sentences
}

fn solve_region_sentences(
    region_chunks: &[MicroChunk],
    preferences: &[BoundaryPreference],
) -> Vec<(usize, usize)> {
    let n = region_chunks.len();
    if n == 0 {
        return Vec::new();
    }

    let mut dp = vec![f64::INFINITY; n + 1];
    let mut back = vec![0usize; n + 1];
    dp[0] = 0.0;

    for i in 1..=n {
        let min_j = i.saturating_sub(DP_MAX_ATOMS_PER_SENTENCE);
        for j in min_j..i {
            let span = &region_chunks[j..i];
            let mut cost = dp[j];
            cost += sentence_shape_cost(span, i < n);
            let prev_pref = if j > 0 { preferences.get(j - 1) } else { None };
            let next_pref = if i < n { preferences.get(i - 1) } else { None };
            cost += sentence_signal_shape_adjustment(span, prev_pref, next_pref);
            for boundary_index in j..i.saturating_sub(1) {
                cost += merge_penalty(&preferences[boundary_index]);
            }
            if i < n {
                cost += split_penalty(&preferences[i - 1]);
            }
            if cost < dp[i] {
                dp[i] = cost;
                back[i] = j;
            }
        }
    }

    let mut spans = Vec::new();
    let mut cursor = n;
    while cursor > 0 {
        let start = back[cursor];
        spans.push((start, cursor - 1));
        cursor = start;
    }
    spans.reverse();
    spans
}

fn sentence_shape_cost(span: &[MicroChunk], has_next: bool) -> f64 {
    let words = span
        .iter()
        .map(|chunk| count_display_words(&chunk.text))
        .sum::<usize>();
    let mut cost = 0.0;

    if words <= 1 && has_next {
        cost += 8.0;
    } else if words == 2 && has_next {
        cost += 4.0;
    } else if words == 3 && has_next {
        cost += 1.8;
    } else if words < 4 && has_next {
        cost += 0.9;
    } else if words > 36 {
        cost += 0.2 + (words.saturating_sub(36) as f64) * 0.08;
    } else if (6..=18).contains(&words) {
        cost -= 0.25;
    }

    if span
        .last()
        .map(|chunk| ends_with_terminal_punctuation(&chunk.text))
        .unwrap_or(false)
    {
        cost -= 0.55;
    } else if has_next {
        cost += 0.25;
    }

    cost.max(-1.0)
}

fn sentence_signal_shape_adjustment(
    span: &[MicroChunk],
    prev_pref: Option<&BoundaryPreference>,
    next_pref: Option<&BoundaryPreference>,
) -> f64 {
    let words = span
        .iter()
        .map(|chunk| count_display_words(&chunk.text))
        .sum::<usize>();
    let bounded_by_strong_splits =
        usize::from(prev_pref.is_some_and(is_strong_terminal_split_signal))
            + usize::from(next_pref.is_some_and(is_strong_terminal_split_signal));

    if bounded_by_strong_splits < 2 {
        return 0.0;
    }

    match words {
        0 | 1 => -3.0,
        2 => -4.0,
        3 => -1.5,
        _ => 0.0,
    }
}

fn merge_penalty(pref: &BoundaryPreference) -> f64 {
    match pref.rule_decision {
        BoundaryDecisionKind::HardSplit => LARGE_PENALTY,
        _ if is_llm_error_fallback(pref) => 0.08,
        _ => match pref.llm_decision {
            BoundaryDecisionKind::Merge => 0.02,
            BoundaryDecisionKind::Split => {
                if is_strong_terminal_split_signal(pref) {
                    4.8 + 5.2 * pref.confidence
                } else if pref.rule_decision == BoundaryDecisionKind::Split {
                    1.0 + 1.8 * pref.confidence
                } else {
                    1.4 + 2.6 * pref.confidence
                }
            }
            BoundaryDecisionKind::Unsure | BoundaryDecisionKind::Unknown => 0.25,
            BoundaryDecisionKind::HardSplit => LARGE_PENALTY,
        },
    }
}

fn split_penalty(pref: &BoundaryPreference) -> f64 {
    match pref.rule_decision {
        BoundaryDecisionKind::HardSplit => 0.0,
        _ if is_llm_error_fallback(pref) => 0.9,
        _ => match pref.llm_decision {
            BoundaryDecisionKind::Merge => {
                if pref.rule_decision == BoundaryDecisionKind::Split {
                    1.2 + 2.8 * pref.confidence
                } else {
                    1.8 + 3.2 * pref.confidence
                }
            }
            BoundaryDecisionKind::Split => {
                if is_strong_terminal_split_signal(pref) {
                    0.0
                } else {
                    0.02
                }
            }
            BoundaryDecisionKind::Unsure | BoundaryDecisionKind::Unknown => {
                if pref.rule_decision == BoundaryDecisionKind::Split {
                    0.1
                } else {
                    0.2
                }
            }
            BoundaryDecisionKind::HardSplit => 0.0,
        },
    }
}

fn finalize_boundaries(
    micro_chunks: &[MicroChunk],
    preferences: &[BoundaryPreference],
    translation_sentences: &[SourceSentence],
) -> Vec<BoundaryDecision> {
    if micro_chunks.len() < 2 {
        return Vec::new();
    }

    let split_after_chunk = translation_sentences
        .iter()
        .map(|sentence| sentence.chunk_end)
        .collect::<std::collections::HashSet<_>>();

    (0..micro_chunks.len() - 1)
        .map(|index| {
            let left = &micro_chunks[index];
            let right = &micro_chunks[index + 1];
            let pref = &preferences[index];
            let final_decision = if split_after_chunk.contains(&left.chunk_id) {
                if pref.rule_decision == BoundaryDecisionKind::HardSplit {
                    BoundaryDecisionKind::HardSplit
                } else {
                    BoundaryDecisionKind::Split
                }
            } else {
                BoundaryDecisionKind::Merge
            };
            BoundaryDecision {
                left_chunk_id: left.chunk_id,
                right_chunk_id: right.chunk_id,
                gap_ms: left.gap_after_ms,
                rule_decision: pref.rule_decision,
                llm_decision: pref.llm_decision,
                final_decision,
                confidence: pref.confidence,
                reason_tag: pref.reason_tag.clone(),
            }
        })
        .collect()
}

fn build_regions(micro_chunks: &[MicroChunk]) -> Vec<(usize, usize)> {
    if micro_chunks.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut start = 0usize;
    for index in 0..micro_chunks.len().saturating_sub(1) {
        if micro_chunks[index].gap_after_ms >= HARD_SPLIT_GAP_MS {
            out.push((start, index));
            start = index + 1;
        }
    }
    out.push((start, micro_chunks.len() - 1));
    out
}

fn build_sentence_from_chunks(sentence_id: usize, chunks: &[MicroChunk]) -> SourceSentence {
    let first = chunks.first().expect("sentence must have chunks");
    let last = chunks.last().expect("sentence must have chunks");
    SourceSentence {
        sentence_id,
        start_ms: first.start_ms,
        end_ms: last.end_ms,
        text: join_words(chunks.iter().map(|chunk| chunk.text.as_str())),
        word_start: first.word_start,
        word_end: last.word_end,
        chunk_start: first.chunk_id,
        chunk_end: last.chunk_id,
    }
}

fn is_cjk_char(ch: char) -> bool {
    matches!(
        ch as u32,
        0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0x3040..=0x309F
            | 0x30A0..=0x30FF
            | 0xAC00..=0xD7AF
            | 0xF900..=0xFAFF
    )
}

fn restore_sentence_terminal_punctuation(
    mut sentence: SourceSentence,
    micro_chunks: &[MicroChunk],
    preferences: &[BoundaryPreference],
) -> SourceSentence {
    if ends_with_terminal_punctuation(&sentence.text) {
        return sentence;
    }

    let punctuation = preferences
        .get(sentence.chunk_end.saturating_sub(1))
        .and_then(|pref| {
            if pref.left_chunk_id == sentence.chunk_end && carries_terminal_punctuation_signal(pref)
            {
                Some(reason_tag_to_punctuation(&pref.reason_tag))
            } else {
                None
            }
        })
        .or_else(|| {
            let is_last_chunk = sentence.chunk_end >= micro_chunks.len();
            is_last_chunk.then_some(".")
        });

    if let Some(mark) = punctuation {
        sentence.text = sentence
            .text
            .trim_end_matches(|ch: char| matches!(ch, ',' | ';' | ':' | '.' | '!' | '?'))
            .to_string();
        sentence.text.push_str(mark);
    }
    sentence
}

fn reason_tag_to_punctuation(reason_tag: &str) -> &'static str {
    match reason_tag {
        "restored_punctuation:QUESTION" | "sentence_refine:QUESTION" => "?",
        "restored_punctuation:EXCLAMATION" | "sentence_refine:EXCLAMATION" => "!",
        _ => ".",
    }
}

fn carries_terminal_punctuation_signal(pref: &BoundaryPreference) -> bool {
    matches!(
        pref.reason_tag.as_str(),
        "terminal_punctuation"
            | "hard_pause"
            | "restored_punctuation:PERIOD"
            | "restored_punctuation:QUESTION"
            | "restored_punctuation:EXCLAMATION"
            | "sentence_refine:PERIOD"
            | "sentence_refine:QUESTION"
            | "sentence_refine:EXCLAMATION"
    )
}

fn is_strong_terminal_split_signal(pref: &BoundaryPreference) -> bool {
    pref.llm_decision == BoundaryDecisionKind::Split
        && pref.confidence >= 0.88
        && carries_terminal_punctuation_signal(pref)
}

fn build_boundary_window_prompt(task: &BoundaryWindowTask) -> String {
    match task.mode {
        BoundaryWindowMode::Sweep => build_restore_text_window_prompt(
            &task.chunks,
            &task.source_lang,
            "restore_sentence_end_punctuation_for_window",
            "Restore sentence-ending punctuation inside this short speech window while preserving exact tokens.",
            "Mark every token that clearly ends a complete sentence.",
        ),
        BoundaryWindowMode::Focus => build_boundary_focus_prompt(task),
    }
}

fn build_boundary_focus_prompt(task: &BoundaryWindowTask) -> String {
    let atoms = task
        .chunks
        .iter()
        .map(|chunk| {
            serde_json::json!({
                "chunkId": chunk.chunk_id,
                "text": chunk.text,
                "gapAfterMs": chunk.gap_after_ms,
            })
        })
        .collect::<Vec<_>>();
    let window_text = join_words(task.chunks.iter().map(|chunk| chunk.text.as_str()));
    let focus_boundary = task
        .focus_boundary_index
        .and_then(|boundary_index| {
            let local_index = boundary_index.checked_sub(task.chunk_start_index)?;
            let left = task.chunks.get(local_index)?;
            let right = task.chunks.get(local_index + 1)?;
            Some(serde_json::json!({
                "leftChunkId": left.chunk_id,
                "leftText": left.text,
                "rightChunkId": right.chunk_id,
                "rightText": right.text,
                "gapMs": left.gap_after_ms,
            }))
        })
        .unwrap_or(serde_json::json!(null));
    let payload = serde_json::json!({
        "task": "restore_sentence_end_punctuation_for_focus_boundary",
        "sourceLanguage": task.source_lang,
        "rule": "Return JSON only.",
        "goal": "Restore sentence-ending punctuation in this local speech window while paying extra attention to the focus boundary.",
        "windowText": window_text,
        "focusBoundary": focus_boundary,
        "policy": [
            "Treat the text as plain speech text. Do not rely on casing or formatting.",
            "Preserve the exact token order and token content.",
            "Do not add, remove, replace, or reorder tokens.",
            "Only attach sentence-ending punctuation directly to existing tokens.",
            "Pay special attention to whether the focus boundary stays inside one sentence or starts a new complete sentence.",
            "If the right side clearly starts a new complete sentence or utterance, attach sentence-ending punctuation to the left token.",
            "If the right side clearly continues the same thought, do not place a sentence end on the left token.",
            "Still judge the whole window coherently. You may add sentence endings away from the focus boundary if they are clearly needed.",
            "Do not place a sentence end where the left side still feels unfinished.",
            "Respect very long pauses: if a gapAfterMs is 2000 or more, that point must stay as a sentence end.",
            "Use PERIOD by default. Use QUESTION only when the wording is clearly a question. Use EXCLAMATION only for unmistakably strong exclamations or commands."
        ],
        "atoms": atoms,
        "output": {
            "restoredText": window_text
        }
    });
    payload.to_string()
}

fn is_llm_error_fallback(pref: &BoundaryPreference) -> bool {
    pref.reason_tag.starts_with("llm_error:")
}

fn is_high_confidence_punctuation_split(chunk: &MicroChunk) -> bool {
    ends_with_terminal_punctuation(&chunk.text)
        && count_display_words(&chunk.text) >= HIGH_CONF_PUNCT_SPLIT_MIN_WORDS
}

fn count_display_words(text: &str) -> usize {
    text.split_whitespace()
        .filter(|part| !part.is_empty())
        .count()
        .max(1)
}

fn has_internal_sentence_punctuation(text: &str) -> bool {
    let trimmed = text.trim();
    let without_terminal =
        trimmed.trim_end_matches(|ch: char| matches!(ch, '.' | '!' | '?' | '。' | '！' | '？'));
    without_terminal
        .chars()
        .any(|ch| matches!(ch, '.' | '!' | '?' | '。' | '！' | '？'))
}

fn ends_with_terminal_punctuation(word: &str) -> bool {
    word.trim_end()
        .chars()
        .last()
        .map(|ch| matches!(ch, '.' | '!' | '?' | '。' | '！' | '？'))
        .unwrap_or(false)
}

fn join_words<'a>(parts: impl Iterator<Item = &'a str>) -> String {
    let mut out = String::new();
    let mut prev_has_ascii_word = false;

    for raw in parts {
        let token = raw.trim();
        if token.is_empty() {
            continue;
        }
        let next_has_ascii_word = token_has_ascii_word(token);
        if !out.is_empty() && prev_has_ascii_word && next_has_ascii_word {
            out.push(' ');
        }
        out.push_str(token);
        prev_has_ascii_word = next_has_ascii_word;
    }

    out.replace(" ,", ",")
        .replace(" .", ".")
        .replace(" !", "!")
        .replace(" ?", "?")
        .replace(" :", ":")
        .replace(" ;", ";")
}

fn token_has_ascii_word(token: &str) -> bool {
    token.chars().any(|ch| ch.is_ascii_alphanumeric())
}

fn gap_ms(left_end_sec: f64, right_start_sec: f64) -> u64 {
    ((right_start_sec - left_end_sec).max(0.0) * 1000.0).round() as u64
}

fn to_core_words(words: Vec<WordTokenDto>) -> Vec<WordToken> {
    words
        .into_iter()
        .map(|word| WordToken {
            start: word.start,
            end: word.end,
            word: word.word,
        })
        .collect()
}

fn from_core_words(words: Vec<WordToken>) -> Vec<WordTokenDto> {
    words
        .into_iter()
        .map(|word| WordTokenDto {
            start: word.start,
            end: word.end,
            word: word.word,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        ATOM_MAX_WORDS, BoundaryDecisionKind, BoundaryPreference, HARD_SPLIT_GAP_MS,
        SourceSentence, build_micro_chunks, count_display_words, ends_with_terminal_punctuation,
        finalize_boundaries, sentence_shape_cost, solve_region_sentences,
    };
    use crate::services::transcribe::WordTokenDto;

    #[test]
    fn hard_pause_forces_micro_chunk_boundary() {
        let words = vec![
            WordTokenDto {
                start: 0.0,
                end: 0.2,
                word: "Hello".to_string(),
            },
            WordTokenDto {
                start: 2.4,
                end: 2.7,
                word: "world".to_string(),
            },
        ];

        let chunks = build_micro_chunks(&words);
        assert_eq!(chunks.len(), 2);
        assert!(chunks[0].hard_split_after);
        assert_eq!(chunks[0].gap_after_ms, HARD_SPLIT_GAP_MS + 200);
    }

    #[test]
    fn medium_pause_can_split_without_terminal_punctuation() {
        let words = vec![
            WordTokenDto {
                start: 0.0,
                end: 0.2,
                word: "Everybody".to_string(),
            },
            WordTokenDto {
                start: 0.21,
                end: 0.4,
                word: "has".to_string(),
            },
            WordTokenDto {
                start: 0.41,
                end: 0.7,
                word: "problems".to_string(),
            },
            WordTokenDto {
                start: 1.2,
                end: 1.4,
                word: "even".to_string(),
            },
            WordTokenDto {
                start: 1.41,
                end: 1.6,
                word: "you".to_string(),
            },
        ];

        let chunks = build_micro_chunks(&words);
        assert_eq!(chunks.len(), 5);
        assert_eq!(chunks[0].text, "Everybody");
        assert_eq!(chunks[3].text, "even");
        assert_eq!(chunks[4].text, "you");
    }

    #[test]
    fn punctuation_still_closes_atom_when_available() {
        let words = vec![
            WordTokenDto {
                start: 0.0,
                end: 0.2,
                word: "God".to_string(),
            },
            WordTokenDto {
                start: 0.21,
                end: 0.4,
                word: "bless".to_string(),
            },
            WordTokenDto {
                start: 0.41,
                end: 0.7,
                word: "you.".to_string(),
            },
            WordTokenDto {
                start: 0.72,
                end: 0.9,
                word: "Remember".to_string(),
            },
            WordTokenDto {
                start: 0.91,
                end: 1.1,
                word: "this".to_string(),
            },
        ];

        let chunks = build_micro_chunks(&words);
        assert_eq!(chunks.len(), 5);
        assert_eq!(chunks[2].text, "you.");
        assert!(ends_with_terminal_punctuation(&chunks[2].text));
        assert_eq!(chunks[3].text, "Remember");
    }

    #[test]
    fn long_run_without_punctuation_is_cut_into_smaller_atoms() {
        let words = (0..18)
            .map(|index| WordTokenDto {
                start: index as f64 * 0.22,
                end: index as f64 * 0.22 + 0.18,
                word: format!("w{index}"),
            })
            .collect::<Vec<_>>();

        let chunks = build_micro_chunks(&words);
        assert!(chunks.len() >= 2);
        assert!(
            chunks
                .iter()
                .all(|chunk| count_display_words(&chunk.text) <= ATOM_MAX_WORDS)
        );
    }

    #[test]
    fn dp_prefers_merge_across_non_terminal_boundary() {
        let chunks = vec![
            super::MicroChunk {
                chunk_id: 1,
                start_ms: 0,
                end_ms: 600,
                text: "Offer some words".to_string(),
                word_start: 0,
                word_end: 2,
                gap_before_ms: 0,
                gap_after_ms: 80,
                hard_split_before: false,
                hard_split_after: false,
            },
            super::MicroChunk {
                chunk_id: 2,
                start_ms: 680,
                end_ms: 1300,
                text: "of appreciation.".to_string(),
                word_start: 3,
                word_end: 4,
                gap_before_ms: 80,
                gap_after_ms: 0,
                hard_split_before: false,
                hard_split_after: false,
            },
        ];
        let prefs = vec![BoundaryPreference {
            left_chunk_id: 1,
            right_chunk_id: 2,
            gap_ms: 80,
            rule_decision: BoundaryDecisionKind::Unknown,
            llm_decision: BoundaryDecisionKind::Merge,
            confidence: 0.9,
            reason_tag: "right_dependent".to_string(),
        }];
        let spans = solve_region_sentences(&chunks, &prefs);
        assert_eq!(spans, vec![(0, 1)]);
    }

    #[test]
    fn finalize_boundaries_marks_sentence_ends() {
        let chunks = vec![
            super::MicroChunk {
                chunk_id: 1,
                start_ms: 0,
                end_ms: 500,
                text: "Offer some words".to_string(),
                word_start: 0,
                word_end: 2,
                gap_before_ms: 0,
                gap_after_ms: 120,
                hard_split_before: false,
                hard_split_after: false,
            },
            super::MicroChunk {
                chunk_id: 2,
                start_ms: 620,
                end_ms: 1100,
                text: "of appreciation".to_string(),
                word_start: 3,
                word_end: 4,
                gap_before_ms: 120,
                gap_after_ms: 80,
                hard_split_before: false,
                hard_split_after: false,
            },
            super::MicroChunk {
                chunk_id: 3,
                start_ms: 1180,
                end_ms: 1600,
                text: "for them.".to_string(),
                word_start: 5,
                word_end: 6,
                gap_before_ms: 80,
                gap_after_ms: 0,
                hard_split_before: false,
                hard_split_after: false,
            },
        ];
        let prefs = vec![
            BoundaryPreference {
                left_chunk_id: 1,
                right_chunk_id: 2,
                gap_ms: 120,
                rule_decision: BoundaryDecisionKind::Unknown,
                llm_decision: BoundaryDecisionKind::Merge,
                confidence: 0.9,
                reason_tag: "merge".to_string(),
            },
            BoundaryPreference {
                left_chunk_id: 2,
                right_chunk_id: 3,
                gap_ms: 80,
                rule_decision: BoundaryDecisionKind::Unknown,
                llm_decision: BoundaryDecisionKind::Split,
                confidence: 0.9,
                reason_tag: "split".to_string(),
            },
        ];
        let sentences = vec![
            SourceSentence {
                sentence_id: 1,
                start_ms: 0,
                end_ms: 1100,
                text: "Offer some words of appreciation".to_string(),
                word_start: 0,
                word_end: 4,
                chunk_start: 1,
                chunk_end: 2,
            },
            SourceSentence {
                sentence_id: 2,
                start_ms: 1180,
                end_ms: 1600,
                text: "for them.".to_string(),
                word_start: 5,
                word_end: 6,
                chunk_start: 3,
                chunk_end: 3,
            },
        ];

        let boundaries = finalize_boundaries(&chunks, &prefs, &sentences);
        assert_eq!(boundaries.len(), 2);
        assert_eq!(boundaries[0].final_decision, BoundaryDecisionKind::Merge);
        assert_eq!(boundaries[1].final_decision, BoundaryDecisionKind::Split);
    }

    #[test]
    fn shape_cost_rewards_complete_sentence() {
        let low = sentence_shape_cost(
            &[super::MicroChunk {
                chunk_id: 1,
                start_ms: 0,
                end_ms: 2000,
                text: "Everybody has problems, even you.".to_string(),
                word_start: 0,
                word_end: 4,
                gap_before_ms: 0,
                gap_after_ms: 0,
                hard_split_before: false,
                hard_split_after: false,
            }],
            true,
        );
        let high = sentence_shape_cost(
            &[super::MicroChunk {
                chunk_id: 1,
                start_ms: 0,
                end_ms: 500,
                text: "and".to_string(),
                word_start: 0,
                word_end: 0,
                gap_before_ms: 0,
                gap_after_ms: 0,
                hard_split_before: false,
                hard_split_after: false,
            }],
            true,
        );
        assert!(low < high);
    }

    #[test]
    fn llm_error_unsure_biases_toward_merge() {
        let chunks = vec![
            super::MicroChunk {
                chunk_id: 1,
                start_ms: 0,
                end_ms: 900,
                text: "If you didn't have our".to_string(),
                word_start: 0,
                word_end: 4,
                gap_before_ms: 0,
                gap_after_ms: 120,
                hard_split_before: false,
                hard_split_after: false,
            },
            super::MicroChunk {
                chunk_id: 2,
                start_ms: 1020,
                end_ms: 1900,
                text: "military equipment".to_string(),
                word_start: 5,
                word_end: 6,
                gap_before_ms: 120,
                gap_after_ms: 0,
                hard_split_before: false,
                hard_split_after: false,
            },
        ];
        let prefs = vec![BoundaryPreference {
            left_chunk_id: 1,
            right_chunk_id: 2,
            gap_ms: 120,
            rule_decision: BoundaryDecisionKind::Unknown,
            llm_decision: BoundaryDecisionKind::Unsure,
            confidence: 0.2,
            reason_tag: "llm_error:invalid_json".to_string(),
        }];

        let spans = solve_region_sentences(&chunks, &prefs);
        assert_eq!(spans, vec![(0, 1)]);
    }

    #[test]
    fn llm_merge_beats_long_sentence_shape_penalty() {
        let chunks = vec![
            super::MicroChunk {
                chunk_id: 34,
                start_ms: 76_400,
                end_ms: 81_840,
                text: "Offer some words of appreciation for the United States of America and the president who's trying".to_string(),
                word_start: 241,
                word_end: 256,
                gap_before_ms: 0,
                gap_after_ms: 0,
                hard_split_before: false,
                hard_split_after: false,
            },
            super::MicroChunk {
                chunk_id: 35,
                start_ms: 81_840,
                end_ms: 84_720,
                text: "to save your country.".to_string(),
                word_start: 257,
                word_end: 260,
                gap_before_ms: 0,
                gap_after_ms: 0,
                hard_split_before: false,
                hard_split_after: false,
            },
        ];
        let prefs = vec![BoundaryPreference {
            left_chunk_id: 34,
            right_chunk_id: 35,
            gap_ms: 0,
            rule_decision: BoundaryDecisionKind::Unknown,
            llm_decision: BoundaryDecisionKind::Merge,
            confidence: 0.5,
            reason_tag: "unclear".to_string(),
        }];

        let spans = solve_region_sentences(&chunks, &prefs);
        assert_eq!(spans, vec![(0, 1)]);
    }
}
