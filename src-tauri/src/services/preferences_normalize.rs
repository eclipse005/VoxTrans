use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};

use super::preferences_types::{
    SavedSettings, SubtitleLayoutStyle, SubtitleLineStyle, SubtitleRenderStyle, TerminologyGroup,
    TerminologyTerm,
};

pub(super) fn default_settings() -> SavedSettings {
    SavedSettings {
        provider: "cpu".to_string(),
        chunk_target_seconds: 180,
        subtitle_max_words_per_segment: 20,
        subtitle_length_reference: 28,
        asr_model: crate::services::model::DEFAULT_ASR_MODEL.to_string(),
        align_model: "Qwen3-ForcedAligner-0.6B".to_string(),
        demucs_model: "htdemucs_ft".to_string(),
        enable_vocal_separation: false,
        translate_api_key: String::new(),
        translate_base_url: "https://api.openai.com/v1".to_string(),
        translate_model: "gpt-4.1-mini".to_string(),
        llm_concurrency: 4,
        terminology_groups: normalize_terminology_groups(Vec::new()),
        enable_terminology: true,
        enable_subtitle_beautify: true,
        auto_burn_hard_subtitle: false,
        subtitle_burn_mode: "bilingualSourceFirst".to_string(),
        subtitle_render_style: SubtitleRenderStyle::default(),
    }
}

pub(super) fn normalize_saved_settings(settings: SavedSettings) -> SavedSettings {
    SavedSettings {
        provider: {
            let trimmed = settings.provider.trim();
            if trimmed.is_empty() {
                "cpu".to_string()
            } else {
                trimmed.to_string()
            }
        },
        chunk_target_seconds: settings.chunk_target_seconds.clamp(30, 300),
        subtitle_max_words_per_segment: settings.subtitle_max_words_per_segment.clamp(8, 40),
        subtitle_length_reference: settings.subtitle_length_reference.clamp(8, 80),
        asr_model: {
            let trimmed = settings.asr_model.trim();
            if trimmed.is_empty() {
                crate::services::model::DEFAULT_ASR_MODEL.to_string()
            } else if trimmed == "parakeet-tdt-0.6b-v2" || trimmed == "cohere-transcribe" {
                crate::services::model::DEFAULT_ASR_MODEL.to_string()
            } else {
                trimmed.to_string()
            }
        },
        align_model: {
            let trimmed = settings.align_model.trim();
            if trimmed.is_empty() {
                "Qwen3-ForcedAligner-0.6B".to_string()
            } else {
                trimmed.to_string()
            }
        },
        demucs_model: match settings.demucs_model.trim() {
            "htdemucs_ft" => "htdemucs_ft".to_string(),
            _ => "htdemucs_ft".to_string(),
        },
        enable_vocal_separation: settings.enable_vocal_separation,
        translate_api_key: settings.translate_api_key.trim().to_string(),
        translate_base_url: {
            let trimmed = settings.translate_base_url.trim();
            if trimmed.is_empty() {
                "https://api.openai.com/v1".to_string()
            } else {
                trimmed.to_string()
            }
        },
        translate_model: {
            let trimmed = settings.translate_model.trim();
            if trimmed.is_empty() {
                "gpt-4.1-mini".to_string()
            } else {
                trimmed.to_string()
            }
        },
        llm_concurrency: settings.llm_concurrency.max(1),
        terminology_groups: normalize_terminology_groups(settings.terminology_groups),
        enable_terminology: settings.enable_terminology,
        enable_subtitle_beautify: settings.enable_subtitle_beautify,
        auto_burn_hard_subtitle: settings.auto_burn_hard_subtitle,
        subtitle_burn_mode: normalize_subtitle_burn_mode(&settings.subtitle_burn_mode).to_string(),
        subtitle_render_style: normalize_subtitle_render_style(settings.subtitle_render_style),
    }
}

fn normalize_subtitle_burn_mode(value: &str) -> &str {
    match value.trim() {
        "source" | "target" | "bilingualSourceFirst" | "bilingualTargetFirst" => value.trim(),
        _ => "bilingualSourceFirst",
    }
}

fn normalize_subtitle_render_style(style: SubtitleRenderStyle) -> SubtitleRenderStyle {
    SubtitleRenderStyle {
        source: normalize_subtitle_line_style(
            style.source,
            SubtitleLineStyle {
                font_family: "Arial".to_string(),
                font_size: 44,
                primary_color: "#FFFFFF".to_string(),
                outline_color: "#101010".to_string(),
                back_color: "#000000".to_string(),
                outline: 2.5,
                shadow: 1.0,
                border_style: "outline".to_string(),
                border_opacity: 88,
            },
        ),
        target: normalize_subtitle_line_style(
            style.target,
            SubtitleLineStyle {
                font_family: "Microsoft YaHei".to_string(),
                font_size: 40,
                primary_color: "#EAF6FF".to_string(),
                outline_color: "#101010".to_string(),
                back_color: "#000000".to_string(),
                outline: 2.5,
                shadow: 1.0,
                border_style: "outline".to_string(),
                border_opacity: 88,
            },
        ),
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
        outline: style.outline.clamp(0.0, 8.0),
        shadow: style.shadow.clamp(0.0, 8.0),
        border_style: normalize_border_style(&style.border_style).to_string(),
        border_opacity: style.border_opacity.clamp(0, 100),
    }
}

fn normalize_border_style(value: &str) -> &str {
    match value.trim() {
        "box" => "box",
        _ => "outline",
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
