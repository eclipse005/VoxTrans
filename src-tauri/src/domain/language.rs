use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "lowercase")]
pub enum LanguageTag {
    En,
    Zh,
    Yue,
    Ja,
    Ko,
    Fr,
    De,
    It,
    Es,
    Pt,
    Ru,
}

impl LanguageTag {
    pub const ALL: &'static [LanguageTag] = &[
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
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            LanguageTag::En => "en",
            LanguageTag::Zh => "zh",
            LanguageTag::Yue => "yue",
            LanguageTag::Ja => "ja",
            LanguageTag::Ko => "ko",
            LanguageTag::Fr => "fr",
            LanguageTag::De => "de",
            LanguageTag::It => "it",
            LanguageTag::Es => "es",
            LanguageTag::Pt => "pt",
            LanguageTag::Ru => "ru",
        }
    }
}

impl fmt::Display for LanguageTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl FromStr for LanguageTag {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "en" => Ok(LanguageTag::En),
            "zh" => Ok(LanguageTag::Zh),
            "yue" => Ok(LanguageTag::Yue),
            "ja" => Ok(LanguageTag::Ja),
            "ko" => Ok(LanguageTag::Ko),
            "fr" => Ok(LanguageTag::Fr),
            "de" => Ok(LanguageTag::De),
            "it" => Ok(LanguageTag::It),
            "es" => Ok(LanguageTag::Es),
            "pt" => Ok(LanguageTag::Pt),
            "ru" => Ok(LanguageTag::Ru),
            _ => Err(format!("unsupported language tag: {s}")),
        }
    }
}

pub struct LanguageMetadata {
    pub tag: LanguageTag,
    pub display_name: &'static str,
    pub display_name_en: &'static str,
    pub short_badge: &'static str,
}

pub const ALL_SOURCE_LANGUAGES: &[LanguageMetadata] = &[
    LanguageMetadata { tag: LanguageTag::En, display_name: "English", display_name_en: "English", short_badge: "EN" },
    LanguageMetadata { tag: LanguageTag::Zh, display_name: "中文普通话", display_name_en: "Mandarin Chinese", short_badge: "中" },
    LanguageMetadata { tag: LanguageTag::Yue, display_name: "粤语", display_name_en: "Cantonese", short_badge: "粤" },
    LanguageMetadata { tag: LanguageTag::Ja, display_name: "日本語", display_name_en: "Japanese", short_badge: "JP" },
    LanguageMetadata { tag: LanguageTag::Ko, display_name: "한국어", display_name_en: "Korean", short_badge: "KO" },
    LanguageMetadata { tag: LanguageTag::Fr, display_name: "Français", display_name_en: "French", short_badge: "FR" },
    LanguageMetadata { tag: LanguageTag::De, display_name: "Deutsch", display_name_en: "German", short_badge: "DE" },
    LanguageMetadata { tag: LanguageTag::It, display_name: "Italiano", display_name_en: "Italian", short_badge: "IT" },
    LanguageMetadata { tag: LanguageTag::Es, display_name: "Español", display_name_en: "Spanish", short_badge: "ES" },
    LanguageMetadata { tag: LanguageTag::Pt, display_name: "Português", display_name_en: "Portuguese", short_badge: "PT" },
    LanguageMetadata { tag: LanguageTag::Ru, display_name: "Русский", display_name_en: "Russian", short_badge: "RU" },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_all_tags() {
        for tag in LanguageTag::ALL {
            let parsed: LanguageTag = tag.as_str().parse().unwrap();
            assert_eq!(*tag, parsed);
        }
    }

    #[test]
    fn rejects_unknown_tag() {
        assert!("xyz".parse::<LanguageTag>().is_err());
    }
}
