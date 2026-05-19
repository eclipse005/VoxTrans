use crate::commands::translate::build_terminology_layer;
use crate::commands::translate_artifacts::{
    Step3TerminologyArtifactForCli, artifact_dir_from_file_path, default_task_id_from_path,
    parse_step2_segments_artifact_for_input,
};
use crate::commands::translate_types::{BuildTerminologyLayerCommandRequest, default_llm_concurrency};

use super::{maybe_run_cli_mode, required_cli_value};

pub fn maybe_run_build_terminology_mode_from_args() -> bool {
    maybe_run_cli_mode(
        "--voxtrans-build-terminology",
        run_build_terminology_mode_from_args,
    )
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
