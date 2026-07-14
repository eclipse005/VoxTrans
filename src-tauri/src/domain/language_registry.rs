use crate::services::preferences_types::{AlignModel, AsrModel};

use super::language::{LanguageMetadata, LanguageTag, ALL_SOURCE_LANGUAGES};

#[derive(Debug, thiserror::Error)]
pub enum LanguageError {
    #[error("language {lang} is not supported by ASR model {model}")]
    UnsupportedForAsr { lang: LanguageTag, model: AsrModel },
    #[error("language {lang} is not supported by align model {model}")]
    UnsupportedForAlign { lang: LanguageTag, model: AlignModel },
}

struct AsrLanguageMapping {
    model: AsrModel,
    supported: &'static [LanguageTag],
    code_for: fn(LanguageTag) -> Option<&'static str>,
}

struct AlignLanguageMapping {
    model: AlignModel,
    supported: &'static [LanguageTag],
    code_for: fn(LanguageTag) -> Option<&'static str>,
}

fn qwen3_asr_code(lang: LanguageTag) -> Option<&'static str> {
    Some(match lang {
        LanguageTag::En => "english",
        LanguageTag::Zh => "chinese",
        LanguageTag::Yue => "cantonese",
        LanguageTag::Ja => "japanese",
        LanguageTag::Ko => "korean",
        LanguageTag::Fr => "french",
        LanguageTag::De => "german",
        LanguageTag::It => "italian",
        LanguageTag::Es => "spanish",
        LanguageTag::Pt => "portuguese",
        LanguageTag::Ru => "russian",
        LanguageTag::Ar => "arabic",
        LanguageTag::Id => "indonesian",
        LanguageTag::Th => "thai",
        LanguageTag::Vi => "vietnamese",
        LanguageTag::Tr => "turkish",
        LanguageTag::Hi => "hindi",
        LanguageTag::Ms => "malay",
        LanguageTag::Nl => "dutch",
        LanguageTag::Sv => "swedish",
        LanguageTag::Da => "danish",
        LanguageTag::Fi => "finnish",
        LanguageTag::Pl => "polish",
        LanguageTag::Cs => "czech",
        LanguageTag::Fil => "filipino",
        LanguageTag::Fa => "persian",
        LanguageTag::El => "greek",
        LanguageTag::Hu => "hungarian",
        LanguageTag::Mk => "macedonian",
        LanguageTag::Ro => "romanian",
    })
}

fn cohere_asr_code(lang: LanguageTag) -> Option<&'static str> {
    Some(match lang {
        // European
        LanguageTag::En => "en",
        LanguageTag::Fr => "fr",
        LanguageTag::De => "de",
        LanguageTag::It => "it",
        LanguageTag::Es => "es",
        LanguageTag::Pt => "pt",
        LanguageTag::El => "el",
        LanguageTag::Nl => "nl",
        LanguageTag::Pl => "pl",
        // APAC
        LanguageTag::Zh => "zh",
        LanguageTag::Ja => "ja",
        LanguageTag::Ko => "ko",
        LanguageTag::Vi => "vi",
        // MENA
        LanguageTag::Ar => "ar",
        _ => return None,
    })
}

/// MOSS always uses the fixed official English diarize prompt; language code is
/// unused by the engine. Still expose aligner-compatible source languages so
/// the UI language list is the intersection with ForcedAligner.
fn moss_asr_code(lang: LanguageTag) -> Option<&'static str> {
    match lang {
        LanguageTag::En
        | LanguageTag::Zh
        | LanguageTag::Yue
        | LanguageTag::Ja
        | LanguageTag::Ko
        | LanguageTag::Fr
        | LanguageTag::De
        | LanguageTag::It
        | LanguageTag::Es
        | LanguageTag::Pt
        | LanguageTag::Ru => Some("en"),
        _ => None,
    }
}

fn qwen3_align_code(lang: LanguageTag) -> Option<&'static str> {
    Some(match lang {
        LanguageTag::En => "English",
        LanguageTag::Zh => "Chinese",
        LanguageTag::Yue => "Cantonese",
        LanguageTag::Ja => "Japanese",
        LanguageTag::Ko => "Korean",
        LanguageTag::Fr => "French",
        LanguageTag::De => "German",
        LanguageTag::It => "Italian",
        LanguageTag::Es => "Spanish",
        LanguageTag::Pt => "Portuguese",
        LanguageTag::Ru => "Russian",
        _ => return None,
    })
}

static ASR_MAPPINGS: &[AsrLanguageMapping] = &[
    AsrLanguageMapping {
        model: AsrModel::Qwen3Asr06B,
        supported: LanguageTag::ALL,
        code_for: qwen3_asr_code,
    },
    AsrLanguageMapping {
        model: AsrModel::Qwen3Asr17B,
        supported: LanguageTag::ALL,
        code_for: qwen3_asr_code,
    },
    AsrLanguageMapping {
        model: AsrModel::CohereTranscribe032026,
        supported: &[
            // European
            LanguageTag::En,
            LanguageTag::Fr,
            LanguageTag::De,
            LanguageTag::It,
            LanguageTag::Es,
            LanguageTag::Pt,
            LanguageTag::El,
            LanguageTag::Nl,
            LanguageTag::Pl,
            // APAC
            LanguageTag::Zh,
            LanguageTag::Ja,
            LanguageTag::Ko,
            LanguageTag::Vi,
            // MENA
            LanguageTag::Ar,
        ],
        code_for: cohere_asr_code,
    },
    AsrLanguageMapping {
        model: AsrModel::MossTranscribeDiarize,
        supported: &[
            LanguageTag::En,
            LanguageTag::Zh,
            LanguageTag::Yue,
            LanguageTag::Ja,
            LanguageTag::Ko,
            LanguageTag::Fr,
            LanguageTag::De,
            LanguageTag::It,
            LanguageTag::Es,
            LanguageTag::Pt,
            LanguageTag::Ru,
        ],
        code_for: moss_asr_code,
    },
];

static ALIGN_MAPPINGS: &[AlignLanguageMapping] = &[
    AlignLanguageMapping {
        model: AlignModel::Qwen3ForcedAligner06B,
        supported: &[
            LanguageTag::En,
            LanguageTag::Zh,
            LanguageTag::Yue,
            LanguageTag::Ja,
            LanguageTag::Ko,
            LanguageTag::Fr,
            LanguageTag::De,
            LanguageTag::It,
            LanguageTag::Es,
            LanguageTag::Pt,
            LanguageTag::Ru,
        ],
        code_for: qwen3_align_code,
    },
];

pub struct LanguageRegistry;

impl LanguageRegistry {
    pub fn all_source_languages() -> &'static [LanguageMetadata] {
        ALL_SOURCE_LANGUAGES
    }

    pub fn supported_for(
        asr: AsrModel,
        align: AlignModel,
    ) -> Vec<&'static LanguageMetadata> {
        let asr_supported = Self::asr_supported_set(asr);
        let align_supported = Self::align_supported_set(align);
        ALL_SOURCE_LANGUAGES
            .iter()
            .filter(|m| asr_supported.contains(&m.tag) && align_supported.contains(&m.tag))
            .collect()
    }

    pub fn asr_code(asr: AsrModel, lang: LanguageTag) -> Result<&'static str, LanguageError> {
        let mapping = ASR_MAPPINGS
            .iter()
            .find(|m| m.model == asr)
            .ok_or_else(|| LanguageError::UnsupportedForAsr {
                lang,
                model: asr,
            })?;
        (mapping.code_for)(lang).ok_or_else(|| LanguageError::UnsupportedForAsr {
            lang,
            model: asr,
        })
    }

    pub fn align_code(
        align: AlignModel,
        lang: LanguageTag,
    ) -> Result<&'static str, LanguageError> {
        let mapping = ALIGN_MAPPINGS
            .iter()
            .find(|m| m.model == align)
            .ok_or_else(|| LanguageError::UnsupportedForAlign {
                lang,
                model: align,
            })?;
        (mapping.code_for)(lang).ok_or_else(|| LanguageError::UnsupportedForAlign {
            lang,
            model: align,
        })
    }

    fn asr_supported_set(asr: AsrModel) -> std::collections::HashSet<LanguageTag> {
        ASR_MAPPINGS
            .iter()
            .find(|m| m.model == asr)
            .map(|m| m.supported.iter().copied().collect())
            .unwrap_or_default()
    }

    fn align_supported_set(align: AlignModel) -> std::collections::HashSet<LanguageTag> {
        ALIGN_MAPPINGS
            .iter()
            .find(|m| m.model == align)
            .map(|m| m.supported.iter().copied().collect())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cohere_does_not_support_russian() {
        assert!(LanguageRegistry::asr_code(AsrModel::CohereTranscribe032026, LanguageTag::Ru).is_err());
    }

    #[test]
    fn qwen_supports_all_languages() {
        for tag in LanguageTag::ALL {
            assert!(LanguageRegistry::asr_code(AsrModel::Qwen3Asr06B, *tag).is_ok());
        }
    }

    #[test]
    fn qwen_aligner_supports_eleven() {
        let supported = LanguageRegistry::supported_for(
            AsrModel::Qwen3Asr06B,
            AlignModel::Qwen3ForcedAligner06B,
        );
        assert_eq!(supported.len(), 11);
        assert!(supported.iter().any(|m| m.tag == LanguageTag::Zh));
        assert!(supported.iter().any(|m| m.tag == LanguageTag::Ru));
        assert!(!supported.iter().any(|m| m.tag == LanguageTag::Ar));
    }

    #[test]
    fn cohere_supports_fourteen_languages() {
        let mut count = 0;
        for tag in LanguageTag::ALL {
            if LanguageRegistry::asr_code(AsrModel::CohereTranscribe032026, *tag).is_ok() {
                count += 1;
            }
        }
        assert_eq!(count, 14);
        assert!(LanguageRegistry::asr_code(AsrModel::CohereTranscribe032026, LanguageTag::Ar).is_ok());
        assert!(LanguageRegistry::asr_code(AsrModel::CohereTranscribe032026, LanguageTag::Vi).is_ok());
        assert!(LanguageRegistry::asr_code(AsrModel::CohereTranscribe032026, LanguageTag::El).is_ok());
        assert!(LanguageRegistry::asr_code(AsrModel::CohereTranscribe032026, LanguageTag::Yue).is_err());
    }

    #[test]
    fn cohere_aligner_intersection_is_nine() {
        let supported = LanguageRegistry::supported_for(
            AsrModel::CohereTranscribe032026,
            AlignModel::Qwen3ForcedAligner06B,
        );
        assert_eq!(supported.len(), 9);
        assert!(!supported.iter().any(|m| m.tag == LanguageTag::Ru));
        assert!(!supported.iter().any(|m| m.tag == LanguageTag::Ar));
        assert!(supported.iter().any(|m| m.tag == LanguageTag::Zh));
    }

    #[test]
    fn code_outputs_match_existing_mappings() {
        assert_eq!(LanguageRegistry::asr_code(AsrModel::Qwen3Asr06B, LanguageTag::Zh).unwrap(), "chinese");
        assert_eq!(LanguageRegistry::asr_code(AsrModel::CohereTranscribe032026, LanguageTag::Zh).unwrap(), "zh");
        assert_eq!(LanguageRegistry::align_code(AlignModel::Qwen3ForcedAligner06B, LanguageTag::Zh).unwrap(), "Chinese");
    }

    #[test]
    fn moss_aligner_intersection_is_eleven() {
        let supported = LanguageRegistry::supported_for(
            AsrModel::MossTranscribeDiarize,
            AlignModel::Qwen3ForcedAligner06B,
        );
        assert_eq!(supported.len(), 11);
        assert_eq!(
            LanguageRegistry::asr_code(AsrModel::MossTranscribeDiarize, LanguageTag::Zh).unwrap(),
            "en"
        );
        assert!(LanguageRegistry::asr_code(AsrModel::MossTranscribeDiarize, LanguageTag::Ar).is_err());
    }
}
