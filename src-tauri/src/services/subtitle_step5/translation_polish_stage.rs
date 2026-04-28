use std::sync::Arc;

use crate::services::llm::batch::run_indexed_concurrent_with_progress;
use crate::services::llm::client::OpenAiCompatLlmClient;
use crate::services::llm::port::{LlmCallContext, LlmConfig, LlmJsonTask, next_llm_request_id};
use crate::services::prompts::subtitle_step5::{Step5PromptTerm, build_polish_prompt};

use super::constants::{
    DEFAULT_BATCH_SIZE, LONG_LINE_SCORE_TRIGGER, MAX_BATCH_SIZE, MAX_TERMS_PER_LINE,
};
use super::language_units::text_length_units;
use super::numbers::extract_numbers;
use super::polish_postprocess::postprocess_polished_segments;
use super::request_validation::validate_step5_polish_request;
use super::responses::validate_polish_response;
use super::source_residue::looks_like_non_cjk_translation_for_cjk_target;
use super::terminology_filter::select_terms_for_text;
use super::text_utils::normalize_inline_text;
use super::types::{
    BuildStep5TranslationPolishRequest, BuildStep5TranslationPolishResponse, Step5FinalSegment,
};
pub async fn build_step_5_3_translation_polish_with_progress(
    request: BuildStep5TranslationPolishRequest,
    on_progress: Option<Arc<dyn Fn(usize, usize) + Send + Sync>>,
) -> Result<BuildStep5TranslationPolishResponse, String> {
    validate_step5_polish_request(&request)?;

    let llm_client = OpenAiCompatLlmClient::new(LlmConfig::new(
        request.translate_base_url.clone(),
        request.translate_api_key.clone(),
        request.translate_model.clone(),
    ))
    .map_err(|err| err.message)?;

    let subtitle_length_reference = request.subtitle_length_reference.clamp(8, 80) as f64;
    let mut segments = Vec::<Step5FinalSegment>::new();
    for parent in &request.parents {
        for part in &parent.parts {
            segments.push(Step5FinalSegment {
                segment_id: segments.len() + 1,
                start: part.start,
                end: part.end.max(part.start),
                source: normalize_inline_text(&part.source),
                translation: normalize_inline_text(&part.translation),
                tokens: part.tokens.clone(),
            });
        }
    }
    let baseline_translations = segments
        .iter()
        .map(|segment| segment.translation.clone())
        .collect::<Vec<_>>();

    let polish_length_trigger = (subtitle_length_reference as f64).min(LONG_LINE_SCORE_TRIGGER);
    let mut polish_candidates = Vec::<usize>::new();
    for (index, segment) in segments.iter().enumerate() {
        let target_len = text_length_units(&segment.translation, &request.target_lang);
        if target_len > polish_length_trigger {
            polish_candidates.push(index);
        }
    }

    if !polish_candidates.is_empty() {
        let tasks = polish_candidates
            .iter()
            .enumerate()
            .map(|(task_id, segment_index)| {
                let segment = &segments[*segment_index];
                let terms = select_terms_for_text(
                    &segment.source,
                    &request.terminology_entries,
                    MAX_TERMS_PER_LINE,
                );
                let prompt_terms = terms
                    .iter()
                    .map(|term| Step5PromptTerm {
                        source: term.source.clone(),
                        target: term.target.clone(),
                        note: term.note.clone(),
                    })
                    .collect::<Vec<_>>();
                let prompt = build_polish_prompt(
                    &request.source_lang,
                    &request.target_lang,
                    &segment.source,
                    &segment.translation,
                    subtitle_length_reference,
                    &prompt_terms,
                );
                LlmJsonTask {
                    id: task_id,
                    request_id: next_llm_request_id(),
                    user_prompt: prompt,
                    response_validator: None,
                }
            })
            .collect::<Vec<_>>();

        let context = LlmCallContext {
            task_id: request.task_id.clone(),
            media_path: Some(request.media_path.clone()),
            phase: "step_5_3_translation_polish".to_string(),
        };
        let candidate_indexes = polish_candidates.clone();
        let progress_callback = on_progress.clone();
        let results = run_indexed_concurrent_with_progress(
            tasks,
            request.llm_concurrency.max(1) as usize,
            {
                let llm_client = llm_client.clone();
                let context = context.clone();
                move |task| {
                    let llm_client = llm_client.clone();
                    let context = context.clone();
                    let indexes = candidate_indexes.clone();
                    async move {
                        let Some(segment_index) = indexes.get(task.id).copied() else {
                            return Err(format!("missing step5 polish task {}", task.id));
                        };
                        let llm_id = task.request_id.clone();
                        let call = llm_client
                            .call_json_validated(
                                &context,
                                &llm_id,
                                &task.user_prompt,
                                task.response_validator.as_ref(),
                                validate_polish_response,
                            )
                            .await
                            .map_err(|err| {
                                format!("step5 polish failed (llmId={}): {}", llm_id, err.message)
                            })?;
                        Ok((segment_index, call.value))
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

        for (_, result) in results {
            let Ok((segment_index, polished)) = result else {
                continue;
            };
            let Some(segment) = segments.get_mut(segment_index) else {
                continue;
            };
            let polished = normalize_inline_text(&polished);
            if polished.is_empty() {
                continue;
            }
            if looks_like_non_cjk_translation_for_cjk_target(&polished, &request.target_lang) {
                continue;
            }
            let source_numbers = extract_numbers(&segment.source);
            if !source_numbers.is_empty() {
                let polished_numbers = extract_numbers(&polished);
                let dropped_anchor = source_numbers
                    .iter()
                    .any(|value| !polished_numbers.contains(value));
                if dropped_anchor {
                    continue;
                }
            }
            if text_length_units(&polished, &request.target_lang)
                <= text_length_units(&segment.translation, &request.target_lang) * 1.02
            {
                segment.translation = polished;
            }
        }
    } else if let Some(callback) = on_progress.as_ref() {
        callback(1, 1);
    }
    postprocess_polished_segments(&mut segments, &baseline_translations, &request.target_lang);

    let batch_size = if request.batch_size == 0 {
        DEFAULT_BATCH_SIZE
    } else {
        request
            .batch_size
            .clamp(1, MAX_BATCH_SIZE)
            .max(DEFAULT_BATCH_SIZE.min(MAX_BATCH_SIZE))
    };
    let batch_total = if segments.is_empty() {
        0
    } else {
        (segments.len() + batch_size - 1) / batch_size
    };

    Ok(BuildStep5TranslationPolishResponse {
        batch_size,
        batch_total,
        segment_total: segments.len(),
        segments,
    })
}
