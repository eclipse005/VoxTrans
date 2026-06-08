use std::sync::Arc;

use super::translate_llm_settings::{
    hydrate_translate_llm_connection_settings, hydrate_translate_llm_settings,
};
use super::translate_terms::normalize_command_terminology_entries;
use super::translate_types::{
    BuildStep51SourceSplitCommandRequest, BuildStep51SourceSplitCommandResponse,
    BuildStep52TranslationAlignCommandRequest, BuildStep52TranslationAlignCommandResponse,
    BuildTranslationSegmentCommand, SegmentTokenForTerminologyCommand,
    Step5AlignedParentCommand, Step5AlignedPartCommand, Step5ArtifactMetaCommand,
    Step5QualitySummaryCommand, Step5SplitParentCommand, Step5SplitPartCommand,
    TranslateTerminologyEntryCommand, step5_pipeline_version, step5_schema_version,
};
use crate::db::store::TaskStore;
use tauri::{AppHandle, Manager};

#[tauri::command]
pub async fn build_step_5_1_source_split(
    app: AppHandle,
    request: BuildStep51SourceSplitCommandRequest,
) -> Result<BuildStep51SourceSplitCommandResponse, String> {
    build_step_5_1_source_split_with_progress(app, request, None).await
}

pub async fn build_step_5_1_source_split_with_progress(
    app: AppHandle,
    request: BuildStep51SourceSplitCommandRequest,
    on_progress: Option<Arc<dyn Fn(usize, usize) + Send + Sync>>,
) -> Result<BuildStep51SourceSplitCommandResponse, String> {
    build_step_5_1_source_split_with_progress_and_unit_store(app, request, on_progress, None).await
}

pub async fn build_step_5_1_source_split_with_progress_and_unit_store(
    app: AppHandle,
    mut request: BuildStep51SourceSplitCommandRequest,
    on_progress: Option<Arc<dyn Fn(usize, usize) + Send + Sync>>,
    unit_store: Option<crate::services::pipeline::UnitStore>,
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

    let store = app.state::<TaskStore>().inner();
    hydrate_translate_llm_connection_settings(
        store,
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
                subtitle_length_preset: request.subtitle_length_preset.clone(),
                translate_api_key: request.translate_api_key.clone(),
                translate_base_url: request.translate_base_url.clone(),
                translate_model: request.translate_model.clone(),
                llm_concurrency: request.llm_concurrency,
                unit_store,
            },
            on_progress,
        )
        .await?;
    Ok(BuildStep51SourceSplitCommandResponse {
        task_id: request.task_id,
        media_path: request.media_path,
        source_lang: request.source_lang,
        target_lang: request.target_lang,
        schema_version: step5_schema_version(),
        pipeline_version: step5_pipeline_version().to_string(),
        artifact_meta: Step5ArtifactMetaCommand {
            schema_version: step5_schema_version(),
            pipeline_version: step5_pipeline_version().to_string(),
        },
        quality_summary: Step5QualitySummaryCommand {
            passed: service_response.parent_total > 0 && service_response.part_total > 0,
            hard_fail_count: 0,
            issue_count: 0,
            soft_score: 100.0,
        },
        subtitle_length_preset: service_response.subtitle_length_preset,
        parent_total: service_response.parent_total,
        part_total: service_response.part_total,
        parents: step5_split_parents_to_command(service_response.parents),
    })
}

#[tauri::command]
pub async fn build_step_5_2_translation_align(
    app: AppHandle,
    request: BuildStep52TranslationAlignCommandRequest,
) -> Result<BuildStep52TranslationAlignCommandResponse, String> {
    build_step_5_2_translation_align_with_progress(app, request, None).await
}

pub async fn build_step_5_2_translation_align_with_progress(
    app: AppHandle,
    request: BuildStep52TranslationAlignCommandRequest,
    on_progress: Option<Arc<dyn Fn(usize, usize) + Send + Sync>>,
) -> Result<BuildStep52TranslationAlignCommandResponse, String> {
    build_step_5_2_translation_align_with_progress_and_unit_store(app, request, on_progress, None).await
}

pub async fn build_step_5_2_translation_align_with_progress_and_unit_store(
    app: AppHandle,
    mut request: BuildStep52TranslationAlignCommandRequest,
    on_progress: Option<Arc<dyn Fn(usize, usize) + Send + Sync>>,
    unit_store: Option<crate::services::pipeline::UnitStore>,
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

    let store = app.state::<TaskStore>().inner();
    hydrate_translate_llm_settings(
        store,
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
                subtitle_length_preset: request.subtitle_length_preset,
                translate_api_key: request.translate_api_key,
                translate_base_url: request.translate_base_url.clone(),
                translate_model: request.translate_model.clone(),
                llm_concurrency: request.llm_concurrency,
                unit_store,
            },
            on_progress,
        )
        .await?;
    Ok(BuildStep52TranslationAlignCommandResponse {
        task_id: request.task_id,
        media_path: request.media_path,
        source_lang: request.source_lang,
        target_lang: request.target_lang,
        schema_version: step5_schema_version(),
        pipeline_version: step5_pipeline_version().to_string(),
        artifact_meta: Step5ArtifactMetaCommand {
            schema_version: step5_schema_version(),
            pipeline_version: step5_pipeline_version().to_string(),
        },
        quality_summary: {
            let mut issue_count = 0usize;
            let mut hard_fail_count = 0usize;
            for parent in &service_response.parents {
                for part in &parent.parts {
                    let text = part.translation.trim();
                    if text.is_empty() {
                        issue_count += 1;
                        hard_fail_count += 1;
                        continue;
                    }
                    if super::translate::is_tail_ellipsis(text) {
                        issue_count += 1;
                        hard_fail_count += 1;
                    }
                }
            }
            Step5QualitySummaryCommand {
                passed: hard_fail_count == 0,
                hard_fail_count,
                issue_count,
                soft_score: if hard_fail_count == 0 { 100.0 } else { 75.0 },
            }
        },
        theme_summary: request.theme_summary.trim().to_string(),
        terminology_entries,
        parent_total: service_response.parent_total,
        part_total: service_response.part_total,
        parents: step5_aligned_parents_to_command(service_response.parents),
    })
}

fn validate_step5_command_base(
    task_id: &str,
    media_path: &str,
    source_lang: &str,
    target_lang: &str,
) -> Result<(), String> {
    if task_id.trim().is_empty() {
        return Err("taskId is required".to_string());
    }
    if media_path.trim().is_empty() {
        return Err("mediaPath is required".to_string());
    }
    if source_lang.trim().is_empty() {
        return Err("sourceLang is required".to_string());
    }
    if target_lang.trim().is_empty() {
        return Err("targetLang is required".to_string());
    }
    Ok(())
}

fn command_tokens_to_step5(
    tokens: &[SegmentTokenForTerminologyCommand],
) -> Vec<crate::services::subtitle_step5::Step5Token> {
    tokens
        .iter()
        .map(|token| crate::services::subtitle_step5::Step5Token {
            text: token.text.clone(),
            start: token.start,
            end: token.end,
        })
        .collect()
}

fn step5_tokens_to_command(
    tokens: Vec<crate::services::subtitle_step5::Step5Token>,
) -> Vec<SegmentTokenForTerminologyCommand> {
    tokens
        .into_iter()
        .map(|token| SegmentTokenForTerminologyCommand {
            text: token.text,
            start: token.start,
            end: token.end,
        })
        .collect()
}

fn command_terminology_to_step5(
    entries: &[TranslateTerminologyEntryCommand],
) -> Vec<crate::services::subtitle_step5::Step5TerminologyEntry> {
    entries
        .iter()
        .map(
            |entry| crate::services::subtitle_step5::Step5TerminologyEntry {
                source: entry.source.clone(),
                target: entry.target.clone(),
                note: entry.note.clone(),
            },
        )
        .collect()
}

fn command_segments_to_step5_draft(
    segments: &[BuildTranslationSegmentCommand],
) -> Vec<crate::services::subtitle_step5::Step5DraftSegment> {
    segments
        .iter()
        .map(
            |segment| crate::services::subtitle_step5::Step5DraftSegment {
                segment_id: segment.segment_id,
                start: segment.start,
                end: segment.end,
                source: segment.source.clone(),
                draft_translation: segment.translation.clone(),
                tokens: command_tokens_to_step5(&segment.tokens),
            },
        )
        .collect()
}

fn step5_split_parents_to_command(
    parents: Vec<crate::services::subtitle_step5::Step5SplitParent>,
) -> Vec<Step5SplitParentCommand> {
    parents
        .into_iter()
        .map(|parent| Step5SplitParentCommand {
            parent_segment_id: parent.parent_segment_id,
            draft_translation: parent.draft_translation,
            parts: parent
                .parts
                .into_iter()
                .map(|part| Step5SplitPartCommand {
                    part_id: part.part_id,
                    start: part.start,
                    end: part.end,
                    source: part.source,
                    tokens: step5_tokens_to_command(part.tokens),
                })
                .collect(),
        })
        .collect()
}

fn command_split_parents_to_step5(
    parents: &[Step5SplitParentCommand],
) -> Vec<crate::services::subtitle_step5::Step5SplitParent> {
    parents
        .iter()
        .map(|parent| crate::services::subtitle_step5::Step5SplitParent {
            parent_segment_id: parent.parent_segment_id,
            draft_translation: parent.draft_translation.clone(),
            parts: parent
                .parts
                .iter()
                .map(|part| crate::services::subtitle_step5::Step5SplitPart {
                    part_id: part.part_id,
                    start: part.start,
                    end: part.end,
                    source: part.source.clone(),
                    tokens: command_tokens_to_step5(&part.tokens),
                })
                .collect(),
        })
        .collect()
}

fn step5_aligned_parents_to_command(
    parents: Vec<crate::services::subtitle_step5::Step5AlignedParent>,
) -> Vec<Step5AlignedParentCommand> {
    parents
        .into_iter()
        .map(|parent| Step5AlignedParentCommand {
            parent_segment_id: parent.parent_segment_id,
            parts: parent
                .parts
                .into_iter()
                .map(|part| Step5AlignedPartCommand {
                    part_id: part.part_id,
                    start: part.start,
                    end: part.end,
                    source: part.source,
                    translation: part.translation,
                    tokens: step5_tokens_to_command(part.tokens),
                })
                .collect(),
        })
        .collect()
}
