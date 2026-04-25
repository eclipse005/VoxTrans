use crate::services::preferences::{self};
use tauri::AppHandle;

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TerminologyTermCommand {
    pub id: String,
    pub origin: String,
    pub target: String,
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TerminologyGroupCommand {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub terms: Vec<TerminologyTermCommand>,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct HotwordTermCommand {
    pub id: String,
    pub word: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub lang: String,
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct HotwordGroupCommand {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub terms: Vec<HotwordTermCommand>,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleLineStyleCommand {
    pub font_family: String,
    pub font_size: u32,
    pub primary_color: String,
    pub outline_color: String,
    pub back_color: String,
    pub outline: f64,
    pub shadow: f64,
    pub border_style: String,
    pub border_opacity: u8,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleLayoutStyleCommand {
    pub margin_v: u32,
    pub alignment: u8,
    pub bilingual_line_gap: u32,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleRenderStyleCommand {
    pub source: SubtitleLineStyleCommand,
    pub target: SubtitleLineStyleCommand,
    pub layout: SubtitleLayoutStyleCommand,
}

impl Default for SubtitleRenderStyleCommand {
    fn default() -> Self {
        Self {
            source: SubtitleLineStyleCommand {
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
            target: SubtitleLineStyleCommand {
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
            layout: SubtitleLayoutStyleCommand {
                margin_v: 40,
                alignment: 2,
                bilingual_line_gap: 10,
            },
        }
    }
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedSettingsCommand {
    pub provider: String,
    pub chunk_target_seconds: u32,
    pub subtitle_max_words_per_segment: u32,
    pub subtitle_length_reference: u32,
    pub asr_model: String,
    pub demucs_model: String,
    pub enable_vocal_separation: bool,
    pub translate_api_key: String,
    pub translate_base_url: String,
    pub translate_model: String,
    pub llm_concurrency: u32,
    #[serde(default)]
    pub terminology_groups: Vec<TerminologyGroupCommand>,
    #[serde(default = "default_true")]
    pub enable_terminology: bool,
    #[serde(default)]
    pub hotword_groups: Vec<HotwordGroupCommand>,
    #[serde(default = "default_true")]
    pub enable_hotwords: bool,
    #[serde(default = "default_true")]
    pub enable_subtitle_beautify: bool,
    #[serde(default)]
    pub auto_burn_hard_subtitle: bool,
    #[serde(default = "default_subtitle_burn_mode")]
    pub subtitle_burn_mode: String,
    #[serde(default)]
    pub subtitle_render_style: SubtitleRenderStyleCommand,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserPreferencesCommandResponse {
    pub settings: SavedSettingsCommand,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveAppSettingsCommandRequest {
    pub settings: SavedSettingsCommand,
}

#[tauri::command]
pub async fn load_user_preferences(
    app: AppHandle,
) -> Result<UserPreferencesCommandResponse, String> {
    let response = preferences::load_user_preferences(&app).await?;
    Ok(UserPreferencesCommandResponse {
        settings: from_service_settings(response.settings),
    })
}

#[tauri::command]
pub async fn save_app_settings(
    app: AppHandle,
    request: SaveAppSettingsCommandRequest,
) -> Result<(), String> {
    preferences::save_app_settings(
        &app,
        &crate::services::preferences::SaveAppSettingsRequest {
            settings: to_service_settings(request.settings),
        },
    )
    .await
}

fn to_service_settings(
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
        hotword_groups: settings
            .hotword_groups
            .into_iter()
            .map(|group| crate::services::preferences::HotwordGroup {
                id: group.id,
                name: group.name,
                terms: group
                    .terms
                    .into_iter()
                    .map(|term| crate::services::preferences::HotwordTerm {
                        id: term.id,
                        word: term.word,
                        aliases: term.aliases,
                        lang: term.lang,
                        note: term.note,
                    })
                    .collect(),
            })
            .collect(),
        enable_hotwords: settings.enable_hotwords,
        enable_subtitle_beautify: settings.enable_subtitle_beautify,
        auto_burn_hard_subtitle: settings.auto_burn_hard_subtitle,
        subtitle_burn_mode: settings.subtitle_burn_mode,
        subtitle_render_style: crate::services::preferences::SubtitleRenderStyle {
            source: crate::services::preferences::SubtitleLineStyle {
                font_family: settings.subtitle_render_style.source.font_family,
                font_size: settings.subtitle_render_style.source.font_size,
                primary_color: settings.subtitle_render_style.source.primary_color,
                outline_color: settings.subtitle_render_style.source.outline_color,
                back_color: settings.subtitle_render_style.source.back_color,
                outline: settings.subtitle_render_style.source.outline,
                shadow: settings.subtitle_render_style.source.shadow,
                border_style: settings.subtitle_render_style.source.border_style,
                border_opacity: settings.subtitle_render_style.source.border_opacity,
            },
            target: crate::services::preferences::SubtitleLineStyle {
                font_family: settings.subtitle_render_style.target.font_family,
                font_size: settings.subtitle_render_style.target.font_size,
                primary_color: settings.subtitle_render_style.target.primary_color,
                outline_color: settings.subtitle_render_style.target.outline_color,
                back_color: settings.subtitle_render_style.target.back_color,
                outline: settings.subtitle_render_style.target.outline,
                shadow: settings.subtitle_render_style.target.shadow,
                border_style: settings.subtitle_render_style.target.border_style,
                border_opacity: settings.subtitle_render_style.target.border_opacity,
            },
            layout: crate::services::preferences::SubtitleLayoutStyle {
                margin_v: settings.subtitle_render_style.layout.margin_v,
                alignment: settings.subtitle_render_style.layout.alignment,
                bilingual_line_gap: settings.subtitle_render_style.layout.bilingual_line_gap,
            },
        },
    }
}

fn from_service_settings(
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
        hotword_groups: settings
            .hotword_groups
            .into_iter()
            .map(|group| HotwordGroupCommand {
                id: group.id,
                name: group.name,
                terms: group
                    .terms
                    .into_iter()
                    .map(|term| HotwordTermCommand {
                        id: term.id,
                        word: term.word,
                        aliases: term.aliases,
                        lang: term.lang,
                        note: term.note,
                    })
                    .collect(),
            })
            .collect(),
        enable_hotwords: settings.enable_hotwords,
        enable_subtitle_beautify: settings.enable_subtitle_beautify,
        auto_burn_hard_subtitle: settings.auto_burn_hard_subtitle,
        subtitle_burn_mode: settings.subtitle_burn_mode,
        subtitle_render_style: SubtitleRenderStyleCommand {
            source: SubtitleLineStyleCommand {
                font_family: settings.subtitle_render_style.source.font_family,
                font_size: settings.subtitle_render_style.source.font_size,
                primary_color: settings.subtitle_render_style.source.primary_color,
                outline_color: settings.subtitle_render_style.source.outline_color,
                back_color: settings.subtitle_render_style.source.back_color,
                outline: settings.subtitle_render_style.source.outline,
                shadow: settings.subtitle_render_style.source.shadow,
                border_style: settings.subtitle_render_style.source.border_style,
                border_opacity: settings.subtitle_render_style.source.border_opacity,
            },
            target: SubtitleLineStyleCommand {
                font_family: settings.subtitle_render_style.target.font_family,
                font_size: settings.subtitle_render_style.target.font_size,
                primary_color: settings.subtitle_render_style.target.primary_color,
                outline_color: settings.subtitle_render_style.target.outline_color,
                back_color: settings.subtitle_render_style.target.back_color,
                outline: settings.subtitle_render_style.target.outline,
                shadow: settings.subtitle_render_style.target.shadow,
                border_style: settings.subtitle_render_style.target.border_style,
                border_opacity: settings.subtitle_render_style.target.border_opacity,
            },
            layout: SubtitleLayoutStyleCommand {
                margin_v: settings.subtitle_render_style.layout.margin_v,
                alignment: settings.subtitle_render_style.layout.alignment,
                bilingual_line_gap: settings.subtitle_render_style.layout.bilingual_line_gap,
            },
        },
    }
}

fn default_true() -> bool {
    true
}

fn default_subtitle_burn_mode() -> String {
    "bilingualSourceFirst".to_string()
}
