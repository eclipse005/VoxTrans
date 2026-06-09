use std::sync::Arc;

use super::translate_llm_settings::hydrate_translate_llm_connection_settings;
use super::translate_terms::normalize_command_terminology_entries;
use super::translate_types::{
    BuildTranslationLayerCommandRequest, BuildTranslationLayerCommandResponse,
    BuildTranslationSegmentCommand, SegmentTokenForTerminologyCommand,
};
use crate::db::store::TaskStore;
use tauri::{AppHandle, Manager};

pub async fn build_translation_layer_with_progress_and_unit_store(
    app: AppHandle,
    mut request: BuildTranslationLayerCommandRequest,
    on_progress: Option<Arc<dyn Fn(usize, usize) + Send + Sync>>,
    unit_store: Option<crate::services::pipeline::UnitStore>,
) -> Result<BuildTranslationLayerCommandResponse, String> {
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

    let store = app.state::<TaskStore>().inner();
    hydrate_translate_llm_connection_settings(
        store,
        &mut request.translate_api_key,
        &mut request.translate_base_url,
        &mut request.translate_model,
    )?;
    request.llm_concurrency = request.llm_concurrency.max(1);

    let terminology_entries = normalize_command_terminology_entries(request.terminology_entries);
    let theme_summary = request.theme_summary.trim().to_string();
    let service_request = crate::services::translation::BuildTranslationLayerRequest {
        task_id: request.task_id.clone(),
        media_path: request.media_path.clone(),
        source_lang: request.source_lang.clone(),
        target_lang: request.target_lang.clone(),
        segments: request
            .segments
            .iter()
            .map(
                |segment| crate::services::translation::TranslationSegmentInput {
                    segment: segment.segment.clone(),
                    start: segment.start,
                    end: segment.end,
                    tokens: segment
                        .tokens
                        .iter()
                        .map(|token| crate::services::translation::TranslationToken {
                            text: token.text.clone(),
                            start: token.start,
                            end: token.end,
                        })
                        .collect(),
                },
            )
            .collect(),
        theme_summary: theme_summary.clone(),
        terminology_entries: terminology_entries
            .iter()
            .map(
                |entry| crate::services::translation::TranslationTerminologyEntry {
                    source: entry.source.clone(),
                    target: entry.target.clone(),
                    note: entry.note.clone(),
                },
            )
            .collect(),
        translate_api_key: request.translate_api_key,
        translate_base_url: request.translate_base_url,
        translate_model: request.translate_model,
        llm_concurrency: request.llm_concurrency,
        batch_size: request.batch_size,
        unit_store,
    };

    let service_response = crate::services::translation::build_translation_layer_with_progress(
        service_request,
        on_progress,
    )
    .await?;

    Ok(BuildTranslationLayerCommandResponse {
        task_id: request.task_id,
        media_path: request.media_path,
        source_lang: request.source_lang,
        target_lang: request.target_lang,
        batch_size: service_response.batch_size,
        batch_total: service_response.batch_total,
        segment_total: service_response.segment_total,
        theme_summary,
        terminology_entries,
        segments: service_response
            .segments
            .into_iter()
            .map(|segment| BuildTranslationSegmentCommand {
                segment_id: segment.segment_id,
                start: segment.start,
                end: segment.end,
                source: segment.source,
                translation: segment.translation,
                tokens: segment
                    .tokens
                    .into_iter()
                    .map(|token| SegmentTokenForTerminologyCommand {
                        text: token.text,
                        start: token.start,
                        end: token.end,
                    })
                    .collect(),
            })
            .collect(),
    })
}
