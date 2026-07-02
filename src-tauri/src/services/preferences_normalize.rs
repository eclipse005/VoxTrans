use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};

use super::preferences_types::{
    AlignModel, AsrModel, DemucsModel, Provider, SavedSettings, SubtitleBurnMode,
    SubtitleLayoutStyle, SubtitleLineStyle, SubtitleLengthPreset, SubtitleRenderStyle,
    TerminologyGroup, TerminologyTerm,
};

pub fn default_settings() -> SavedSettings {
    SavedSettings {
        provider: Provider::Cpu,
        chunk_target_seconds: 30,
        subtitle_length_preset: SubtitleLengthPreset::Standard,
        asr_model: AsrModel::default(),
        align_model: AlignModel::default(),
        demucs_model: DemucsModel::default(),
        enable_vocal_separation: false,
        translate_api_key: String::new(),
        translate_base_url: "https://api.deepseek.com/v1".to_string(),
        translate_model: "deepseek-chat".to_string(),
        llm_concurrency: 4,
        terminology_groups: normalize_terminology_groups(Vec::new()),
        active_terminology_group_id: String::new(),
        enable_subtitle_beautify: true,
        enable_click_sound: true,
        auto_burn_hard_subtitle: false,
        subtitle_burn_mode: SubtitleBurnMode::BilingualSourceFirst,
        subtitle_render_style: SubtitleRenderStyle::default(),
        flat_srt_output: false,
        flat_srt_items: vec![SubtitleBurnMode::Source, SubtitleBurnMode::Target],
        enable_vision_assist: false,
    }
}

pub(super) fn normalize_saved_settings(settings: SavedSettings) -> SavedSettings {
    SavedSettings {
        provider: settings.provider,
        chunk_target_seconds: settings.chunk_target_seconds.clamp(30, 60),
        subtitle_length_preset: settings.subtitle_length_preset,
        asr_model: settings.asr_model,
        align_model: settings.align_model,
        demucs_model: settings.demucs_model,
        enable_vocal_separation: settings.enable_vocal_separation,
        translate_api_key: settings.translate_api_key.trim().to_string(),
        translate_base_url: {
            let trimmed = settings.translate_base_url.trim();
            if trimmed.is_empty() {
                "https://api.deepseek.com/v1".to_string()
            } else {
                trimmed.to_string()
            }
        },
        translate_model: {
            let trimmed = settings.translate_model.trim();
            if trimmed.is_empty() {
                "deepseek-chat".to_string()
            } else {
                trimmed.to_string()
            }
        },
        llm_concurrency: settings.llm_concurrency.max(1),
        terminology_groups: normalize_terminology_groups(settings.terminology_groups),
        active_terminology_group_id: settings.active_terminology_group_id.clone(),
        enable_subtitle_beautify: settings.enable_subtitle_beautify,
        enable_click_sound: settings.enable_click_sound,
        auto_burn_hard_subtitle: settings.auto_burn_hard_subtitle,
        subtitle_burn_mode: settings.subtitle_burn_mode,
        subtitle_render_style: normalize_subtitle_render_style(settings.subtitle_render_style),
        flat_srt_output: settings.flat_srt_output,
        flat_srt_items: normalize_flat_srt_items(settings.flat_srt_items),
        enable_vision_assist: settings.enable_vision_assist,
    }
}

fn normalize_flat_srt_items(items: Vec<SubtitleBurnMode>) -> Vec<SubtitleBurnMode> {
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    for item in items {
        if seen.insert(item) {
            result.push(item);
        }
    }
    if result.is_empty() {
        vec![SubtitleBurnMode::Source, SubtitleBurnMode::Target]
    } else {
        result
    }
}

fn normalize_subtitle_render_style(style: SubtitleRenderStyle) -> SubtitleRenderStyle {
    let defaults = SubtitleRenderStyle::default();
    SubtitleRenderStyle {
        source: normalize_subtitle_line_style(style.source, defaults.source),
        target: normalize_subtitle_line_style(style.target, defaults.target),
        layout: SubtitleLayoutStyle {
            margin_v: style.layout.margin_v.clamp(0, 200),
            alignment: match style.layout.alignment {
                1..=3 => style.layout.alignment,
                _ => 2,
            },
            bilingual_line_gap: style.layout.bilingual_line_gap.clamp(0, 140),
        },
    }
}

fn normalize_subtitle_line_style(
    style: SubtitleLineStyle,
    fallback: SubtitleLineStyle,
) -> SubtitleLineStyle {
    SubtitleLineStyle {
        font_family: {
            let value = style.font_family.trim();
            if value.is_empty() {
                fallback.font_family
            } else {
                value.to_string()
            }
        },
        font_size: style.font_size.clamp(16, 96),
        primary_color: normalize_hex_color(&style.primary_color, &fallback.primary_color),
        outline_color: normalize_hex_color(&style.outline_color, &fallback.outline_color),
        back_color: normalize_hex_color(&style.back_color, &fallback.back_color),
        // libass renders nothing at outline=0.0, so clamp to 0.1 minimum.
        outline: style.outline.clamp(0.1, 8.0),
        shadow: style.shadow.clamp(0.0, 8.0),
        border_style: style.border_style,
        border_opacity: style.border_opacity.clamp(0, 100),
    }
}

fn normalize_hex_color(value: &str, fallback: &str) -> String {
    let trimmed = value.trim();
    let is_hex = trimmed.len() == 7
        && trimmed.starts_with('#')
        && trimmed.chars().skip(1).all(|c| c.is_ascii_hexdigit());
    if is_hex {
        trimmed.to_ascii_uppercase()
    } else {
        fallback.to_string()
    }
}

fn normalize_terminology_groups(groups: Vec<TerminologyGroup>) -> Vec<TerminologyGroup> {
    let mut seen_group_ids = HashSet::new();
    let mut normalized = Vec::new();

    for (group_idx, group) in groups.into_iter().enumerate() {
        let mut group_id = group.id.trim().to_string();
        if group_id.is_empty() || !seen_group_ids.insert(group_id.clone()) {
            group_id = make_entity_id("group", group_idx);
            seen_group_ids.insert(group_id.clone());
        }

        let name = {
            let trimmed = group.name.trim();
            if trimmed.is_empty() {
                "默认".to_string()
            } else {
                trimmed.to_string()
            }
        };

        let terms = normalize_terminology_terms(group.terms, group_idx);

        normalized.push(TerminologyGroup {
            id: group_id,
            name,
            terms,
        });
    }

    if normalized.is_empty() {
        return vec![default_terminology_group()];
    }

    normalized
}

fn normalize_terminology_terms(
    terms: Vec<TerminologyTerm>,
    group_idx: usize,
) -> Vec<TerminologyTerm> {
    let mut normalized = Vec::new();
    let mut seen_term_ids = HashSet::new();

    for (term_idx, term) in terms.into_iter().enumerate() {
        let origin = term.origin.trim();
        let target = term.target.trim();
        if origin.is_empty() || target.is_empty() {
            continue;
        }

        let mut term_id = term.id.trim().to_string();
        if term_id.is_empty() || !seen_term_ids.insert(term_id.clone()) {
            let seq = group_idx.saturating_mul(10_000).saturating_add(term_idx);
            term_id = make_entity_id("term", seq);
            seen_term_ids.insert(term_id.clone());
        }

        normalized.push(TerminologyTerm {
            id: term_id,
            origin: origin.to_string(),
            target: target.to_string(),
            note: term.note.trim().to_string(),
        });
    }

    normalized
}

fn default_terminology_group() -> TerminologyGroup {
    TerminologyGroup {
        id: make_entity_id("group", 0),
        name: "默认".to_string(),
        terms: Vec::new(),
    }
}

fn make_entity_id(prefix: &str, seq: usize) -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("{prefix}-{millis}-{seq}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_chunk_target_seconds_is_thirty() {
        assert_eq!(default_settings().chunk_target_seconds, 30);
    }

    #[test]
    fn normalize_saved_settings_clamps_chunk_target_upper_bound() {
        let mut settings = default_settings();
        settings.chunk_target_seconds = 61;

        let normalized = normalize_saved_settings(settings);

        assert_eq!(normalized.chunk_target_seconds, 60);
    }
}
