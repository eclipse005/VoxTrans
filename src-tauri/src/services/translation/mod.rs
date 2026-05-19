use std::collections::HashMap;
use std::sync::Arc;

use crate::services::llm::batch::run_indexed_concurrent_with_progress;
use crate::services::llm::client::OpenAiCompatLlmClient;
use crate::services::llm::port::{LlmCallContext, LlmConfig, LlmJsonTask, next_llm_request_id};

mod batches;
mod responses;
mod segments;
#[cfg(test)]
mod tests;
mod text;
mod types;

use batches::build_batch_windows;
use responses::validate_batch_translation_response;
use segments::{merge_dangling_source_segments, normalize_segments};
pub use types::{
    BuildTranslationLayerRequest, BuildTranslationLayerResponse, TranslationSegmentInput,
    TranslationSegmentOutput, TranslationTerminologyEntry, TranslationToken,
};

const DEFAULT_BATCH_SIZE: usize = 20;
const MAX_BATCH_SIZE: usize = 40;
const CONTEXT_LINE_LIMIT: usize = 6;
const MAX_TERMS_PER_BATCH: usize = 16;

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
    ?;

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
                            |value| validate_batch_translation_response(value, &window.local_ids),
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
