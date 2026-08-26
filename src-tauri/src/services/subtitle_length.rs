// The preset enum is defined once in `preferences_types` (the serde/ts-rs
// source of truth) and reused here so the domain limits logic and the DB/
// settings layer can never drift apart. Re-exported for call-site locality.
pub use crate::services::preferences_types::SubtitleLengthPreset;

/// Parse a preset from its lowercase string id. Thin wrapper over the
/// canonical enum's `parse` so there is a single mapping to maintain.
pub fn subtitle_length_preset_from_id(value: &str) -> SubtitleLengthPreset {
    SubtitleLengthPreset::parse(value)
}

pub fn target_limit_for_preset(target_lang: &str, preset_id: &str) -> u32 {
    target_limit_for_language(target_lang, subtitle_length_preset_from_id(preset_id))
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
        .find(['-', '_'])
        .unwrap_or(trimmed.len());
    trimmed[..end].to_ascii_lowercase()
}

fn target_word_limits(preset: SubtitleLengthPreset) -> u32 {
    match preset {
        SubtitleLengthPreset::Short => 10,
        SubtitleLengthPreset::Standard => 12,
        SubtitleLengthPreset::Loose => 16,
    }
}

fn medium_word_target_limits(preset: SubtitleLengthPreset) -> u32 {
    match preset {
        SubtitleLengthPreset::Short => 10,
        SubtitleLengthPreset::Standard => 12,
        SubtitleLengthPreset::Loose => 15,
    }
}

fn long_word_target_limits(preset: SubtitleLengthPreset) -> u32 {
    match preset {
        SubtitleLengthPreset::Short => 9,
        SubtitleLengthPreset::Standard => 11,
        SubtitleLengthPreset::Loose => 14,
    }
}

fn vietnamese_limits(preset: SubtitleLengthPreset) -> u32 {
    match preset {
        SubtitleLengthPreset::Short => 11,
        SubtitleLengthPreset::Standard => 14,
        SubtitleLengthPreset::Loose => 18,
    }
}

fn cjk_limits(preset: SubtitleLengthPreset) -> u32 {
    match preset {
        SubtitleLengthPreset::Short => 16,
        SubtitleLengthPreset::Standard => 22,
        SubtitleLengthPreset::Loose => 28,
    }
}

fn korean_limits(preset: SubtitleLengthPreset) -> u32 {
    match preset {
        SubtitleLengthPreset::Short => 15,
        SubtitleLengthPreset::Standard => 20,
        SubtitleLengthPreset::Loose => 26,
    }
}

fn thai_limits(preset: SubtitleLengthPreset) -> u32 {
    match preset {
        SubtitleLengthPreset::Short => 24,
        SubtitleLengthPreset::Standard => 32,
        SubtitleLengthPreset::Loose => 42,
    }
}
