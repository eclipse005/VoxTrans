use std::path::PathBuf;

use super::{
    DEFAULT_ALIGN_MODEL, DEFAULT_ASR_MODEL, DEMUCS_MODEL_DOWNLOAD_FILES, ModelTarget,
    QWEN3_ASR_06B_MODEL, QWEN3_ASR_17B_MODEL, REQUIRED_QWEN_ALIGNER_MODEL_FILES,
    REQUIRED_QWEN3_ASR_06B_MODEL_FILES, REQUIRED_QWEN3_ASR_17B_MODEL_FILES, resolve_model_dir,
};

#[derive(Debug, Clone)]
pub(crate) struct ModelDownloadFile {
    pub(crate) file_name: String,
    pub(crate) url: String,
    pub(crate) expected_size: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct ModelDefinition {
    pub(crate) target: ModelTarget,
    pub(crate) model: String,
    pub(crate) model_dir: PathBuf,
    pub(crate) required_files: Vec<String>,
    pub(crate) download_files: Vec<ModelDownloadFile>,
}

pub(crate) fn model_definition(
    target: ModelTarget,
    model: Option<&str>,
) -> Result<ModelDefinition, String> {
    let model = normalize_model_name(target, model);
    match target {
        ModelTarget::Asr => {
            let files = qwen3_asr_download_files(&model)?;
            Ok(ModelDefinition {
                target,
                model: model.clone(),
                model_dir: resolve_model_dir(target, &model),
                required_files: files.iter().map(|(name, _)| (*name).to_string()).collect(),
                download_files: files
                    .iter()
                    .map(|(file_name, expected_size)| ModelDownloadFile {
                        file_name: (*file_name).to_string(),
                        url: modelscope_download_url(&model, file_name),
                        expected_size: *expected_size,
                    })
                    .collect(),
            })
        }
        ModelTarget::Align => Ok(ModelDefinition {
            target,
            model: model.clone(),
            model_dir: resolve_model_dir(target, &model),
            required_files: aligner_download_files()
                .iter()
                .map(|(name, _)| (*name).to_string())
                .collect(),
            download_files: aligner_download_files()
                .iter()
                .map(|(file_name, expected_size)| ModelDownloadFile {
                    file_name: (*file_name).to_string(),
                    url: modelscope_download_url(&model, file_name),
                    expected_size: *expected_size,
                })
                .collect(),
        }),
        ModelTarget::Demucs => {
            let weights_name = format!("{model}.safetensors");
            let Some((_, url, expected_size)) = DEMUCS_MODEL_DOWNLOAD_FILES
                .iter()
                .find(|(name, _, _)| *name == weights_name)
            else {
                return Err(format!("unknown demucs model: {model}"));
            };
            Ok(ModelDefinition {
                target,
                model: model.clone(),
                model_dir: resolve_model_dir(target, &model),
                required_files: vec![weights_name.clone()],
                download_files: vec![ModelDownloadFile {
                    file_name: weights_name,
                    url: (*url).to_string(),
                    expected_size: *expected_size,
                }],
            })
        }
    }
}

pub(crate) fn normalize_model_name(target: ModelTarget, model: Option<&str>) -> String {
    let fallback = match target {
        ModelTarget::Asr => DEFAULT_ASR_MODEL,
        ModelTarget::Align => DEFAULT_ALIGN_MODEL,
        ModelTarget::Demucs => "htdemucs_ft",
    };
    let value = model.unwrap_or(fallback).trim();
    if value.is_empty() {
        fallback.to_string()
    } else {
        value.to_string()
    }
}

fn qwen3_asr_download_files(model: &str) -> Result<Vec<(&'static str, u64)>, String> {
    match model {
        QWEN3_ASR_06B_MODEL => Ok(REQUIRED_QWEN3_ASR_06B_MODEL_FILES
            .iter()
            .map(|file| (*file, qwen3_asr_file_size(model, file)))
            .collect()),
        QWEN3_ASR_17B_MODEL => Ok(REQUIRED_QWEN3_ASR_17B_MODEL_FILES
            .iter()
            .map(|file| (*file, qwen3_asr_file_size(model, file)))
            .collect()),
        _ => Err(format!("unknown asr model: {model}")),
    }
}

fn qwen3_asr_file_size(model: &str, file: &str) -> u64 {
    match (model, file) {
        (QWEN3_ASR_06B_MODEL, "config.json") => 6_193,
        (QWEN3_ASR_06B_MODEL, "model.safetensors") => 1_876_091_704,
        (QWEN3_ASR_06B_MODEL, "tokenizer.json") => 4_760_186,
        (QWEN3_ASR_17B_MODEL, "config.json") => 6_194,
        (QWEN3_ASR_17B_MODEL, "model-00001-of-00002.safetensors") => 4_220_320_824,
        (QWEN3_ASR_17B_MODEL, "model-00002-of-00002.safetensors") => 478_200_688,
        (QWEN3_ASR_17B_MODEL, "model.safetensors.index.json") => 64_821,
        (QWEN3_ASR_17B_MODEL, "tokenizer.json") => 4_760_186,
        _ => 1,
    }
}

fn aligner_download_files() -> Vec<(&'static str, u64)> {
    REQUIRED_QWEN_ALIGNER_MODEL_FILES
        .iter()
        .map(|file| {
            let expected_size = match *file {
                "config.json" => 5_982,
                "merges.txt" => 1_671_853,
                "model.safetensors" => 1_835_544_544,
                "tokenizer_config.json" => 12_666,
                "vocab.json" => 2_776_833,
                _ => 1,
            };
            (*file, expected_size)
        })
        .collect()
}

fn modelscope_download_url(model: &str, file_name: &str) -> String {
    format!("https://modelscope.cn/models/eclipse005/{model}/resolve/master/{file_name}")
}
