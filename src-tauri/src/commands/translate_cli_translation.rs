use super::translate::{BuildTranslationLayerCommandRequest, build_translation_layer};
use super::translate_artifacts::{
    Step4TranslationArtifactForCli, artifact_dir_from_file_path, default_task_id_from_path,
    parse_step2_segments_artifact_for_input, parse_step3_terminology_artifact_for_input,
};
use super::translate_cli_args::required_cli_value;
use super::translate_defaults::{default_batch_size, default_llm_concurrency};
use super::translate_llm_settings::hydrate_translate_llm_settings;
pub(super) fn run_build_translation_mode_from_args(args: &[String]) -> Result<(), String> {
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
