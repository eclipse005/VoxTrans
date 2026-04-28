use std::sync::Arc;

use super::translate_llm_settings::{
    hydrate_translate_llm_connection_settings, hydrate_translate_llm_settings,
};
use super::translate_quality::{summarize_step52_quality, summarize_step53_quality};
use super::translate_step5_common::{
    step5_artifact_meta, step5_pipeline_version_value, step5_schema_version_value,
    step51_quality_summary, validate_step5_command_base,
};
use super::translate_step5_mapping::{
    command_aligned_parents_to_step5, command_segments_to_step5_draft,
    command_split_parents_to_step5, command_terminology_to_step5, step5_aligned_parents_to_command,
    step5_final_segments_to_command, step5_split_parents_to_command,
};
use super::translate_terms::normalize_command_terminology_entries;
use super::translate_types::{
    BuildStep51SourceSplitCommandRequest, BuildStep51SourceSplitCommandResponse,
    BuildStep52TranslationAlignCommandRequest, BuildStep52TranslationAlignCommandResponse,
    BuildStep53TranslationPolishCommandRequest, BuildStep53TranslationPolishCommandResponse,
};

#[tauri::command]
pub async fn build_step_5_1_source_split(
    request: BuildStep51SourceSplitCommandRequest,
) -> Result<BuildStep51SourceSplitCommandResponse, String> {
    build_step_5_1_source_split_with_progress(request, None).await
}

pub async fn build_step_5_1_source_split_with_progress(
    mut request: BuildStep51SourceSplitCommandRequest,
    on_progress: Option<Arc<dyn Fn(usize, usize) + Send + Sync>>,
) -> Result<BuildStep51SourceSplitCommandResponse, String> {
    validate_step5_command_base(
        &request.task_id,
        &request.media_path,
        &request.source_lang,
        &request.target_lang,
    )?;
    if request.segments.is_empty() {
        return Err("segments is required".to_string());
    }

    hydrate_translate_llm_connection_settings(
        &mut request.translate_api_key,
        &mut request.translate_base_url,
        &mut request.translate_model,
    )?;
    request.llm_concurrency = request.llm_concurrency.max(1);

    let service_response =
        crate::services::subtitle_step5::build_step_5_1_source_split_with_progress(
            crate::services::subtitle_step5::BuildStep5SourceSplitRequest {
                task_id: request.task_id.clone(),
                media_path: request.media_path.clone(),
                source_lang: request.source_lang.clone(),
                target_lang: request.target_lang.clone(),
                segments: command_segments_to_step5_draft(&request.segments),
                subtitle_max_words_per_segment: request.subtitle_max_words_per_segment,
                subtitle_length_reference: request.subtitle_length_reference,
                translate_api_key: request.translate_api_key.clone(),
                translate_base_url: request.translate_base_url.clone(),
                translate_model: request.translate_model.clone(),
                llm_concurrency: request.llm_concurrency,
            },
            on_progress,
        )
        .await?;
    Ok(BuildStep51SourceSplitCommandResponse {
        task_id: request.task_id,
        media_path: request.media_path,
        source_lang: request.source_lang,
        target_lang: request.target_lang,
        schema_version: step5_schema_version_value(),
        pipeline_version: step5_pipeline_version_value(),
        artifact_meta: step5_artifact_meta(),
        quality_summary: step51_quality_summary(
            service_response.parent_total,
            service_response.part_total,
        ),
        subtitle_max_words_per_segment: service_response.subtitle_max_words_per_segment,
        subtitle_length_reference: service_response.subtitle_length_reference,
        parent_total: service_response.parent_total,
        part_total: service_response.part_total,
        parents: step5_split_parents_to_command(service_response.parents),
    })
}

#[tauri::command]
pub async fn build_step_5_2_translation_align(
    request: BuildStep52TranslationAlignCommandRequest,
) -> Result<BuildStep52TranslationAlignCommandResponse, String> {
    build_step_5_2_translation_align_with_progress(request, None).await
}

pub async fn build_step_5_2_translation_align_with_progress(
    mut request: BuildStep52TranslationAlignCommandRequest,
    on_progress: Option<Arc<dyn Fn(usize, usize) + Send + Sync>>,
) -> Result<BuildStep52TranslationAlignCommandResponse, String> {
    validate_step5_command_base(
        &request.task_id,
        &request.media_path,
        &request.source_lang,
        &request.target_lang,
    )?;
    if request.parents.is_empty() {
        return Err("parents is required".to_string());
    }

    hydrate_translate_llm_settings(
        &mut request.translate_api_key,
        &mut request.translate_base_url,
        &mut request.translate_model,
        &mut request.llm_concurrency,
    )?;

    let terminology_entries = normalize_command_terminology_entries(request.terminology_entries);
    let service_response =
        crate::services::subtitle_step5::build_step_5_2_translation_align_with_progress(
            crate::services::subtitle_step5::BuildStep5TranslationAlignRequest {
                task_id: request.task_id.clone(),
                media_path: request.media_path.clone(),
                source_lang: request.source_lang.clone(),
                target_lang: request.target_lang.clone(),
                theme_summary: request.theme_summary.trim().to_string(),
                terminology_entries: command_terminology_to_step5(&terminology_entries),
                parents: command_split_parents_to_step5(&request.parents),
                translate_api_key: request.translate_api_key,
                translate_base_url: request.translate_base_url.clone(),
                translate_model: request.translate_model.clone(),
                llm_concurrency: request.llm_concurrency,
            },
            on_progress,
        )
        .await?;
    Ok(BuildStep52TranslationAlignCommandResponse {
        task_id: request.task_id,
        media_path: request.media_path,
        source_lang: request.source_lang,
        target_lang: request.target_lang,
        schema_version: step5_schema_version_value(),
        pipeline_version: step5_pipeline_version_value(),
        artifact_meta: step5_artifact_meta(),
        quality_summary: summarize_step52_quality(&service_response.parents),
        theme_summary: request.theme_summary.trim().to_string(),
        terminology_entries,
        parent_total: service_response.parent_total,
        part_total: service_response.part_total,
        parents: step5_aligned_parents_to_command(service_response.parents),
    })
}

#[tauri::command]
pub async fn build_step_5_3_translation_polish(
    request: BuildStep53TranslationPolishCommandRequest,
) -> Result<BuildStep53TranslationPolishCommandResponse, String> {
    build_step_5_3_translation_polish_with_progress(request, None).await
}

pub async fn build_step_5_3_translation_polish_with_progress(
    mut request: BuildStep53TranslationPolishCommandRequest,
    on_progress: Option<Arc<dyn Fn(usize, usize) + Send + Sync>>,
) -> Result<BuildStep53TranslationPolishCommandResponse, String> {
    validate_step5_command_base(
        &request.task_id,
        &request.media_path,
        &request.source_lang,
        &request.target_lang,
    )?;
    if request.parents.is_empty() {
        return Err("parents is required".to_string());
    }

    hydrate_translate_llm_settings(
        &mut request.translate_api_key,
        &mut request.translate_base_url,
        &mut request.translate_model,
        &mut request.llm_concurrency,
    )?;

    let terminology_entries = normalize_command_terminology_entries(request.terminology_entries);
    let service_response =
        crate::services::subtitle_step5::build_step_5_3_translation_polish_with_progress(
            crate::services::subtitle_step5::BuildStep5TranslationPolishRequest {
                task_id: request.task_id.clone(),
                media_path: request.media_path.clone(),
                source_lang: request.source_lang.clone(),
                target_lang: request.target_lang.clone(),
                terminology_entries: command_terminology_to_step5(&terminology_entries),
                parents: command_aligned_parents_to_step5(&request.parents),
                translate_api_key: request.translate_api_key,
                translate_base_url: request.translate_base_url.clone(),
                translate_model: request.translate_model.clone(),
                llm_concurrency: request.llm_concurrency,
                subtitle_length_reference: request.subtitle_length_reference,
                batch_size: request.batch_size,
            },
            on_progress,
        )
        .await?;

    let quality_summary = summarize_step53_quality(&service_response.segments);
    Ok(BuildStep53TranslationPolishCommandResponse {
        task_id: request.task_id,
        media_path: request.media_path,
        source_lang: request.source_lang,
        target_lang: request.target_lang,
        schema_version: step5_schema_version_value(),
        pipeline_version: step5_pipeline_version_value(),
        artifact_meta: step5_artifact_meta(),
        quality_summary,
        batch_size: service_response.batch_size,
        batch_total: service_response.batch_total,
        segment_total: service_response.segment_total,
        theme_summary: request.theme_summary.trim().to_string(),
        terminology_entries,
        segments: step5_final_segments_to_command(service_response.segments),
    })
}
