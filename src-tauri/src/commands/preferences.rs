use tauri::State;

use crate::app_state::AppState;
use crate::services::preferences::{self};

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
    pub enable_punctuation_optimization: bool,
    #[serde(default = "default_true")]
    pub enable_subtitle_beautify: bool,
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
    state: State<'_, AppState>,
) -> Result<UserPreferencesCommandResponse, String> {
    let response = preferences::load_user_preferences(&state.pool).await?;
    Ok(UserPreferencesCommandResponse {
        settings: from_service_settings(response.settings),
    })
}

#[tauri::command]
pub async fn save_app_settings(
    state: State<'_, AppState>,
    request: SaveAppSettingsCommandRequest,
) -> Result<(), String> {
    preferences::save_app_settings(
        &state.pool,
        &crate::services::preferences::SaveAppSettingsRequest {
            settings: to_service_settings(request.settings),
        },
    )
    .await
}

fn to_service_settings(settings: SavedSettingsCommand) -> crate::services::preferences::SavedSettings {
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
        enable_punctuation_optimization: settings.enable_punctuation_optimization,
        enable_subtitle_beautify: settings.enable_subtitle_beautify,
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
        enable_punctuation_optimization: settings.enable_punctuation_optimization,
        enable_subtitle_beautify: settings.enable_subtitle_beautify,
    }
}

fn default_true() -> bool {
    true
}
