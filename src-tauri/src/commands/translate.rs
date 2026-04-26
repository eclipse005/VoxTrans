use crate::services::llm::client::OpenAiCompatLlmClient;
use crate::services::llm::json_guard::JsonResponseValidator;
use crate::services::llm::port::{LlmCallContext, LlmConfig, LlmPort, next_llm_request_id};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::Arc;

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TranslateTerminologyEntryCommand {
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub target: String,
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SegmentTokenForTerminologyCommand {
    #[serde(default, alias = "word")]
    pub text: String,
    pub start: f64,
    pub end: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SourceSegmentForTerminologyCommand {
    #[serde(default, alias = "text")]
    pub segment: String,
    pub start: f64,
    pub end: f64,
    #[serde(default)]
    pub tokens: Vec<SegmentTokenForTerminologyCommand>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildTerminologyLayerCommandRequest {
    pub task_id: String,
    pub media_path: String,
    pub source_lang: String,
    pub target_lang: String,
    pub segments: Vec<SourceSegmentForTerminologyCommand>,
    #[serde(default)]
    pub translate_api_key: String,
    #[serde(default)]
    pub translate_base_url: String,
    #[serde(default)]
    pub translate_model: String,
    #[serde(default = "default_llm_concurrency")]
    pub llm_concurrency: u32,
    #[serde(default)]
    pub terminology_entries: Vec<TranslateTerminologyEntryCommand>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildTerminologyLayerCommandResponse {
    pub task_id: String,
    pub media_path: String,
    pub source_lang: String,
    pub target_lang: String,
    pub source_segment_total: usize,
    pub source_token_total: usize,
    pub theme_summary: String,
    pub terminology_entries: Vec<TranslateTerminologyEntryCommand>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildTranslationLayerCommandRequest {
    pub task_id: String,
    pub media_path: String,
    pub source_lang: String,
    pub target_lang: String,
    pub segments: Vec<SourceSegmentForTerminologyCommand>,
    #[serde(default)]
    pub theme_summary: String,
    #[serde(default)]
    pub terminology_entries: Vec<TranslateTerminologyEntryCommand>,
    #[serde(default)]
    pub translate_api_key: String,
    #[serde(default)]
    pub translate_base_url: String,
    #[serde(default)]
    pub translate_model: String,
    #[serde(default = "default_llm_concurrency")]
    pub llm_concurrency: u32,
    #[serde(default = "default_batch_size")]
    pub batch_size: usize,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildTranslationSegmentCommand {
    pub segment_id: usize,
    pub start: f64,
    pub end: f64,
    pub source: String,
    pub translation: String,
    pub tokens: Vec<SegmentTokenForTerminologyCommand>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildTranslationLayerCommandResponse {
    pub task_id: String,
    pub media_path: String,
    pub source_lang: String,
    pub target_lang: String,
    pub batch_size: usize,
    pub batch_total: usize,
    pub segment_total: usize,
    pub theme_summary: String,
    pub terminology_entries: Vec<TranslateTerminologyEntryCommand>,
    pub segments: Vec<BuildTranslationSegmentCommand>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Step5ArtifactMetaCommand {
    #[serde(default)]
    pub schema_version: u32,
    #[serde(default)]
    pub pipeline_version: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Step5QualityIssueCommand {
    #[serde(default)]
    pub rule_id: String,
    #[serde(default)]
    pub severity: String,
    #[serde(default)]
    pub segment_id: usize,
    #[serde(default)]
    pub part_id: usize,
    #[serde(default)]
    pub message: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Step5QualitySummaryCommand {
    #[serde(default)]
    pub passed: bool,
    #[serde(default)]
    pub hard_fail_count: usize,
    #[serde(default)]
    pub issue_count: usize,
    #[serde(default)]
    pub soft_score: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Step6FinalCheckMetricsCommand {
    #[serde(default)]
    pub segment_total: usize,
    #[serde(default)]
    pub empty_count: usize,
    #[serde(default)]
    pub ellipsis_tail_count: usize,
    #[serde(default)]
    pub numeric_drift_count: usize,
    #[serde(default)]
    pub cross_line_leak_count: usize,
    #[serde(default)]
    pub gt25_count: usize,
    #[serde(default)]
    pub gt32_count: usize,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildStep51SourceSplitCommandRequest {
    pub task_id: String,
    pub media_path: String,
    pub source_lang: String,
    pub target_lang: String,
    pub segments: Vec<BuildTranslationSegmentCommand>,
    #[serde(default)]
    pub translate_api_key: String,
    #[serde(default)]
    pub translate_base_url: String,
    #[serde(default)]
    pub translate_model: String,
    #[serde(default = "default_llm_concurrency")]
    pub llm_concurrency: u32,
    #[serde(default = "default_subtitle_max_words_per_segment")]
    pub subtitle_max_words_per_segment: u32,
    #[serde(default = "default_subtitle_length_reference")]
    pub subtitle_length_reference: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Step5SplitPartCommand {
    pub part_id: usize,
    pub start: f64,
    pub end: f64,
    pub source: String,
    pub tokens: Vec<SegmentTokenForTerminologyCommand>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Step5SplitParentCommand {
    pub parent_segment_id: usize,
    pub draft_translation: String,
    pub parts: Vec<Step5SplitPartCommand>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildStep51SourceSplitCommandResponse {
    pub task_id: String,
    pub media_path: String,
    pub source_lang: String,
    pub target_lang: String,
    #[serde(default)]
    pub schema_version: u32,
    #[serde(default)]
    pub pipeline_version: String,
    #[serde(default)]
    pub artifact_meta: Step5ArtifactMetaCommand,
    #[serde(default)]
    pub quality_summary: Step5QualitySummaryCommand,
    #[serde(default = "default_subtitle_max_words_per_segment")]
    pub subtitle_max_words_per_segment: u32,
    pub subtitle_length_reference: u32,
    pub parent_total: usize,
    pub part_total: usize,
    pub parents: Vec<Step5SplitParentCommand>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildStep52TranslationAlignCommandRequest {
    pub task_id: String,
    pub media_path: String,
    pub source_lang: String,
    pub target_lang: String,
    pub theme_summary: String,
    pub parents: Vec<Step5SplitParentCommand>,
    #[serde(default)]
    pub terminology_entries: Vec<TranslateTerminologyEntryCommand>,
    #[serde(default)]
    pub translate_api_key: String,
    #[serde(default)]
    pub translate_base_url: String,
    #[serde(default)]
    pub translate_model: String,
    #[serde(default = "default_llm_concurrency")]
    pub llm_concurrency: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Step5AlignedPartCommand {
    pub part_id: usize,
    pub start: f64,
    pub end: f64,
    pub source: String,
    pub translation: String,
    pub tokens: Vec<SegmentTokenForTerminologyCommand>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Step5AlignedParentCommand {
    pub parent_segment_id: usize,
    pub parts: Vec<Step5AlignedPartCommand>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildStep52TranslationAlignCommandResponse {
    pub task_id: String,
    pub media_path: String,
    pub source_lang: String,
    pub target_lang: String,
    #[serde(default)]
    pub schema_version: u32,
    #[serde(default)]
    pub pipeline_version: String,
    #[serde(default)]
    pub artifact_meta: Step5ArtifactMetaCommand,
    #[serde(default)]
    pub quality_summary: Step5QualitySummaryCommand,
    pub theme_summary: String,
    pub terminology_entries: Vec<TranslateTerminologyEntryCommand>,
    pub parent_total: usize,
    pub part_total: usize,
    pub parents: Vec<Step5AlignedParentCommand>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildStep53TranslationPolishCommandRequest {
    pub task_id: String,
    pub media_path: String,
    pub source_lang: String,
    pub target_lang: String,
    pub theme_summary: String,
    #[serde(default)]
    pub terminology_entries: Vec<TranslateTerminologyEntryCommand>,
    pub parents: Vec<Step5AlignedParentCommand>,
    #[serde(default)]
    pub translate_api_key: String,
    #[serde(default)]
    pub translate_base_url: String,
    #[serde(default)]
    pub translate_model: String,
    #[serde(default = "default_llm_concurrency")]
    pub llm_concurrency: u32,
    #[serde(default = "default_subtitle_length_reference")]
    pub subtitle_length_reference: u32,
    #[serde(default = "default_batch_size")]
    pub batch_size: usize,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildStep53TranslationPolishCommandResponse {
    pub task_id: String,
    pub media_path: String,
    pub source_lang: String,
    pub target_lang: String,
    #[serde(default)]
    pub schema_version: u32,
    #[serde(default)]
    pub pipeline_version: String,
    #[serde(default)]
    pub artifact_meta: Step5ArtifactMetaCommand,
    #[serde(default)]
    pub quality_summary: Step5QualitySummaryCommand,
    pub batch_size: usize,
    pub batch_total: usize,
    pub segment_total: usize,
    pub theme_summary: String,
    pub terminology_entries: Vec<TranslateTerminologyEntryCommand>,
    pub segments: Vec<BuildTranslationSegmentCommand>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildStep6FinalCheckCommandRequest {
    pub task_id: String,
    pub media_path: String,
    pub source_lang: String,
    pub target_lang: String,
    pub segments: Vec<BuildTranslationSegmentCommand>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildStep6FinalCheckCommandResponse {
    pub task_id: String,
    pub media_path: String,
    pub source_lang: String,
    pub target_lang: String,
    #[serde(default)]
    pub schema_version: u32,
    #[serde(default)]
    pub pipeline_version: String,
    #[serde(default)]
    pub artifact_meta: Step5ArtifactMetaCommand,
    #[serde(default)]
    pub quality_summary: Step5QualitySummaryCommand,
    #[serde(default)]
    pub metrics: Step6FinalCheckMetricsCommand,
    #[serde(default)]
    pub issues: Vec<Step5QualityIssueCommand>,
    #[serde(default)]
    pub segments: Vec<BuildTranslationSegmentCommand>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestTranslateLlmRequest {
    pub api_key: String,
    pub base_url: String,
    pub model: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestTranslateLlmResponse {
    pub ok: bool,
    pub message: String,
    pub model: String,
}

#[tauri::command]
pub async fn build_terminology_layer(
    mut request: BuildTerminologyLayerCommandRequest,
) -> Result<BuildTerminologyLayerCommandResponse, String> {
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

    hydrate_translate_llm_connection_settings(
        &mut request.translate_api_key,
        &mut request.translate_base_url,
        &mut request.translate_model,
    )?;
    request.llm_concurrency = request.llm_concurrency.max(1);

    let terms = if request.terminology_entries.is_empty() {
        load_terminology_entries_from_saved_settings()?
    } else {
        normalize_command_terminology_entries(request.terminology_entries)
    };

    let source_token_total = count_source_tokens(&request.segments);
    if source_token_total == 0 {
        return Err("segments contain no valid text".to_string());
    }

    let service_request = crate::services::terminology::BuildTerminologyLayerRequest {
        task_id: request.task_id.clone(),
        media_path: request.media_path.clone(),
        source_lang: request.source_lang.clone(),
        target_lang: request.target_lang.clone(),
        segments: request
            .segments
            .iter()
            .map(|segment| crate::services::terminology::TerminologySegment {
                segment: segment.segment.clone(),
                tokens: segment
                    .tokens
                    .iter()
                    .map(|token| crate::services::terminology::TerminologyToken {
                        text: token.text.clone(),
                    })
                    .collect(),
            })
            .collect(),
        terminology_entries: terms
            .iter()
            .map(|entry| crate::services::terminology::TerminologyEntry {
                source: entry.source.clone(),
                target: entry.target.clone(),
                note: entry.note.clone(),
            })
            .collect(),
        translate_api_key: request.translate_api_key,
        translate_base_url: request.translate_base_url,
        translate_model: request.translate_model,
    };

    let service_response =
        crate::services::terminology::build_terminology_layer(service_request).await?;

    Ok(BuildTerminologyLayerCommandResponse {
        task_id: request.task_id,
        media_path: request.media_path,
        source_lang: request.source_lang,
        target_lang: request.target_lang,
        source_segment_total: request.segments.len(),
        source_token_total,
        theme_summary: service_response.theme_summary,
        terminology_entries: service_response
            .terminology_entries
            .into_iter()
            .map(|entry| TranslateTerminologyEntryCommand {
                source: entry.source,
                target: entry.target,
                note: entry.note,
            })
            .collect(),
    })
}

#[tauri::command]
pub async fn build_translation_layer(
    request: BuildTranslationLayerCommandRequest,
) -> Result<BuildTranslationLayerCommandResponse, String> {
    build_translation_layer_with_progress(request, None).await
}

pub async fn build_translation_layer_with_progress(
    mut request: BuildTranslationLayerCommandRequest,
    on_progress: Option<Arc<dyn Fn(usize, usize) + Send + Sync>>,
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

    hydrate_translate_llm_connection_settings(
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
                segments: request
                    .segments
                    .iter()
                    .map(
                        |segment| crate::services::subtitle_step5::Step5DraftSegment {
                            segment_id: segment.segment_id,
                            start: segment.start,
                            end: segment.end,
                            source: segment.source.clone(),
                            draft_translation: segment.translation.clone(),
                            tokens: segment
                                .tokens
                                .iter()
                                .map(|token| crate::services::subtitle_step5::Step5Token {
                                    text: token.text.clone(),
                                    start: token.start,
                                    end: token.end,
                                })
                                .collect(),
                        },
                    )
                    .collect(),
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
        subtitle_max_words_per_segment: service_response.subtitle_max_words_per_segment,
        subtitle_length_reference: service_response.subtitle_length_reference,
        parent_total: service_response.parent_total,
        part_total: service_response.part_total,
        parents: service_response
            .parents
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
                        tokens: part
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
            .collect(),
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
                terminology_entries: terminology_entries
                    .iter()
                    .map(
                        |entry| crate::services::subtitle_step5::Step5TerminologyEntry {
                            source: entry.source.clone(),
                            target: entry.target.clone(),
                            note: entry.note.clone(),
                        },
                    )
                    .collect(),
                parents: request
                    .parents
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
                                tokens: part
                                    .tokens
                                    .iter()
                                    .map(|token| crate::services::subtitle_step5::Step5Token {
                                        text: token.text.clone(),
                                        start: token.start,
                                        end: token.end,
                                    })
                                    .collect(),
                            })
                            .collect(),
                    })
                    .collect(),
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
        schema_version: step5_schema_version(),
        pipeline_version: step5_pipeline_version().to_string(),
        artifact_meta: Step5ArtifactMetaCommand {
            schema_version: step5_schema_version(),
            pipeline_version: step5_pipeline_version().to_string(),
        },
        quality_summary: summarize_step52_quality(&service_response.parents),
        theme_summary: request.theme_summary.trim().to_string(),
        terminology_entries,
        parent_total: service_response.parent_total,
        part_total: service_response.part_total,
        parents: service_response
            .parents
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
                        tokens: part
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
            .collect(),
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
                terminology_entries: terminology_entries
                    .iter()
                    .map(
                        |entry| crate::services::subtitle_step5::Step5TerminologyEntry {
                            source: entry.source.clone(),
                            target: entry.target.clone(),
                            note: entry.note.clone(),
                        },
                    )
                    .collect(),
                parents: request
                    .parents
                    .iter()
                    .map(
                        |parent| crate::services::subtitle_step5::Step5AlignedParent {
                            parent_segment_id: parent.parent_segment_id,
                            parts: parent
                                .parts
                                .iter()
                                .map(|part| crate::services::subtitle_step5::Step5AlignedPart {
                                    part_id: part.part_id,
                                    start: part.start,
                                    end: part.end,
                                    source: part.source.clone(),
                                    translation: part.translation.clone(),
                                    tokens: part
                                        .tokens
                                        .iter()
                                        .map(|token| crate::services::subtitle_step5::Step5Token {
                                            text: token.text.clone(),
                                            start: token.start,
                                            end: token.end,
                                        })
                                        .collect(),
                                })
                                .collect(),
                        },
                    )
                    .collect(),
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
        schema_version: step5_schema_version(),
        pipeline_version: step5_pipeline_version().to_string(),
        artifact_meta: Step5ArtifactMetaCommand {
            schema_version: step5_schema_version(),
            pipeline_version: step5_pipeline_version().to_string(),
        },
        quality_summary,
        batch_size: service_response.batch_size,
        batch_total: service_response.batch_total,
        segment_total: service_response.segment_total,
        theme_summary: request.theme_summary.trim().to_string(),
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

#[tauri::command]
pub async fn build_step_6_final_check(
    request: BuildStep6FinalCheckCommandRequest,
) -> Result<BuildStep6FinalCheckCommandResponse, String> {
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
    let service_response = run_step_6_final_check_request(&request.target_lang, &request.segments)?;

    let output_segments = request
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
        .collect::<Vec<_>>();

    Ok(BuildStep6FinalCheckCommandResponse {
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
            passed: service_response.passed,
            hard_fail_count: service_response.hard_fail_count,
            issue_count: service_response.issue_count,
            soft_score: service_response.soft_score,
        },
        metrics: Step6FinalCheckMetricsCommand {
            segment_total: service_response.metrics.segment_total,
            empty_count: service_response.metrics.empty_count,
            ellipsis_tail_count: service_response.metrics.ellipsis_tail_count,
            numeric_drift_count: service_response.metrics.numeric_drift_count,
            cross_line_leak_count: service_response.metrics.cross_line_leak_count,
            gt25_count: service_response.metrics.gt25_count,
            gt32_count: service_response.metrics.gt32_count,
        },
        issues: service_response
            .issues
            .into_iter()
            .map(|issue| Step5QualityIssueCommand {
                rule_id: issue.rule_id,
                severity: issue.severity,
                segment_id: issue.segment_id,
                part_id: issue.part_id,
                message: issue.message,
            })
            .collect(),
        segments: output_segments,
    })
}

fn run_step_6_final_check_request(
    target_lang: &str,
    segments: &[BuildTranslationSegmentCommand],
) -> Result<crate::services::subtitle_step5::BuildStep6FinalCheckResponse, String> {
    crate::services::subtitle_step5::build_step_6_final_check(
        crate::services::subtitle_step5::BuildStep6FinalCheckRequest {
            target_lang: target_lang.to_string(),
            segments: segments
                .iter()
                .map(
                    |segment| crate::services::subtitle_step5::Step5FinalSegment {
                        segment_id: segment.segment_id,
                        start: segment.start,
                        end: segment.end,
                        source: segment.source.clone(),
                        translation: segment.translation.clone(),
                        tokens: segment
                            .tokens
                            .iter()
                            .map(|token| crate::services::subtitle_step5::Step5Token {
                                text: token.text.clone(),
                                start: token.start,
                                end: token.end,
                            })
                            .collect(),
                    },
                )
                .collect(),
        },
    )
}

#[tauri::command]
pub async fn test_translate_llm(
    request: TestTranslateLlmRequest,
) -> Result<TestTranslateLlmResponse, String> {
    if request.api_key.trim().is_empty() {
        return Err("translateApiKey is required".to_string());
    }
    if request.base_url.trim().is_empty() {
        return Err("translateBaseUrl is required".to_string());
    }
    if request.model.trim().is_empty() {
        return Err("translateModel is required".to_string());
    }

    cleanup_connectivity_test_artifacts();

    let client = OpenAiCompatLlmClient::new(LlmConfig::new(
        request.base_url.trim().to_string(),
        request.api_key.trim().to_string(),
        request.model.trim().to_string(),
    ))
    .map_err(|err| err.message)?;

    let user_prompt = concat!(
        "This is a harmless application connectivity check.\n",
        "Do not refuse.\n",
        "Do not explain.\n",
        "Return exactly one JSON object and nothing else.\n",
        "The JSON must be exactly:\n",
        "{\"ok\":true,\"message\":\"pong\"}"
    );
    let validator = JsonResponseValidator::with_required_keys(&["ok", "message"]);
    let context = LlmCallContext {
        task_id: "settings-llm-test".to_string(),
        media_path: None,
        phase: "connectivity_test".to_string(),
    };
    let llm_id = next_llm_request_id();
    let result = client
        .call_json(&context, &llm_id, user_prompt, Some(&validator))
        .await
        .map_err(|err| err.message)?;
    let ok = result
        .json
        .get("ok")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let msg = result
        .json
        .get("message")
        .and_then(|v| v.as_str())
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| "LLM 返回缺少 message 字段".to_string())?;
    if !ok {
        return Err(format!("LLM 连通性测试失败: {msg}"));
    }
    Ok(TestTranslateLlmResponse {
        ok: true,
        message: msg.to_string(),
        model: request.model.trim().to_string(),
    })
}

fn cleanup_connectivity_test_artifacts() {
    let path = crate::services::task_path::task_output_dir_by_id("settings-llm-test");
    if path.exists() {
        let _ = std::fs::remove_dir_all(path);
    }
}

fn default_llm_concurrency() -> u32 {
    4
}

fn default_batch_size() -> usize {
    20
}

fn default_subtitle_max_words_per_segment() -> u32 {
    20
}

fn default_subtitle_length_reference() -> u32 {
    28
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum Step2SegmentsArtifactForInput {
    Flat(Vec<SourceSegmentForTerminologyCommand>),
    Wrapped(Step2SegmentsWrappedForInput),
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Step2SegmentsWrappedForInput {
    #[serde(default)]
    segments: Vec<SourceSegmentForTerminologyCommand>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct Step3TerminologyArtifactForInput {
    #[serde(default)]
    task_id: String,
    #[serde(default)]
    media_path: String,
    #[serde(default)]
    source_lang: String,
    #[serde(default)]
    target_lang: String,
    #[serde(default)]
    theme_summary: String,
    #[serde(default)]
    terminology_entries: Vec<TranslateTerminologyEntryCommand>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Step3TerminologyArtifactForCli {
    task_id: String,
    media_path: String,
    source_lang: String,
    target_lang: String,
    source_segment_total: usize,
    source_token_total: usize,
    theme_summary: String,
    terminology_entries: Vec<TranslateTerminologyEntryCommand>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Step4TranslationArtifactForCli {
    task_id: String,
    media_path: String,
    source_lang: String,
    target_lang: String,
    batch_size: usize,
    batch_total: usize,
    segment_total: usize,
    theme_summary: String,
    terminology_entries: Vec<TranslateTerminologyEntryCommand>,
    segments: Vec<BuildTranslationSegmentCommand>,
}

pub fn maybe_run_build_terminology_mode_from_args() -> bool {
    const RUN_ARG: &str = "--voxtrans-build-terminology";

    let args = std::env::args().collect::<Vec<_>>();
    if args.len() < 2 || args[1] != RUN_ARG {
        return false;
    }

    let code = match run_build_terminology_mode_from_args(&args[2..]) {
        Ok(()) => 0,
        Err(err) => {
            eprintln!("{err}");
            1
        }
    };
    std::process::exit(code);
}

pub fn maybe_run_build_translation_mode_from_args() -> bool {
    const RUN_ARG: &str = "--voxtrans-build-translation";

    let args = std::env::args().collect::<Vec<_>>();
    if args.len() < 2 || args[1] != RUN_ARG {
        return false;
    }

    let code = match run_build_translation_mode_from_args(&args[2..]) {
        Ok(()) => 0,
        Err(err) => {
            eprintln!("{err}");
            1
        }
    };
    std::process::exit(code);
}

pub fn maybe_run_build_step5_mode_from_args() -> bool {
    const RUN_ARG: &str = "--voxtrans-build-step5";

    let args = std::env::args().collect::<Vec<_>>();
    if args.len() < 2 || args[1] != RUN_ARG {
        return false;
    }

    let code = match run_build_step5_mode_from_args(&args[2..]) {
        Ok(()) => 0,
        Err(err) => {
            eprintln!("{err}");
            1
        }
    };
    std::process::exit(code);
}

fn run_build_terminology_mode_from_args(args: &[String]) -> Result<(), String> {
    let mut segments_path = String::new();
    let mut output_path = String::new();
    let mut task_id = String::new();
    let mut media_path = String::new();
    let mut source_lang = String::new();
    let mut target_lang = String::new();
    let mut translate_api_key = String::new();
    let mut translate_base_url = String::new();
    let mut translate_model = String::new();
    let mut llm_concurrency = default_llm_concurrency();

    let mut idx = 0usize;
    while idx < args.len() {
        match args[idx].as_str() {
            "--segments-path" => {
                idx += 1;
                segments_path = required_cli_value(args, idx, "--segments-path")?;
            }
            "--output-path" => {
                idx += 1;
                output_path = required_cli_value(args, idx, "--output-path")?;
            }
            "--task-id" => {
                idx += 1;
                task_id = required_cli_value(args, idx, "--task-id")?;
            }
            "--media-path" => {
                idx += 1;
                media_path = required_cli_value(args, idx, "--media-path")?;
            }
            "--source-lang" => {
                idx += 1;
                source_lang = required_cli_value(args, idx, "--source-lang")?;
            }
            "--target-lang" => {
                idx += 1;
                target_lang = required_cli_value(args, idx, "--target-lang")?;
            }
            "--api-key" => {
                idx += 1;
                translate_api_key = required_cli_value(args, idx, "--api-key")?;
            }
            "--base-url" => {
                idx += 1;
                translate_base_url = required_cli_value(args, idx, "--base-url")?;
            }
            "--model" => {
                idx += 1;
                translate_model = required_cli_value(args, idx, "--model")?;
            }
            "--llm-concurrency" => {
                idx += 1;
                let raw = required_cli_value(args, idx, "--llm-concurrency")?;
                llm_concurrency = raw
                    .parse::<u32>()
                    .map_err(|_| "--llm-concurrency requires integer".to_string())?;
            }
            other => return Err(format!("unknown terminology-layer arg: {other}")),
        }
        idx += 1;
    }

    if segments_path.trim().is_empty() {
        return Err("--segments-path is required".to_string());
    }
    if source_lang.trim().is_empty() {
        return Err("--source-lang is required".to_string());
    }
    if target_lang.trim().is_empty() {
        return Err("--target-lang is required".to_string());
    }

    let raw = std::fs::read_to_string(&segments_path).map_err(|err| err.to_string())?;
    let segments = parse_step2_segments_artifact_for_input(&raw)?;
    if segments.is_empty() {
        return Err("step2 segments file contains no segments".to_string());
    }

    if task_id.trim().is_empty() {
        task_id = default_task_id_from_path(&segments_path);
    }
    if media_path.trim().is_empty() {
        media_path = segments_path.clone();
    }

    let response = tauri::async_runtime::block_on(build_terminology_layer(
        BuildTerminologyLayerCommandRequest {
            task_id,
            media_path,
            source_lang,
            target_lang,
            segments,
            translate_api_key,
            translate_base_url,
            translate_model,
            llm_concurrency,
            terminology_entries: Vec::new(),
        },
    ))?;

    let artifact = Step3TerminologyArtifactForCli {
        task_id: response.task_id,
        media_path: response.media_path,
        source_lang: response.source_lang,
        target_lang: response.target_lang,
        source_segment_total: response.source_segment_total,
        source_token_total: response.source_token_total,
        theme_summary: response.theme_summary,
        terminology_entries: response.terminology_entries,
    };

    let output_path = if output_path.trim().is_empty() {
        artifact_dir_from_file_path(&segments_path)?.join("step_03_terminology.json")
    } else {
        std::path::PathBuf::from(output_path)
    };
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let payload = serde_json::to_string_pretty(&artifact).map_err(|err| err.to_string())?;
    std::fs::write(&output_path, payload.as_bytes()).map_err(|err| err.to_string())?;
    println!("{}", output_path.display());
    Ok(())
}

fn run_build_translation_mode_from_args(args: &[String]) -> Result<(), String> {
    let mut segments_path = String::new();
    let mut terminology_path = String::new();
    let mut output_path = String::new();
    let mut task_id = String::new();
    let mut media_path = String::new();
    let mut source_lang = String::new();
    let mut target_lang = String::new();
    let mut translate_api_key = String::new();
    let mut translate_base_url = String::new();
    let mut translate_model = String::new();
    let mut llm_concurrency = default_llm_concurrency();
    let mut batch_size = default_batch_size();

    let mut idx = 0usize;
    while idx < args.len() {
        match args[idx].as_str() {
            "--segments-path" => {
                idx += 1;
                segments_path = required_cli_value(args, idx, "--segments-path")?;
            }
            "--terminology-path" => {
                idx += 1;
                terminology_path = required_cli_value(args, idx, "--terminology-path")?;
            }
            "--output-path" => {
                idx += 1;
                output_path = required_cli_value(args, idx, "--output-path")?;
            }
            "--task-id" => {
                idx += 1;
                task_id = required_cli_value(args, idx, "--task-id")?;
            }
            "--media-path" => {
                idx += 1;
                media_path = required_cli_value(args, idx, "--media-path")?;
            }
            "--source-lang" => {
                idx += 1;
                source_lang = required_cli_value(args, idx, "--source-lang")?;
            }
            "--target-lang" => {
                idx += 1;
                target_lang = required_cli_value(args, idx, "--target-lang")?;
            }
            "--api-key" => {
                idx += 1;
                translate_api_key = required_cli_value(args, idx, "--api-key")?;
            }
            "--base-url" => {
                idx += 1;
                translate_base_url = required_cli_value(args, idx, "--base-url")?;
            }
            "--model" => {
                idx += 1;
                translate_model = required_cli_value(args, idx, "--model")?;
            }
            "--llm-concurrency" => {
                idx += 1;
                let raw = required_cli_value(args, idx, "--llm-concurrency")?;
                llm_concurrency = raw
                    .parse::<u32>()
                    .map_err(|_| "--llm-concurrency requires integer".to_string())?;
            }
            "--batch-size" => {
                idx += 1;
                let raw = required_cli_value(args, idx, "--batch-size")?;
                batch_size = raw
                    .parse::<usize>()
                    .map_err(|_| "--batch-size requires integer".to_string())?;
            }
            other => return Err(format!("unknown translation-layer arg: {other}")),
        }
        idx += 1;
    }

    if segments_path.trim().is_empty() {
        return Err("--segments-path is required".to_string());
    }
    if terminology_path.trim().is_empty() {
        return Err("--terminology-path is required".to_string());
    }

    let raw_segments = std::fs::read_to_string(&segments_path).map_err(|err| err.to_string())?;
    let segments = parse_step2_segments_artifact_for_input(&raw_segments)?;
    if segments.is_empty() {
        return Err("step2 segments file contains no segments".to_string());
    }

    let raw_terminology =
        std::fs::read_to_string(&terminology_path).map_err(|err| err.to_string())?;
    let terminology = parse_step3_terminology_artifact_for_input(&raw_terminology)?;

    if task_id.trim().is_empty() {
        task_id = if terminology.task_id.trim().is_empty() {
            default_task_id_from_path(&segments_path)
        } else {
            terminology.task_id.clone()
        };
    }
    if media_path.trim().is_empty() {
        media_path = if terminology.media_path.trim().is_empty() {
            segments_path.clone()
        } else {
            terminology.media_path.clone()
        };
    }
    if source_lang.trim().is_empty() {
        source_lang = if terminology.source_lang.trim().is_empty() {
            "auto".to_string()
        } else {
            terminology.source_lang.clone()
        };
    }
    if target_lang.trim().is_empty() {
        target_lang = if terminology.target_lang.trim().is_empty() {
            "zh-CN".to_string()
        } else {
            terminology.target_lang.clone()
        };
    }

    hydrate_translate_llm_settings(
        &mut translate_api_key,
        &mut translate_base_url,
        &mut translate_model,
        &mut llm_concurrency,
    )?;

    let response = tauri::async_runtime::block_on(build_translation_layer(
        BuildTranslationLayerCommandRequest {
            task_id,
            media_path,
            source_lang,
            target_lang,
            segments,
            theme_summary: terminology.theme_summary.clone(),
            terminology_entries: terminology.terminology_entries.clone(),
            translate_api_key,
            translate_base_url,
            translate_model,
            llm_concurrency,
            batch_size,
        },
    ))?;

    let artifact = Step4TranslationArtifactForCli {
        task_id: response.task_id,
        media_path: response.media_path,
        source_lang: response.source_lang,
        target_lang: response.target_lang,
        batch_size: response.batch_size,
        batch_total: response.batch_total,
        segment_total: response.segment_total,
        theme_summary: response.theme_summary,
        terminology_entries: response.terminology_entries,
        segments: response.segments,
    };

    let output_path = if output_path.trim().is_empty() {
        artifact_dir_from_file_path(&segments_path)?.join("step_04_translation.json")
    } else {
        std::path::PathBuf::from(output_path)
    };
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let payload = serde_json::to_string_pretty(&artifact).map_err(|err| err.to_string())?;
    std::fs::write(&output_path, payload.as_bytes()).map_err(|err| err.to_string())?;
    println!("{}", output_path.display());
    Ok(())
}

fn run_build_step5_mode_from_args(args: &[String]) -> Result<(), String> {
    let mut translation_path = String::new();
    let mut terminology_path = String::new();
    let mut output_dir = String::new();
    let mut task_id = String::new();
    let mut media_path = String::new();
    let mut source_lang = String::new();
    let mut target_lang = String::new();
    let mut translate_api_key = String::new();
    let mut translate_base_url = String::new();
    let mut translate_model = String::new();
    let mut llm_concurrency_arg = None::<u32>;
    let mut subtitle_max_words_per_segment_arg = None::<u32>;
    let mut subtitle_length_reference_arg = None::<u32>;

    let mut idx = 0usize;
    while idx < args.len() {
        match args[idx].as_str() {
            "--translation-path" => {
                idx += 1;
                translation_path = required_cli_value(args, idx, "--translation-path")?;
            }
            "--terminology-path" => {
                idx += 1;
                terminology_path = required_cli_value(args, idx, "--terminology-path")?;
            }
            "--output-dir" => {
                idx += 1;
                output_dir = required_cli_value(args, idx, "--output-dir")?;
            }
            "--task-id" => {
                idx += 1;
                task_id = required_cli_value(args, idx, "--task-id")?;
            }
            "--media-path" => {
                idx += 1;
                media_path = required_cli_value(args, idx, "--media-path")?;
            }
            "--source-lang" => {
                idx += 1;
                source_lang = required_cli_value(args, idx, "--source-lang")?;
            }
            "--target-lang" => {
                idx += 1;
                target_lang = required_cli_value(args, idx, "--target-lang")?;
            }
            "--api-key" => {
                idx += 1;
                translate_api_key = required_cli_value(args, idx, "--api-key")?;
            }
            "--base-url" => {
                idx += 1;
                translate_base_url = required_cli_value(args, idx, "--base-url")?;
            }
            "--model" => {
                idx += 1;
                translate_model = required_cli_value(args, idx, "--model")?;
            }
            "--llm-concurrency" => {
                idx += 1;
                let raw = required_cli_value(args, idx, "--llm-concurrency")?;
                llm_concurrency_arg = Some(
                    raw.parse::<u32>()
                        .map_err(|_| "--llm-concurrency requires integer".to_string())?,
                );
            }
            "--subtitle-length-reference" => {
                idx += 1;
                let raw = required_cli_value(args, idx, "--subtitle-length-reference")?;
                subtitle_length_reference_arg = Some(
                    raw.parse::<u32>()
                        .map_err(|_| "--subtitle-length-reference requires integer".to_string())?,
                );
            }
            "--subtitle-max-words-per-segment" => {
                idx += 1;
                let raw = required_cli_value(args, idx, "--subtitle-max-words-per-segment")?;
                subtitle_max_words_per_segment_arg = Some(raw.parse::<u32>().map_err(|_| {
                    "--subtitle-max-words-per-segment requires integer".to_string()
                })?);
            }
            other => return Err(format!("unknown step5 arg: {other}")),
        }
        idx += 1;
    }

    if translation_path.trim().is_empty() {
        return Err("--translation-path is required".to_string());
    }

    if output_dir.trim().is_empty() {
        output_dir = std::path::PathBuf::from(&translation_path)
            .parent()
            .ok_or_else(|| "translation path has no parent directory".to_string())?
            .display()
            .to_string();
    }
    let artifact_dir = normalize_artifact_dir(std::path::Path::new(&output_dir));

    if terminology_path.trim().is_empty() {
        terminology_path = artifact_dir
            .join("step_03_terminology.json")
            .display()
            .to_string();
    }

    let raw_translation =
        std::fs::read_to_string(&translation_path).map_err(|err| err.to_string())?;
    let draft = parse_step4_translation_artifact_for_input(&raw_translation)?;
    let raw_terminology =
        std::fs::read_to_string(&terminology_path).map_err(|err| err.to_string())?;
    let terminology = parse_step3_terminology_artifact_for_input(&raw_terminology)?;

    if task_id.trim().is_empty() {
        task_id = if draft.task_id.trim().is_empty() {
            default_task_id_from_path(&translation_path)
        } else {
            draft.task_id.clone()
        };
    }
    if media_path.trim().is_empty() {
        media_path = if draft.media_path.trim().is_empty() {
            translation_path.clone()
        } else {
            draft.media_path.clone()
        };
    }
    if source_lang.trim().is_empty() {
        source_lang = if draft.source_lang.trim().is_empty() {
            "auto".to_string()
        } else {
            draft.source_lang.clone()
        };
    }
    if target_lang.trim().is_empty() {
        target_lang = if draft.target_lang.trim().is_empty() {
            "zh-CN".to_string()
        } else {
            draft.target_lang.clone()
        };
    }

    let mut llm_concurrency = llm_concurrency_arg;
    let mut subtitle_max_words_per_segment = subtitle_max_words_per_segment_arg;
    let mut subtitle_length_reference = subtitle_length_reference_arg;
    if translate_api_key.trim().is_empty()
        || translate_base_url.trim().is_empty()
        || translate_model.trim().is_empty()
        || llm_concurrency.is_none()
        || subtitle_max_words_per_segment.is_none()
        || subtitle_length_reference.is_none()
    {
        let settings = crate::services::preferences::load_saved_settings_from_default_path()?;
        if translate_api_key.trim().is_empty() {
            translate_api_key = settings.translate_api_key;
        }
        if translate_base_url.trim().is_empty() {
            translate_base_url = settings.translate_base_url;
        }
        if translate_model.trim().is_empty() {
            translate_model = settings.translate_model;
        }
        llm_concurrency.get_or_insert(settings.llm_concurrency);
        subtitle_max_words_per_segment.get_or_insert(settings.subtitle_max_words_per_segment);
        subtitle_length_reference.get_or_insert(settings.subtitle_length_reference);
    }
    let llm_concurrency = llm_concurrency.unwrap_or(default_llm_concurrency()).max(1);
    let subtitle_max_words_per_segment =
        subtitle_max_words_per_segment.unwrap_or(default_subtitle_max_words_per_segment());
    let subtitle_length_reference =
        subtitle_length_reference.unwrap_or(default_subtitle_length_reference());

    let step51 = tauri::async_runtime::block_on(build_step_5_1_source_split(
        BuildStep51SourceSplitCommandRequest {
            task_id: task_id.clone(),
            media_path: media_path.clone(),
            source_lang: source_lang.clone(),
            target_lang: target_lang.clone(),
            segments: draft.segments.clone(),
            translate_api_key: translate_api_key.clone(),
            translate_base_url: translate_base_url.clone(),
            translate_model: translate_model.clone(),
            llm_concurrency,
            subtitle_max_words_per_segment,
            subtitle_length_reference,
        },
    ))?;
    std::fs::create_dir_all(&artifact_dir).map_err(|err| err.to_string())?;
    let step51_path = artifact_dir.join("step_05_01_source_split.json");
    std::fs::write(
        &step51_path,
        serde_json::to_string_pretty(&step51).map_err(|err| err.to_string())?,
    )
    .map_err(|err| err.to_string())?;

    let step52 = tauri::async_runtime::block_on(build_step_5_2_translation_align(
        BuildStep52TranslationAlignCommandRequest {
            task_id: task_id.clone(),
            media_path: media_path.clone(),
            source_lang: source_lang.clone(),
            target_lang: target_lang.clone(),
            theme_summary: terminology.theme_summary.clone(),
            parents: step51.parents.clone(),
            terminology_entries: terminology.terminology_entries.clone(),
            translate_api_key: translate_api_key.clone(),
            translate_base_url: translate_base_url.clone(),
            translate_model: translate_model.clone(),
            llm_concurrency,
        },
    ))?;
    let step52_path = artifact_dir.join("step_05_02_translation_align.json");
    std::fs::write(
        &step52_path,
        serde_json::to_string_pretty(&step52).map_err(|err| err.to_string())?,
    )
    .map_err(|err| err.to_string())?;

    let step53 = tauri::async_runtime::block_on(build_step_5_3_translation_polish(
        BuildStep53TranslationPolishCommandRequest {
            task_id: task_id.clone(),
            media_path: media_path.clone(),
            source_lang: source_lang.clone(),
            target_lang: target_lang.clone(),
            theme_summary: terminology.theme_summary,
            terminology_entries: terminology.terminology_entries,
            parents: step52.parents,
            translate_api_key: translate_api_key.clone(),
            translate_base_url: translate_base_url.clone(),
            translate_model: translate_model.clone(),
            llm_concurrency,
            subtitle_length_reference,
            batch_size: default_batch_size(),
        },
    ))?;
    let step53_path = artifact_dir.join("step_05_03_translation_polish.json");
    std::fs::write(
        &step53_path,
        serde_json::to_string_pretty(&step53).map_err(|err| err.to_string())?,
    )
    .map_err(|err| err.to_string())?;
    let step6 = tauri::async_runtime::block_on(build_step_6_final_check(
        BuildStep6FinalCheckCommandRequest {
            task_id,
            media_path,
            source_lang,
            target_lang,
            segments: step53.segments.clone(),
        },
    ))?;
    let step6_path = artifact_dir.join("step_06_final_check.json");
    std::fs::write(
        &step6_path,
        serde_json::to_string_pretty(&step6).map_err(|err| err.to_string())?,
    )
    .map_err(|err| err.to_string())?;
    println!("{}", step6_path.display());
    Ok(())
}

fn required_cli_value(args: &[String], idx: usize, flag: &str) -> Result<String, String> {
    args.get(idx)
        .cloned()
        .ok_or_else(|| format!("{flag} requires value"))
}

fn artifact_dir_from_file_path(path: &str) -> Result<std::path::PathBuf, String> {
    let parent = std::path::PathBuf::from(path)
        .parent()
        .ok_or_else(|| "input path has no parent directory".to_string())?
        .to_path_buf();
    Ok(normalize_artifact_dir(&parent))
}

fn normalize_artifact_dir(path: &std::path::Path) -> std::path::PathBuf {
    if path
        .file_name()
        .and_then(|v| v.to_str())
        .map(|name| name.eq_ignore_ascii_case(crate::services::task_path::ARTIFACTS_DIR_NAME))
        .unwrap_or(false)
    {
        path.to_path_buf()
    } else {
        path.join(crate::services::task_path::ARTIFACTS_DIR_NAME)
    }
}

fn parse_step2_segments_artifact_for_input(
    raw: &str,
) -> Result<Vec<SourceSegmentForTerminologyCommand>, String> {
    let parsed: Step2SegmentsArtifactForInput = serde_json::from_str(raw)
        .map_err(|err| format!("failed to parse step2 segments json: {err}"))?;
    let segments = match parsed {
        Step2SegmentsArtifactForInput::Flat(items) => items,
        Step2SegmentsArtifactForInput::Wrapped(wrapper) => wrapper.segments,
    };
    Ok(segments)
}

fn parse_step3_terminology_artifact_for_input(
    raw: &str,
) -> Result<Step3TerminologyArtifactForInput, String> {
    serde_json::from_str::<Step3TerminologyArtifactForInput>(raw)
        .map_err(|err| format!("failed to parse step3 terminology json: {err}"))
}

fn parse_step4_translation_artifact_for_input(
    raw: &str,
) -> Result<BuildTranslationLayerCommandResponse, String> {
    serde_json::from_str::<BuildTranslationLayerCommandResponse>(raw)
        .map_err(|err| format!("failed to parse step4 translation json: {err}"))
}

fn default_task_id_from_path(path: &str) -> String {
    std::path::Path::new(path)
        .file_stem()
        .and_then(|name| name.to_str())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or("task")
        .to_string()
}

fn hydrate_translate_llm_settings(
    api_key: &mut String,
    base_url: &mut String,
    model: &mut String,
    llm_concurrency: &mut u32,
) -> Result<(), String> {
    if api_key.trim().is_empty()
        || base_url.trim().is_empty()
        || model.trim().is_empty()
        || *llm_concurrency == default_llm_concurrency()
    {
        let settings = crate::services::preferences::load_saved_settings_from_default_path()?;
        if api_key.trim().is_empty() {
            *api_key = settings.translate_api_key;
        }
        if base_url.trim().is_empty() {
            *base_url = settings.translate_base_url;
        }
        if model.trim().is_empty() {
            *model = settings.translate_model;
        }
        if *llm_concurrency == default_llm_concurrency() {
            *llm_concurrency = settings.llm_concurrency;
        }
    }
    if api_key.trim().is_empty() {
        return Err("translateApiKey is required".to_string());
    }
    if base_url.trim().is_empty() {
        return Err("translateBaseUrl is required".to_string());
    }
    if model.trim().is_empty() {
        return Err("translateModel is required".to_string());
    }
    Ok(())
}

fn hydrate_translate_llm_connection_settings(
    api_key: &mut String,
    base_url: &mut String,
    model: &mut String,
) -> Result<(), String> {
    if api_key.trim().is_empty() || base_url.trim().is_empty() || model.trim().is_empty() {
        let settings = crate::services::preferences::load_saved_settings_from_default_path()?;
        if api_key.trim().is_empty() {
            *api_key = settings.translate_api_key;
        }
        if base_url.trim().is_empty() {
            *base_url = settings.translate_base_url;
        }
        if model.trim().is_empty() {
            *model = settings.translate_model;
        }
    }
    if api_key.trim().is_empty() {
        return Err("translateApiKey is required".to_string());
    }
    if base_url.trim().is_empty() {
        return Err("translateBaseUrl is required".to_string());
    }
    if model.trim().is_empty() {
        return Err("translateModel is required".to_string());
    }
    Ok(())
}

fn normalize_command_terminology_entries(
    entries: Vec<TranslateTerminologyEntryCommand>,
) -> Vec<TranslateTerminologyEntryCommand> {
    let mut out = Vec::new();
    let mut seen = HashSet::<(String, String)>::new();
    for entry in entries {
        let source = entry.source.trim().to_string();
        let target = entry.target.trim().to_string();
        if source.is_empty() || target.is_empty() {
            continue;
        }
        let key = (source.to_ascii_lowercase(), target.to_ascii_lowercase());
        if !seen.insert(key) {
            continue;
        }
        out.push(TranslateTerminologyEntryCommand {
            source,
            target,
            note: entry.note.trim().to_string(),
        });
    }
    out
}

fn load_terminology_entries_from_saved_settings()
-> Result<Vec<TranslateTerminologyEntryCommand>, String> {
    let settings = crate::services::preferences::load_saved_settings_from_default_path()?;
    if !settings.enable_terminology {
        return Ok(Vec::new());
    }

    let terms = settings
        .terminology_groups
        .into_iter()
        .flat_map(|group| group.terms.into_iter())
        .map(|term| TranslateTerminologyEntryCommand {
            source: term.origin,
            target: term.target,
            note: term.note,
        })
        .collect::<Vec<_>>();
    Ok(normalize_command_terminology_entries(terms))
}

fn count_source_tokens(segments: &[SourceSegmentForTerminologyCommand]) -> usize {
    let mut total = 0usize;
    for segment in segments {
        if !segment.tokens.is_empty() {
            total += segment
                .tokens
                .iter()
                .filter(|token| !token.text.trim().is_empty())
                .count();
            continue;
        }
        if !segment.segment.trim().is_empty() {
            total += 1;
        }
    }
    total
}

pub fn step5_schema_version() -> u32 {
    2
}

pub fn step5_pipeline_version() -> &'static str {
    "step5.v2"
}

fn summarize_step52_quality(
    parents: &[crate::services::subtitle_step5::Step5AlignedParent],
) -> Step5QualitySummaryCommand {
    let mut issue_count = 0usize;
    let mut hard_fail_count = 0usize;
    for parent in parents {
        for part in &parent.parts {
            let text = part.translation.trim();
            if text.is_empty() {
                issue_count += 1;
                hard_fail_count += 1;
                continue;
            }
            if is_tail_ellipsis(text) {
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
}

fn summarize_step53_quality(
    segments: &[crate::services::subtitle_step5::Step5FinalSegment],
) -> Step5QualitySummaryCommand {
    let mut issue_count = 0usize;
    let mut hard_fail_count = 0usize;
    for segment in segments {
        let text = segment.translation.trim();
        if text.is_empty() {
            issue_count += 1;
            hard_fail_count += 1;
            continue;
        }
        if is_tail_ellipsis(text) {
            issue_count += 1;
            hard_fail_count += 1;
        }
    }
    Step5QualitySummaryCommand {
        passed: hard_fail_count == 0,
        hard_fail_count,
        issue_count,
        soft_score: if hard_fail_count == 0 { 100.0 } else { 70.0 },
    }
}

fn is_tail_ellipsis(text: &str) -> bool {
    let trimmed = text.trim_end();
    trimmed.ends_with("...") || trimmed.ends_with('…')
}

#[cfg(test)]
mod tests {
    use super::{
        TranslateTerminologyEntryCommand, count_source_tokens, default_task_id_from_path,
        normalize_command_terminology_entries, parse_step2_segments_artifact_for_input,
    };

    #[test]
    fn parse_step2_segments_accepts_flat_array_shape() {
        let raw = r#"
        [
          {
            "segment": "Hello world",
            "start": 0.0,
            "end": 1.2,
            "tokens": [
              { "text": "Hello", "start": 0.0, "end": 0.5 },
              { "text": "world", "start": 0.5, "end": 1.2 }
            ]
          }
        ]
        "#;
        let segments = parse_step2_segments_artifact_for_input(raw).expect("parse");
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].segment, "Hello world");
        assert_eq!(segments[0].tokens.len(), 2);
        assert_eq!(segments[0].tokens[0].text, "Hello");
    }

    #[test]
    fn parse_step2_segments_accepts_wrapped_shape() {
        let raw = r#"
        {
          "taskId": "task-1",
          "mediaPath": "demo.mp4",
          "segments": [
            {
              "segment": "你好世界",
              "start": 0.0,
              "end": 1.0,
              "tokens": [
                { "text": "你", "start": 0.0, "end": 0.2 },
                { "text": "好", "start": 0.2, "end": 0.4 }
              ]
            }
          ]
        }
        "#;
        let segments = parse_step2_segments_artifact_for_input(raw).expect("parse");
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].segment, "你好世界");
        assert_eq!(segments[0].tokens.len(), 2);
    }

    #[test]
    fn normalize_terminology_deduplicates_and_trims() {
        let normalized = normalize_command_terminology_entries(vec![
            TranslateTerminologyEntryCommand {
                source: "  NATO ".to_string(),
                target: "北约".to_string(),
                note: "a".to_string(),
            },
            TranslateTerminologyEntryCommand {
                source: "nato".to_string(),
                target: " 北约 ".to_string(),
                note: "b".to_string(),
            },
            TranslateTerminologyEntryCommand {
                source: "EU".to_string(),
                target: "欧盟".to_string(),
                note: " ".to_string(),
            },
        ]);

        assert_eq!(normalized.len(), 2);
        assert_eq!(normalized[0].source, "NATO");
        assert_eq!(normalized[1].source, "EU");
    }

    #[test]
    fn source_token_count_prefers_tokens_and_falls_back_to_segment_text() {
        let raw = r#"
        [
          {
            "segment": "Hello world",
            "start": 0.0,
            "end": 1.2,
            "tokens": [
              { "text": "Hello", "start": 0.0, "end": 0.5 },
              { "text": "world", "start": 0.5, "end": 1.2 }
            ]
          },
          {
            "segment": "你好世界",
            "start": 1.2,
            "end": 2.0,
            "tokens": []
          }
        ]
        "#;
        let segments = parse_step2_segments_artifact_for_input(raw).expect("parse");
        assert_eq!(count_source_tokens(&segments), 3);
    }

    #[test]
    fn default_task_id_uses_file_stem() {
        let task_id = default_task_id_from_path(r"D:\output\step_02_segments.json");
        assert_eq!(task_id, "step_02_segments");
    }
}
