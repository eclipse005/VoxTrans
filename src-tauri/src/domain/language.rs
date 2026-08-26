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
    Ar,
    Id,
    Th,
    Vi,
    Tr,
    Hi,
    Ms,
    Nl,
    Sv,
    Da,
    Fi,
    Pl,
    Cs,
    Fil,
    Fa,
    El,
    Hu,
    Mk,
    Ro,
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
        LanguageTag::Ar,
        LanguageTag::Id,
        LanguageTag::Th,
        LanguageTag::Vi,
        LanguageTag::Tr,
        LanguageTag::Hi,
        LanguageTag::Ms,
        LanguageTag::Nl,
        LanguageTag::Sv,
        LanguageTag::Da,
        LanguageTag::Fi,
        LanguageTag::Pl,
        LanguageTag::Cs,
        LanguageTag::Fil,
        LanguageTag::Fa,
        LanguageTag::El,
        LanguageTag::Hu,
        LanguageTag::Mk,
        LanguageTag::Ro,
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
            LanguageTag::Ar => "ar",
            LanguageTag::Id => "id",
            LanguageTag::Th => "th",
            LanguageTag::Vi => "vi",
            LanguageTag::Tr => "tr",
            LanguageTag::Hi => "hi",
            LanguageTag::Ms => "ms",
            LanguageTag::Nl => "nl",
            LanguageTag::Sv => "sv",
            LanguageTag::Da => "da",
            LanguageTag::Fi => "fi",
            LanguageTag::Pl => "pl",
            LanguageTag::Cs => "cs",
            LanguageTag::Fil => "fil",
            LanguageTag::Fa => "fa",
            LanguageTag::El => "el",
            LanguageTag::Hu => "hu",
            LanguageTag::Mk => "mk",
            LanguageTag::Ro => "ro",
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
            "ar" => Ok(LanguageTag::Ar),
            "id" => Ok(LanguageTag::Id),
            "th" => Ok(LanguageTag::Th),
            "vi" => Ok(LanguageTag::Vi),
            "tr" => Ok(LanguageTag::Tr),
            "hi" => Ok(LanguageTag::Hi),
            "ms" => Ok(LanguageTag::Ms),
            "nl" => Ok(LanguageTag::Nl),
            "sv" => Ok(LanguageTag::Sv),
            "da" => Ok(LanguageTag::Da),
            "fi" => Ok(LanguageTag::Fi),
            "pl" => Ok(LanguageTag::Pl),
            "cs" => Ok(LanguageTag::Cs),
            "fil" => Ok(LanguageTag::Fil),
            "fa" => Ok(LanguageTag::Fa),
            "el" => Ok(LanguageTag::El),
            "hu" => Ok(LanguageTag::Hu),
            "mk" => Ok(LanguageTag::Mk),
            "ro" => Ok(LanguageTag::Ro),
            _ => Err(format!("unsupported language tag: {s}")),
        }
    }
}

pub struct LanguageMetadata {
    pub tag: LanguageTag,
    pub display_name: &'static str,
    pub short_badge: &'static str,
}

pub const ALL_SOURCE_LANGUAGES: &[LanguageMetadata] = &[
    LanguageMetadata { tag: LanguageTag::En, display_name: "English", short_badge: "EN" },
    LanguageMetadata { tag: LanguageTag::Zh, display_name: "中文普通话", short_badge: "中" },
    LanguageMetadata { tag: LanguageTag::Yue, display_name: "粤语", short_badge: "粤" },
    LanguageMetadata { tag: LanguageTag::Ja, display_name: "日本語", short_badge: "JP" },
    LanguageMetadata { tag: LanguageTag::Ko, display_name: "한국어", short_badge: "KO" },
    LanguageMetadata { tag: LanguageTag::Fr, display_name: "Français", short_badge: "FR" },
    LanguageMetadata { tag: LanguageTag::De, display_name: "Deutsch", short_badge: "DE" },
    LanguageMetadata { tag: LanguageTag::It, display_name: "Italiano", short_badge: "IT" },
    LanguageMetadata { tag: LanguageTag::Es, display_name: "Español", short_badge: "ES" },
    LanguageMetadata { tag: LanguageTag::Pt, display_name: "Português", short_badge: "PT" },
    LanguageMetadata { tag: LanguageTag::Ru, display_name: "Русский", short_badge: "RU" },
    LanguageMetadata { tag: LanguageTag::Ar, display_name: "العربية", short_badge: "AR" },
    LanguageMetadata { tag: LanguageTag::Id, display_name: "Bahasa Indonesia", short_badge: "ID" },
    LanguageMetadata { tag: LanguageTag::Th, display_name: "ไทย", short_badge: "TH" },
    LanguageMetadata { tag: LanguageTag::Vi, display_name: "Tiếng Việt", short_badge: "VI" },
    LanguageMetadata { tag: LanguageTag::Tr, display_name: "Türkçe", short_badge: "TR" },
    LanguageMetadata { tag: LanguageTag::Hi, display_name: "हिन्दी", short_badge: "HI" },
    LanguageMetadata { tag: LanguageTag::Ms, display_name: "Bahasa Melayu", short_badge: "MS" },
    LanguageMetadata { tag: LanguageTag::Nl, display_name: "Nederlands", short_badge: "NL" },
    LanguageMetadata { tag: LanguageTag::Sv, display_name: "Svenska", short_badge: "SV" },
    LanguageMetadata { tag: LanguageTag::Da, display_name: "Dansk", short_badge: "DA" },
    LanguageMetadata { tag: LanguageTag::Fi, display_name: "Suomi", short_badge: "FI" },
    LanguageMetadata { tag: LanguageTag::Pl, display_name: "Polski", short_badge: "PL" },
    LanguageMetadata { tag: LanguageTag::Cs, display_name: "Čeština", short_badge: "CS" },
    LanguageMetadata { tag: LanguageTag::Fil, display_name: "Filipino", short_badge: "FIL" },
    LanguageMetadata { tag: LanguageTag::Fa, display_name: "فارسی", short_badge: "FA" },
    LanguageMetadata { tag: LanguageTag::El, display_name: "Ελληνικά", short_badge: "EL" },
    LanguageMetadata { tag: LanguageTag::Hu, display_name: "Magyar", short_badge: "HU" },
    LanguageMetadata { tag: LanguageTag::Mk, display_name: "Македонски", short_badge: "MK" },
    LanguageMetadata { tag: LanguageTag::Ro, display_name: "Română", short_badge: "RO" },
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
