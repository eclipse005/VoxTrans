use std::collections::HashSet;

use crate::commands::translate_types::TranslateTerminologyEntryCommand;
use crate::db::store::TaskStore;
use crate::services::preferences_types::TerminologyGroup;

/// Task-frozen settings: captured at enqueue time and never read live from
/// the saved settings row during execution. These all affect "what the
/// task means" rather than "how it talks to the LLM" -- changing them
/// mid-run would make the same task produce inconsistent output across
/// chunks/restarts. Everything else (LLM endpoint/model, concurrency,
/// chunk length, ASR/align model) is resolved live from saved settings
/// each call, so the user can swap providers without re-enqueuing.
#[derive(Debug, Clone, Default)]
pub struct FrozenSettings {
    pub subtitle_length_preset: String,
    pub enable_subtitle_beautify: bool,
    pub terminology_groups: Vec<TerminologyGroup>,
}

impl FrozenSettings {
    /// Snapshot the user-frozen settings, keeping ONLY the per-task selected
    /// terminology group. `selected_group_id` == "" means no terminology
    /// (empty frozen groups); otherwise only the matching group (by id) is
    /// frozen, so each task translates with exactly one group's terms.
    pub fn from_saved(
        saved: &crate::services::preferences::SavedSettings,
        selected_group_id: &str,
    ) -> Self {
        let selected = selected_group_id.trim();
        let terminology_groups = if selected.is_empty() {
            Vec::new()
        } else {
            saved
                .terminology_groups
                .iter()
                .find(|group| group.id == selected)
                .cloned()
                .map(|group| vec![group])
                .unwrap_or_default()
        };
        Self {
            subtitle_length_preset: saved.subtitle_length_preset.clone(),
            enable_subtitle_beautify: saved.enable_subtitle_beautify,
            terminology_groups,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PipelineRuntimeSettings {
    pub asr_model: String,
    pub align_model: String,
    pub provider: String,
    pub chunk_target_seconds: u32,
    pub enable_vocal_separation: bool,
    pub demucs_model: String,
    pub translate_api_key: String,
    pub translate_base_url: String,
    pub translate_model: String,
    pub llm_concurrency: u32,
    pub subtitle_length_preset: String,
    pub terminology_entries: Vec<TranslateTerminologyEntryCommand>,
    pub enable_subtitle_beautify: bool,
}

pub fn resolve_runtime_settings(
    store: &TaskStore,
    frozen: &FrozenSettings,
    require_translate_llm: bool,
) -> Result<PipelineRuntimeSettings, String> {
    // Live settings: read fresh on every call so the user can swap
    // LLM endpoint/model, concurrency, ASR/align model, or chunk length
    // and have the next call/chunk pick it up.
    let saved = crate::services::preferences::load_saved_settings_from_default_path(store)
        .unwrap_or_else(|_| fallback_saved_settings());

    let provider = saved.provider.clone();
    let chunk_target_seconds = saved.chunk_target_seconds.clamp(30, 60);
    let asr_model = saved.asr_model.clone();
    let align_model = saved.align_model.clone();
    let enable_vocal_separation = saved.enable_vocal_separation;
    let demucs_model = saved.demucs_model.clone();
    let translate_api_key = saved.translate_api_key.clone();
    let translate_base_url = saved.translate_base_url.clone();
    let translate_model = saved.translate_model.clone();
    let llm_concurrency = saved.llm_concurrency.clamp(1, 16);

    if require_translate_llm && translate_api_key.trim().is_empty() {
        return Err("translateApiKey is required for step_03~step_05".to_string());
    }
    if require_translate_llm && translate_base_url.trim().is_empty() {
        return Err("translateBaseUrl is required for step_03~step_05".to_string());
    }
    if require_translate_llm && translate_model.trim().is_empty() {
        return Err("translateModel is required for step_03~step_05".to_string());
    }

    // Frozen settings: captured at enqueue time, do not change mid-task.
    let subtitle_length_preset =
        crate::services::subtitle_length::normalize_subtitle_length_preset(
            &frozen.subtitle_length_preset,
        );
    let enable_subtitle_beautify = frozen.enable_subtitle_beautify;

    // Terminology is driven by the per-task frozen selection: terminology_groups
    // holds 0 or 1 group (0 == "none"/no terminology). Flatten whatever is there.
    let mut seen = HashSet::<(String, String)>::new();
    let mut terminology_entries = Vec::<TranslateTerminologyEntryCommand>::new();
    for group in &frozen.terminology_groups {
        for term in &group.terms {
            let source = term.origin.trim().to_string();
            let target = term.target.trim().to_string();
            if source.is_empty() || target.is_empty() {
                continue;
            }
            let key = (source.to_ascii_lowercase(), target.to_ascii_lowercase());
            if !seen.insert(key) {
                continue;
            }
            terminology_entries.push(TranslateTerminologyEntryCommand {
                source,
                target,
                note: term.note.trim().to_string(),
            });
        }
    }

    Ok(PipelineRuntimeSettings {
        asr_model,
        align_model,
        provider,
        chunk_target_seconds,
        enable_vocal_separation,
        demucs_model,
        translate_api_key,
        translate_base_url,
        translate_model,
        llm_concurrency,
        subtitle_length_preset,
        terminology_entries,
        enable_subtitle_beautify,
    })
}

pub fn fallback_saved_settings() -> crate::services::preferences::SavedSettings {
    crate::services::preferences::SavedSettings {
        provider: "cpu".to_string(),
        chunk_target_seconds: 45,
        subtitle_length_preset: crate::services::subtitle_length::DEFAULT_SUBTITLE_LENGTH_PRESET
            .to_string(),
        asr_model: crate::services::model::DEFAULT_ASR_MODEL.to_string(),
        align_model: "Qwen3-ForcedAligner-0.6B".to_string(),
        demucs_model: "htdemucs_ft".to_string(),
        enable_vocal_separation: false,
        translate_api_key: String::new(),
        translate_base_url: "https://api.openai.com/v1".to_string(),
        translate_model: "gpt-4.1-mini".to_string(),
        llm_concurrency: 4,
        terminology_groups: Vec::new(),
        active_terminology_group_id: String::new(),
        enable_subtitle_beautify: true,
        enable_click_sound: true,
        auto_burn_hard_subtitle: false,
        subtitle_burn_mode: "bilingualSourceFirst".to_string(),
        subtitle_render_style: crate::services::preferences::SubtitleRenderStyle::default(),
        flat_srt_output: false,
        flat_srt_items: vec!["source".to_string(), "target".to_string()],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::preferences_types::TerminologyTerm;

    fn dummy_store() -> TaskStore {
        let pool = tauri::async_runtime::block_on(crate::db::store::test_pool());
        TaskStore::new(pool)
    }

    #[test]
    fn frozen_subtitle_preset_is_preserved_across_calls() {
        let frozen = FrozenSettings {
            subtitle_length_preset: "loose".to_string(),
            enable_subtitle_beautify: false,
            terminology_groups: Vec::new(),
        };
        let settings = resolve_runtime_settings(&dummy_store(), &frozen, false)
            .expect("resolve");
        assert_eq!(settings.subtitle_length_preset, "loose");
        assert!(!settings.enable_subtitle_beautify);
        assert!(settings.terminology_entries.is_empty());
    }

    #[test]
    fn frozen_terminology_entries_are_normalized_and_deduplicated() {
        let frozen = FrozenSettings {
            subtitle_length_preset: "default".to_string(),
            enable_subtitle_beautify: true,
            terminology_groups: vec![TerminologyGroup {
                id: "g1".to_string(),
                name: "Default".to_string(),
                terms: vec![
                    TerminologyTerm {
                        id: "t1".to_string(),
                        origin: "  NATO ".to_string(),
                        target: "北约".to_string(),
                        note: "a".to_string(),
                    },
                    TerminologyTerm {
                        id: "t2".to_string(),
                        origin: "nato".to_string(),
                        target: " 北约 ".to_string(),
                        note: "dup".to_string(),
                    },
                ],
            }],
        };
        let settings = resolve_runtime_settings(&dummy_store(), &frozen, false)
            .expect("resolve");
        assert_eq!(settings.terminology_entries.len(), 1);
        assert_eq!(settings.terminology_entries[0].source, "NATO");
        assert_eq!(settings.terminology_entries[0].target, "北约");
    }
}
