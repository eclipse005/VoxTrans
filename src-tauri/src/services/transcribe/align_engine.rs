//! Multi-backend forced aligner (mirrors `AsrEngine`).
//!
//! - Qwen: language code from `LanguageRegistry`
//! - CTC (MMS): language fixed `eng`, romanize=true; `split_size` char for Zh/Yue/Ja else word

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::domain::language::LanguageTag;
use crate::domain::language_registry::LanguageRegistry;
use crate::domain::AlignedSpan;
use crate::services::preferences_types::AlignModel;

enum AlignBackend {
    Qwen(qwen_forced_aligner_rs::Qwen3ForcedAligner),
    Ctc(ctc_forced_aligner_rs::Aligner),
}

/// Loaded aligner selected by `AlignModel`.
pub(super) struct AlignEngine {
    backend: AlignBackend,
    model: AlignModel,
}

impl AlignEngine {
    /// Qwen forced-align emits word/char tokens **without** punctuation; a
    /// dedicated post-step re-attaches punct from the ASR transcript.
    /// CTC char/word split already carries punctuation from the same transcript
    /// — running re-attach would double it (`，，` / `。。`).
    pub(super) fn restores_punctuation_from_transcript(&self) -> bool {
        matches!(self.model, AlignModel::Qwen3ForcedAligner06B)
    }

    pub(super) fn load(
        model: AlignModel,
        model_dir: &Path,
        use_cuda: bool,
    ) -> Result<Self, String> {
        match model {
            AlignModel::Qwen3ForcedAligner06B => {
                let device = if use_cuda {
                    #[cfg(feature = "cuda")]
                    {
                        qwen_forced_aligner_rs::DeviceRequest::Cuda(0)
                    }
                    #[cfg(not(feature = "cuda"))]
                    {
                        return Err(
                            "CUDA provider requested but this build was not compiled with CUDA support"
                                .to_string(),
                        );
                    }
                } else {
                    qwen_forced_aligner_rs::DeviceRequest::Cpu
                };
                let aligner = qwen_forced_aligner_rs::load_model(
                    model_dir,
                    qwen_forced_aligner_rs::ModelOptions { device },
                )
                .map_err(|err| {
                    format!(
                        "failed to load Qwen aligner from {}: {err:#}",
                        model_dir.display()
                    )
                })?;
                Ok(Self {
                    backend: AlignBackend::Qwen(aligner),
                    model,
                })
            }
            AlignModel::MmsCtcForcedAligner300m => {
                let device = if use_cuda {
                    #[cfg(feature = "cuda")]
                    {
                        ctc_forced_aligner_rs::DeviceRequest::Cuda(0)
                    }
                    #[cfg(not(feature = "cuda"))]
                    {
                        return Err(
                            "CUDA provider requested but this build was not compiled with CUDA support"
                                .to_string(),
                        );
                    }
                } else {
                    ctc_forced_aligner_rs::DeviceRequest::Cpu
                };
                let aligner = ctc_forced_aligner_rs::load_model(
                    model_dir,
                    ctc_forced_aligner_rs::ModelOptions { device },
                )
                .map_err(|err| {
                    format!(
                        "failed to load CTC aligner from {}: {err:#}",
                        model_dir.display()
                    )
                })?;
                Ok(Self {
                    backend: AlignBackend::Ctc(aligner),
                    model,
                })
            }
        }
    }

    /// Align one segment: audio path + transcript text + source language tag.
    pub(super) fn align_segment(
        &self,
        audio_path: &Path,
        text: &str,
        source_lang: LanguageTag,
    ) -> Result<Vec<AlignedSpan>, String> {
        match &self.backend {
            AlignBackend::Qwen(aligner) => {
                let language = LanguageRegistry::align_code(self.model, source_lang)
                    .map_err(|e| e.to_string())?;
                let result = aligner
                    .align(qwen_forced_aligner_rs::AlignRequest::new(
                        qwen_forced_aligner_rs::AudioInput::Path(audio_path.to_path_buf()),
                        qwen_forced_aligner_rs::TextInput::Text(text.to_string()),
                        language.to_string(),
                    ))
                    .map_err(|err| format!("qwen alignment failed: {err:#}"))?;
                Ok(result
                    .items
                    .into_iter()
                    .map(|item| AlignedSpan::new(item.text, item.start_time, item.end_time))
                    .collect())
            }
            AlignBackend::Ctc(aligner) => {
                // Contract: language always eng + romanize; VoxTrans sets split_size.
                let text_path = write_temp_transcript(text)
                    .map_err(|err| format!("failed to write CTC transcript temp file: {err}"))?;
                let split_size = ctc_split_size(source_lang);
                let mut req = ctc_forced_aligner_rs::AlignRequest::from_paths(
                    audio_path,
                    &text_path.path,
                    "eng",
                );
                req.options.romanize = true;
                req.options.language = "eng".into();
                req.options.split_size = split_size.into();
                let result = aligner
                    .align(req)
                    .map_err(|err| format!("ctc alignment failed: {err:#}"))?;
                Ok(result
                    .items
                    .into_iter()
                    .map(|item| AlignedSpan::new(item.text, item.start, item.end))
                    .collect())
            }
        }
    }
}

fn ctc_split_size(lang: LanguageTag) -> &'static str {
    // CJK-like scripts usually have no spaces: "word" collapses to one span
    // per segment (seen on Yue tasks). Use char split for zh / yue / ja.
    match lang {
        LanguageTag::Zh | LanguageTag::Yue | LanguageTag::Ja => "char",
        _ => "word",
    }
}

struct TemporaryText {
    path: PathBuf,
}

impl TemporaryText {
    fn write(text: &str) -> Result<Self, std::io::Error> {
        let path = temp_text_path();
        std::fs::write(&path, text)?;
        Ok(Self { path })
    }
}

impl Drop for TemporaryText {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn write_temp_transcript(text: &str) -> Result<TemporaryText, std::io::Error> {
    TemporaryText::write(text)
}

fn temp_text_path() -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let pid = std::process::id();
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("voxtrans_ctc_align_{pid}_{seq}.txt"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ctc_split_size_cjk_char_else_word() {
        assert_eq!(ctc_split_size(LanguageTag::Zh), "char");
        assert_eq!(ctc_split_size(LanguageTag::Yue), "char");
        assert_eq!(ctc_split_size(LanguageTag::Ja), "char");
        assert_eq!(ctc_split_size(LanguageTag::En), "word");
        assert_eq!(ctc_split_size(LanguageTag::Ko), "word");
    }
}
