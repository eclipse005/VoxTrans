use super::transcription::{
    BuildSourceSentencesCommandRequest, WordTokenCommandDto, build_source_sentences,
    default_llm_concurrency, default_subtitle_max_words_per_segment,
};

#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct AsrArtifactForSentenceCli {
    task_id: String,
    media_path: String,
    source_lang: String,
    words: Vec<WordTokenCommandDto>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(untagged)]
enum AsrArtifactForSentenceCliInput {
    Flat(AsrArtifactForSentenceCli),
    Segment(AsrSegmentArtifactForSentenceCli),
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct AsrSegmentArtifactForSentenceCli {
    #[serde(default)]
    task_id: String,
    #[serde(default)]
    media_path: String,
    #[serde(default)]
    source_lang: String,
    #[serde(default)]
    language: String,
    #[serde(default)]
    words: Vec<WordTokenCommandDto>,
    segments: Vec<AsrSegmentWithWordsForSentenceCli>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct AsrSegmentWithWordsForSentenceCli {
    #[serde(default)]
    words: Vec<WordTokenCommandDto>,
}

pub fn maybe_run_build_source_sentences_mode_from_args() -> bool {
    const RUN_ARG: &str = "--voxtrans-build-source-sentences";
    let args = std::env::args().collect::<Vec<_>>();
    if args.len() < 2 || args[1] != RUN_ARG {
        return false;
    }

    let code = match run_build_source_sentences_mode_from_args(&args[2..]) {
        Ok(()) => 0,
        Err(err) => {
            eprintln!("{err}");
            1
        }
    };
    std::process::exit(code);
}

fn run_build_source_sentences_mode_from_args(args: &[String]) -> Result<(), String> {
    let mut asr_path = String::new();
    let mut output_path = String::new();
    let mut translate_api_key = String::new();
    let mut translate_base_url = String::new();
    let mut translate_model = String::new();
    let mut llm_concurrency = default_llm_concurrency();

    let mut idx = 0usize;
    while idx < args.len() {
        match args[idx].as_str() {
            "--asr-path" => {
                idx += 1;
                asr_path = required_cli_value(args, idx, "--asr-path")?;
            }
            "--output-path" => {
                idx += 1;
                output_path = required_cli_value(args, idx, "--output-path")?;
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
            other => return Err(format!("unknown source-sentences arg: {other}")),
        }
        idx += 1;
    }

    if asr_path.trim().is_empty() {
        return Err("--asr-path is required".to_string());
    }
    if translate_api_key.trim().is_empty()
        || translate_base_url.trim().is_empty()
        || translate_model.trim().is_empty()
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
        if llm_concurrency == default_llm_concurrency() {
            llm_concurrency = settings.llm_concurrency;
        }
    }

    let raw = std::fs::read_to_string(&asr_path).map_err(|err| err.to_string())?;
    let asr = parse_asr_artifact_for_sentence_cli(&raw, &asr_path)?;
    let response = tauri::async_runtime::block_on(build_source_sentences(
        BuildSourceSentencesCommandRequest {
            task_id: asr.task_id.clone(),
            audio_path: asr.media_path.clone(),
            source_lang: asr.source_lang.clone(),
            words: asr.words,
            subtitle_max_words_per_segment: default_subtitle_max_words_per_segment(),
            translate_api_key,
            translate_base_url,
            translate_model,
            llm_concurrency,
        },
    ))?;
    let output_path = if output_path.trim().is_empty() {
        std::path::PathBuf::from(&asr_path)
            .parent()
            .ok_or_else(|| "asr path has no parent directory".to_string())?
            .join("step_02_segments.json")
    } else {
        std::path::PathBuf::from(output_path)
    };
    let segments_payload =
        serde_json::to_string_pretty(&response.segments).map_err(|err| err.to_string())?;
    std::fs::write(&output_path, segments_payload.as_bytes()).map_err(|err| err.to_string())?;

    println!("{}", output_path.display());
    Ok(())
}

fn required_cli_value(args: &[String], idx: usize, flag: &str) -> Result<String, String> {
    args.get(idx)
        .cloned()
        .ok_or_else(|| format!("{flag} requires value"))
}

fn parse_asr_artifact_for_sentence_cli(
    raw: &str,
    asr_path: &str,
) -> Result<AsrArtifactForSentenceCli, String> {
    let parsed: AsrArtifactForSentenceCliInput =
        serde_json::from_str(raw).map_err(|err| format!("failed to parse asr json: {err}"))?;
    match parsed {
        AsrArtifactForSentenceCliInput::Flat(flat) => Ok(flat),
        AsrArtifactForSentenceCliInput::Segment(segment) => {
            let words = if !segment.words.is_empty() {
                segment.words
            } else {
                segment
                    .segments
                    .into_iter()
                    .flat_map(|entry| entry.words.into_iter())
                    .collect::<Vec<_>>()
            };
            if words.is_empty() {
                return Err("failed to parse asr json: no words found".to_string());
            }

            let task_id = if segment.task_id.trim().is_empty() {
                std::path::Path::new(asr_path)
                    .file_stem()
                    .and_then(|name| name.to_str())
                    .filter(|name| !name.trim().is_empty())
                    .unwrap_or("task")
                    .to_string()
            } else {
                segment.task_id
            };
            let media_path = if segment.media_path.trim().is_empty() {
                asr_path.to_string()
            } else {
                segment.media_path
            };
            let source_lang = if !segment.source_lang.trim().is_empty() {
                segment.source_lang
            } else if !segment.language.trim().is_empty() {
                segment.language
            } else {
                "auto".to_string()
            };

            Ok(AsrArtifactForSentenceCli {
                task_id,
                media_path,
                source_lang,
                words,
            })
        }
    }
}
