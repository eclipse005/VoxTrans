#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubtitleLengthPreset {
    Short,
    Standard,
    Loose,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SubtitleLengthLimits {
    pub source_limit: u32,
    pub target_limit: u32,
}

pub const DEFAULT_SUBTITLE_LENGTH_PRESET: &str = "standard";

pub fn normalize_subtitle_length_preset(value: &str) -> String {
    subtitle_length_preset_from_id(value).as_id().to_string()
}

pub fn subtitle_length_preset_from_id(value: &str) -> SubtitleLengthPreset {
    match value.trim() {
        "short" => SubtitleLengthPreset::Short,
        "loose" => SubtitleLengthPreset::Loose,
        "standard" => SubtitleLengthPreset::Standard,
        _ => SubtitleLengthPreset::Standard,
    }
}

pub fn source_limit_for_preset(source_lang: &str, preset_id: &str) -> u32 {
    source_limit_for_language(source_lang, subtitle_length_preset_from_id(preset_id))
}

pub fn target_limit_for_preset(target_lang: &str, preset_id: &str) -> u32 {
    target_limit_for_language(target_lang, subtitle_length_preset_from_id(preset_id))
}

pub fn effective_subtitle_limits_from_preset(
    source_lang: &str,
    target_lang: &str,
    preset_id: &str,
) -> SubtitleLengthLimits {
    effective_subtitle_limits(
        source_lang,
        target_lang,
        subtitle_length_preset_from_id(preset_id),
    )
}

pub fn effective_subtitle_limits(
    source_lang: &str,
    target_lang: &str,
    preset: SubtitleLengthPreset,
) -> SubtitleLengthLimits {
    SubtitleLengthLimits {
        source_limit: source_limit_for_language(source_lang, preset),
        target_limit: target_limit_for_language(target_lang, preset),
    }
}

impl SubtitleLengthPreset {
    pub fn as_id(self) -> &'static str {
        match self {
            SubtitleLengthPreset::Short => "short",
            SubtitleLengthPreset::Standard => "standard",
            SubtitleLengthPreset::Loose => "loose",
        }
    }
}

fn source_limit_for_language(lang: &str, preset: SubtitleLengthPreset) -> u32 {
    match language_key(lang).as_str() {
        "zh" | "yue" | "ja" => cjk_limits(preset),
        "ko" => korean_limits(preset),
        "de" | "fr" => long_word_source_limits(preset),
        "en" | "it" | "es" | "pt" => source_word_limits(preset),
        _ => source_word_limits(preset),
    }
}

fn target_limit_for_language(lang: &str, preset: SubtitleLengthPreset) -> u32 {
    match language_key(lang).as_str() {
        "zh" | "ja" => cjk_limits(preset),
        "ko" => korean_limits(preset),
        "th" => thai_limits(preset),
        "vi" => vietnamese_limits(preset),
        "de" | "tr" | "pl" | "ru" => long_word_target_limits(preset),
        "fr" | "es" | "it" | "pt" | "nl" | "id" => medium_word_target_limits(preset),
        "en" | "ar" => target_word_limits(preset),
        _ => target_word_limits(preset),
    }
}

fn language_key(lang: &str) -> String {
    let trimmed = lang.trim();
    let end = trimmed
        .find(|ch| ch == '-' || ch == '_')
        .unwrap_or(trimmed.len());
    trimmed[..end].to_ascii_lowercase()
}

fn source_word_limits(preset: SubtitleLengthPreset) -> u32 {
    match preset {
        SubtitleLengthPreset::Short => 16,
        SubtitleLengthPreset::Standard => 20,
        SubtitleLengthPreset::Loose => 28,
    }
}

fn long_word_source_limits(preset: SubtitleLengthPreset) -> u32 {
    match preset {
        SubtitleLengthPreset::Short => 14,
        SubtitleLengthPreset::Standard => 18,
        SubtitleLengthPreset::Loose => 25,
    }
}

fn target_word_limits(preset: SubtitleLengthPreset) -> u32 {
    match preset {
        SubtitleLengthPreset::Short => 12,
        SubtitleLengthPreset::Standard => 16,
        SubtitleLengthPreset::Loose => 22,
    }
}

fn medium_word_target_limits(preset: SubtitleLengthPreset) -> u32 {
    match preset {
        SubtitleLengthPreset::Short => 12,
        SubtitleLengthPreset::Standard => 15,
        SubtitleLengthPreset::Loose => 21,
    }
}

fn long_word_target_limits(preset: SubtitleLengthPreset) -> u32 {
    match preset {
        SubtitleLengthPreset::Short => 11,
        SubtitleLengthPreset::Standard => 14,
        SubtitleLengthPreset::Loose => 20,
    }
}

fn vietnamese_limits(preset: SubtitleLengthPreset) -> u32 {
    match preset {
        SubtitleLengthPreset::Short => 14,
        SubtitleLengthPreset::Standard => 18,
        SubtitleLengthPreset::Loose => 24,
    }
}

fn cjk_limits(preset: SubtitleLengthPreset) -> u32 {
    match preset {
        SubtitleLengthPreset::Short => 22,
        SubtitleLengthPreset::Standard => 28,
        SubtitleLengthPreset::Loose => 38,
    }
}

fn korean_limits(preset: SubtitleLengthPreset) -> u32 {
    match preset {
        SubtitleLengthPreset::Short => 20,
        SubtitleLengthPreset::Standard => 26,
        SubtitleLengthPreset::Loose => 34,
    }
}

fn thai_limits(preset: SubtitleLengthPreset) -> u32 {
    match preset {
        SubtitleLengthPreset::Short => 32,
        SubtitleLengthPreset::Standard => 42,
        SubtitleLengthPreset::Loose => 56,
    }
}
