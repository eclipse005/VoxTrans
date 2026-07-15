mod catalog;
mod download_http;
mod download_progress;
mod downloader;
mod path;
mod status;

use crate::app_state::{AppState, ModelDownloadRuntime};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

pub(crate) use catalog::{ModelDefinition, model_definition};
pub use downloader::{cancel_model_download, start_model_download};
pub use path::{
    open_model_dir, resolve_aligner_model_dir, resolve_asr_model_dir, resolve_engine_model_dir,
    resolve_model_dir, resolve_models_root, set_models_root_override,
};
pub use status::{ModelStatusResponse, get_model_status};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelTarget {
    Asr,
    Align,
    Demucs,
}

impl ModelTarget {
    pub fn dir_name(self) -> &'static str {
        match self {
            Self::Asr => DEFAULT_ASR_MODEL,
            Self::Align => DEFAULT_ALIGN_MODEL,
            Self::Demucs => "demucs",
        }
    }
}

pub(crate) const DEFAULT_ASR_MODEL: &str = QWEN3_ASR_06B_MODEL;
pub(crate) const QWEN3_ASR_06B_MODEL: &str = "Qwen3-ASR-0.6B";
pub(crate) const QWEN3_ASR_17B_MODEL: &str = "Qwen3-ASR-1.7B";
pub(crate) const COHERE_ASR_MODEL: &str = "cohere-transcribe-03-2026";
pub(crate) const MOSS_ASR_MODEL: &str = "moss-transcribe-diarize";
pub(crate) const DEFAULT_ALIGN_MODEL: &str = "mms-300m-1130-forced-aligner";
pub(crate) const QWEN_ALIGN_MODEL: &str = "Qwen3-ForcedAligner-0.6B";
pub(crate) const MMS_CTC_ALIGN_MODEL: &str = "mms-300m-1130-forced-aligner";

pub(crate) const REQUIRED_QWEN3_ASR_06B_MODEL_FILES: [&str; 3] =
    ["config.json", "model.safetensors", "tokenizer.json"];

pub(crate) const REQUIRED_QWEN3_ASR_17B_MODEL_FILES: [&str; 5] = [
    "config.json",
    "model-00001-of-00002.safetensors",
    "model-00002-of-00002.safetensors",
    "model.safetensors.index.json",
    "tokenizer.json",
];

// Cohere ASR model files. Unlike Qwen (which uses a HuggingFace
// `tokenizer.json`), Cohere uses a SentencePiece tokenizer, so the required
// files are `tokenizer.model` + `vocab.json` + `tokenizer_config.json`.
// `preprocessor_config.json` is required by Cohere's
// `FeatureConfig::from_model_dir` (mel/window frontend).
pub(crate) const REQUIRED_COHERE_ASR_MODEL_FILES: [&str; 6] = [
    "config.json",
    "model.safetensors",
    "preprocessor_config.json",
    "tokenizer.model",
    "tokenizer_config.json",
    "vocab.json",
];

// MOSS-Transcribe-Diarize: sharded safetensors + HF tokenizer.
// Enough for moss-transcribe-diarize-rs `AsrInference::load_with`.
pub(crate) const REQUIRED_MOSS_ASR_MODEL_FILES: [&str; 4] = [
    "config.json",
    "model.safetensors.index.json",
    "model-00000-of-00001.safetensors",
    "tokenizer.json",
];

pub(crate) const REQUIRED_QWEN_ALIGNER_MODEL_FILES: [&str; 5] = [
    "config.json",
    "merges.txt",
    "model.safetensors",
    "tokenizer_config.json",
    "vocab.json",
];

// MMS CTC forced aligner — only files read by ctc-forced-aligner-rs.
pub(crate) const REQUIRED_MMS_CTC_ALIGNER_MODEL_FILES: [&str; 3] = [
    "config.json",
    "model.safetensors",
    "vocab.json",
];

pub(crate) const DEMUCS_MODEL_DOWNLOAD_FILES: [(&str, &str, u64); 1] = [(
    "htdemucs_ft.safetensors",
    "https://modelscope.cn/models/eclipse005/htdemucs/resolve/master/htdemucs_ft.safetensors",
    349_312_000,
)];

pub(crate) fn runtime_for_target(
    state: &AppState,
    target: ModelTarget,
) -> Arc<Mutex<ModelDownloadRuntime>> {
    match target {
        ModelTarget::Asr => state.asr_model_download.clone(),
        ModelTarget::Align => state.align_model_download.clone(),
        ModelTarget::Demucs => state.demucs_model_download.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_keeps_asr_and_aligner_as_independent_models() {
        let asr =
            model_definition(ModelTarget::Asr, Some(QWEN3_ASR_06B_MODEL)).expect("asr definition");
        let align = model_definition(ModelTarget::Align, Some("Qwen3-ForcedAligner-0.6B"))
            .expect("align definition");

        assert_eq!(asr.model, QWEN3_ASR_06B_MODEL);
        assert_eq!(align.model, "Qwen3-ForcedAligner-0.6B");
        assert!(
            asr.required_files
                .iter()
                .all(|file| !file.contains("ForcedAligner"))
        );
        assert!(
            align
                .required_files
                .iter()
                .all(|file| !file.contains("ASR"))
        );
        assert!(!asr.download_files.is_empty());
        assert!(!align.download_files.is_empty());
    }
}
