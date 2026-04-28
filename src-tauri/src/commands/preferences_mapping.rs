use super::preferences_types::{
    SavedSettingsCommand, SubtitleLayoutStyleCommand, SubtitleLineStyleCommand,
    SubtitleRenderStyleCommand, TerminologyGroupCommand, TerminologyTermCommand,
};

pub(super) fn to_service_settings(
    settings: SavedSettingsCommand,
) -> crate::services::preferences::SavedSettings {
    crate::services::preferences::SavedSettings {
        provider: settings.provider,
        chunk_target_seconds: settings.chunk_target_seconds,
        subtitle_max_words_per_segment: settings.subtitle_max_words_per_segment,
        subtitle_length_reference: settings.subtitle_length_reference,
        asr_model: settings.asr_model,
        demucs_model: settings.demucs_model,
        enable_vocal_separation: settings.enable_vocal_separation,
        translate_api_key: settings.translate_api_key,
        translate_base_url: settings.translate_base_url,
        translate_model: settings.translate_model,
        llm_concurrency: settings.llm_concurrency,
        terminology_groups: settings
            .terminology_groups
            .into_iter()
            .map(|group| crate::services::preferences::TerminologyGroup {
                id: group.id,
                name: group.name,
                terms: group
                    .terms
                    .into_iter()
                    .map(|term| crate::services::preferences::TerminologyTerm {
                        id: term.id,
                        origin: term.origin,
                        target: term.target,
                        note: term.note,
                    })
                    .collect(),
            })
            .collect(),
        enable_terminology: settings.enable_terminology,
        enable_subtitle_beautify: settings.enable_subtitle_beautify,
        auto_burn_hard_subtitle: settings.auto_burn_hard_subtitle,
        subtitle_burn_mode: settings.subtitle_burn_mode,
        subtitle_render_style: to_service_render_style(settings.subtitle_render_style),
    }
}

pub(super) fn from_service_settings(
    settings: crate::services::preferences::SavedSettings,
) -> SavedSettingsCommand {
    SavedSettingsCommand {
        provider: settings.provider,
        chunk_target_seconds: settings.chunk_target_seconds,
        subtitle_max_words_per_segment: settings.subtitle_max_words_per_segment,
        subtitle_length_reference: settings.subtitle_length_reference,
        asr_model: settings.asr_model,
        demucs_model: settings.demucs_model,
        enable_vocal_separation: settings.enable_vocal_separation,
        translate_api_key: settings.translate_api_key,
        translate_base_url: settings.translate_base_url,
        translate_model: settings.translate_model,
        llm_concurrency: settings.llm_concurrency,
        terminology_groups: settings
            .terminology_groups
            .into_iter()
            .map(|group| TerminologyGroupCommand {
                id: group.id,
                name: group.name,
                terms: group
                    .terms
                    .into_iter()
                    .map(|term| TerminologyTermCommand {
                        id: term.id,
                        origin: term.origin,
                        target: term.target,
                        note: term.note,
                    })
                    .collect(),
            })
            .collect(),
        enable_terminology: settings.enable_terminology,
        enable_subtitle_beautify: settings.enable_subtitle_beautify,
        auto_burn_hard_subtitle: settings.auto_burn_hard_subtitle,
        subtitle_burn_mode: settings.subtitle_burn_mode,
        subtitle_render_style: from_service_render_style(settings.subtitle_render_style),
    }
}

fn to_service_render_style(
    style: SubtitleRenderStyleCommand,
) -> crate::services::preferences::SubtitleRenderStyle {
    crate::services::preferences::SubtitleRenderStyle {
        source: to_service_line_style(style.source),
        target: to_service_line_style(style.target),
        layout: to_service_layout_style(style.layout),
    }
}

fn to_service_line_style(
    style: SubtitleLineStyleCommand,
) -> crate::services::preferences::SubtitleLineStyle {
    crate::services::preferences::SubtitleLineStyle {
        font_family: style.font_family,
        font_size: style.font_size,
        primary_color: style.primary_color,
        outline_color: style.outline_color,
        back_color: style.back_color,
        outline: style.outline,
        shadow: style.shadow,
        border_style: style.border_style,
        border_opacity: style.border_opacity,
    }
}

fn to_service_layout_style(
    style: SubtitleLayoutStyleCommand,
) -> crate::services::preferences::SubtitleLayoutStyle {
    crate::services::preferences::SubtitleLayoutStyle {
        margin_v: style.margin_v,
        alignment: style.alignment,
        bilingual_line_gap: style.bilingual_line_gap,
    }
}

fn from_service_render_style(
    style: crate::services::preferences::SubtitleRenderStyle,
) -> SubtitleRenderStyleCommand {
    SubtitleRenderStyleCommand {
        source: from_service_line_style(style.source),
        target: from_service_line_style(style.target),
        layout: from_service_layout_style(style.layout),
    }
}

fn from_service_line_style(
    style: crate::services::preferences::SubtitleLineStyle,
) -> SubtitleLineStyleCommand {
    SubtitleLineStyleCommand {
        font_family: style.font_family,
        font_size: style.font_size,
        primary_color: style.primary_color,
        outline_color: style.outline_color,
        back_color: style.back_color,
        outline: style.outline,
        shadow: style.shadow,
        border_style: style.border_style,
        border_opacity: style.border_opacity,
    }
}

fn from_service_layout_style(
    style: crate::services::preferences::SubtitleLayoutStyle,
) -> SubtitleLayoutStyleCommand {
    SubtitleLayoutStyleCommand {
        margin_v: style.margin_v,
        alignment: style.alignment,
        bilingual_line_gap: style.bilingual_line_gap,
    }
}
