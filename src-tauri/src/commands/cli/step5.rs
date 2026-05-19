use crate::commands::translate::{
    build_step_5_1_source_split, build_step_5_2_translation_align,
};
use crate::commands::translate_artifacts::{
    default_task_id_from_path, normalize_artifact_dir,
    parse_step3_terminology_artifact_for_input, parse_step4_translation_artifact_for_input,
};
use crate::commands::translate_types::{
    BuildStep51SourceSplitCommandRequest, BuildStep52TranslationAlignCommandRequest,
    default_llm_concurrency,
};

use super::{maybe_run_cli_mode, required_cli_value};

pub fn maybe_run_build_step5_mode_from_args() -> bool {
    maybe_run_cli_mode("--voxtrans-build-step5", run_build_step5_mode_from_args)
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
    let mut subtitle_length_preset_arg = None::<String>;

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
            "--subtitle-length-preset" => {
                idx += 1;
                let raw = required_cli_value(args, idx, "--subtitle-length-preset")?;
                subtitle_length_preset_arg =
                    Some(crate::services::subtitle_length::normalize_subtitle_length_preset(&raw));
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
    let mut subtitle_length_preset = subtitle_length_preset_arg;
    if translate_api_key.trim().is_empty()
        || translate_base_url.trim().is_empty()
        || translate_model.trim().is_empty()
        || llm_concurrency.is_none()
        || subtitle_length_preset.is_none()
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
        subtitle_length_preset.get_or_insert(settings.subtitle_length_preset);
    }
    let llm_concurrency = llm_concurrency.unwrap_or(default_llm_concurrency()).max(1);
    let subtitle_length_preset = subtitle_length_preset.ok_or_else(|| {
        "--subtitle-length-preset is required when settings are unavailable".to_string()
    })?;

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
            subtitle_length_preset: subtitle_length_preset.clone(),
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
            subtitle_length_preset,
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

    println!("{}", step52_path.display());
    Ok(())
}
