use crate::services::llm::client::OpenAiCompatLlmClient;
use crate::services::llm::json_guard::JsonResponseValidator;
use crate::services::llm::port::{LlmCallContext, LlmConfig, LlmPort, next_llm_request_id};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

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

    hydrate_translate_llm_settings(
        &mut request.translate_api_key,
        &mut request.translate_base_url,
        &mut request.translate_model,
        &mut request.llm_concurrency,
    )?;

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
                start: segment.start,
                end: segment.end,
                tokens: segment
                    .tokens
                    .iter()
                    .map(|token| crate::services::terminology::TerminologyToken {
                        text: token.text.clone(),
                        start: token.start,
                        end: token.end,
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
    mut request: BuildTranslationLayerCommandRequest,
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

    hydrate_translate_llm_settings(
        &mut request.translate_api_key,
        &mut request.translate_base_url,
        &mut request.translate_model,
        &mut request.llm_concurrency,
    )?;

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

    let service_response =
        crate::services::translation::build_translation_layer(service_request).await?;

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

    let client = OpenAiCompatLlmClient::new(LlmConfig::new(
        request.base_url.trim().to_string(),
        request.api_key.trim().to_string(),
        request.model.trim().to_string(),
    ))
    .map_err(|err| err.message)?;

    let user_prompt = "返回 JSON：{\"ok\":true,\"message\":\"pong\"}";
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

fn default_llm_concurrency() -> u32 {
    4
}

fn default_batch_size() -> usize {
    20
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
        std::path::PathBuf::from(&segments_path)
            .parent()
            .ok_or_else(|| "segments path has no parent directory".to_string())?
            .join("step3_terminology.json")
    } else {
        std::path::PathBuf::from(output_path)
    };
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
        std::path::PathBuf::from(&segments_path)
            .parent()
            .ok_or_else(|| "segments path has no parent directory".to_string())?
            .join("step4_translation.json")
    } else {
        std::path::PathBuf::from(output_path)
    };
    let payload = serde_json::to_string_pretty(&artifact).map_err(|err| err.to_string())?;
    std::fs::write(&output_path, payload.as_bytes()).map_err(|err| err.to_string())?;
    println!("{}", output_path.display());
    Ok(())
}

fn required_cli_value(args: &[String], idx: usize, flag: &str) -> Result<String, String> {
    args.get(idx)
        .cloned()
        .ok_or_else(|| format!("{flag} requires value"))
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
        let task_id = default_task_id_from_path(r"D:\output\step2_segments.json");
        assert_eq!(task_id, "step2_segments");
    }
}
