use std::collections::HashSet;

use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Clone)]
pub(super) struct PipelineRuntimeSettings {
    pub(super) provider: String,
    pub(super) chunk_target_seconds: u32,
    pub(super) translate_api_key: String,
    pub(super) translate_base_url: String,
    pub(super) translate_model: String,
    pub(super) llm_concurrency: u32,
    pub(super) subtitle_max_words_per_segment: u32,
    pub(super) subtitle_length_reference: u32,
    pub(super) terminology_entries:
        Vec<crate::commands::translate::TranslateTerminologyEntryCommand>,
    pub(super) enable_subtitle_beautify: bool,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct SettingsSnapshotInput {
    #[serde(default)]
    provider: Option<String>,
    #[serde(default)]
    chunk_target_seconds: Option<u32>,
    #[serde(default)]
    translate_api_key: Option<String>,
    #[serde(default)]
    translate_base_url: Option<String>,
    #[serde(default)]
    translate_model: Option<String>,
    #[serde(default)]
    llm_concurrency: Option<u32>,
    #[serde(default)]
    subtitle_max_words_per_segment: Option<u32>,
    #[serde(default)]
    subtitle_length_reference: Option<u32>,
    #[serde(default)]
    terminology_groups: Option<Vec<SettingsSnapshotTerminologyGroup>>,
    #[serde(default)]
    enable_terminology: Option<bool>,
    #[serde(default)]
    enable_subtitle_beautify: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct SettingsSnapshotTerminologyGroup {
    #[serde(default)]
    terms: Vec<SettingsSnapshotTerminologyTerm>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct SettingsSnapshotTerminologyTerm {
    #[serde(default)]
    origin: String,
    #[serde(default)]
    target: String,
    #[serde(default)]
    note: String,
}

pub(super) fn resolve_runtime_settings(
    snapshot: &Value,
    require_translate_llm: bool,
) -> Result<PipelineRuntimeSettings, String> {
    let snapshot_parsed =
        serde_json::from_value::<SettingsSnapshotInput>(snapshot.clone()).unwrap_or_default();
    let saved = crate::services::preferences::load_saved_settings_from_default_path()
        .unwrap_or_else(|_| fallback_saved_settings());

    let provider = snapshot_parsed
        .provider
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| saved.provider.clone());
    let chunk_target_seconds = snapshot_parsed
        .chunk_target_seconds
        .unwrap_or(saved.chunk_target_seconds)
        .clamp(30, 300);

    let translate_api_key = snapshot_parsed
        .translate_api_key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| saved.translate_api_key.clone());
    let translate_base_url = snapshot_parsed
        .translate_base_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| saved.translate_base_url.clone());
    let translate_model = snapshot_parsed
        .translate_model
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| saved.translate_model.clone());

    if require_translate_llm && translate_api_key.trim().is_empty() {
        return Err("translateApiKey is required for step_03~step_05".to_string());
    }
    if require_translate_llm && translate_base_url.trim().is_empty() {
        return Err("translateBaseUrl is required for step_03~step_05".to_string());
    }
    if require_translate_llm && translate_model.trim().is_empty() {
        return Err("translateModel is required for step_03~step_05".to_string());
    }

    let llm_concurrency = snapshot_parsed
        .llm_concurrency
        .unwrap_or(saved.llm_concurrency)
        .clamp(1, 16);
    let subtitle_max_words_per_segment = snapshot_parsed
        .subtitle_max_words_per_segment
        .unwrap_or(saved.subtitle_max_words_per_segment)
        .clamp(8, 40);
    let subtitle_length_reference = snapshot_parsed
        .subtitle_length_reference
        .unwrap_or(saved.subtitle_length_reference)
        .clamp(8, 80);
    let enable_terminology = snapshot_parsed
        .enable_terminology
        .unwrap_or(saved.enable_terminology);
    let enable_subtitle_beautify = snapshot_parsed
        .enable_subtitle_beautify
        .unwrap_or(saved.enable_subtitle_beautify);

    let terminology_entries = if enable_terminology {
        let mut seen = HashSet::<(String, String)>::new();
        let mut out = Vec::<crate::commands::translate::TranslateTerminologyEntryCommand>::new();
        let snapshot_entries = snapshot_parsed
            .terminology_groups
            .unwrap_or_default()
            .into_iter()
            .flat_map(|group| group.terms.into_iter())
            .map(
                |term| crate::commands::translate::TranslateTerminologyEntryCommand {
                    source: term.origin.trim().to_string(),
                    target: term.target.trim().to_string(),
                    note: term.note.trim().to_string(),
                },
            )
            .collect::<Vec<_>>();

        for entry in snapshot_entries
            .into_iter()
            .chain(saved_terminology_entries(&saved).into_iter())
        {
            let source = entry.source.trim().to_string();
            let target = entry.target.trim().to_string();
            if source.is_empty() || target.is_empty() {
                continue;
            }
            let key = (source.to_ascii_lowercase(), target.to_ascii_lowercase());
            if !seen.insert(key) {
                continue;
            }
            out.push(
                crate::commands::translate::TranslateTerminologyEntryCommand {
                    source,
                    target,
                    note: entry.note.trim().to_string(),
                },
            );
        }
        out
    } else {
        Vec::new()
    };

    Ok(PipelineRuntimeSettings {
        provider,
        chunk_target_seconds,
        translate_api_key,
        translate_base_url,
        translate_model,
        llm_concurrency,
        subtitle_max_words_per_segment,
        subtitle_length_reference,
        terminology_entries,
        enable_subtitle_beautify,
    })
}

pub(super) fn fallback_saved_settings() -> crate::services::preferences::SavedSettings {
    crate::services::preferences::SavedSettings {
        provider: "cpu".to_string(),
        chunk_target_seconds: 180,
        subtitle_max_words_per_segment: 20,
        subtitle_length_reference: 28,
        asr_model: "parakeet-tdt-0.6b-v2".to_string(),
        demucs_model: "htdemucs_ft".to_string(),
        enable_vocal_separation: false,
        translate_api_key: String::new(),
        translate_base_url: "https://api.openai.com/v1".to_string(),
        translate_model: "gpt-4.1-mini".to_string(),
        llm_concurrency: 4,
        terminology_groups: Vec::new(),
        enable_terminology: true,
        enable_subtitle_beautify: true,
        auto_burn_hard_subtitle: false,
        subtitle_burn_mode: "bilingualSourceFirst".to_string(),
        subtitle_render_style: crate::services::preferences::SubtitleRenderStyle::default(),
    }
}

fn saved_terminology_entries(
    saved: &crate::services::preferences::SavedSettings,
) -> Vec<crate::commands::translate::TranslateTerminologyEntryCommand> {
    saved
        .terminology_groups
        .iter()
        .flat_map(|group| group.terms.iter())
        .map(
            |term| crate::commands::translate::TranslateTerminologyEntryCommand {
                source: term.origin.clone(),
                target: term.target.clone(),
                note: term.note.clone(),
            },
        )
        .collect()
}
