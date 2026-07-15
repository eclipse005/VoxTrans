use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};

use super::preferences_types::{
    AlignModel, AsrModel, DemucsModel, LlmProfile, Locale, Provider, SavedSettings,
    SubtitleBurnMode, SubtitleLayoutStyle, SubtitleLineStyle, SubtitleLengthPreset,
    SubtitleRenderStyle, TerminologyGroup, TerminologyTerm,
};

const DEFAULT_ACTIVE_LLM_PROFILE_ID: &str = "deepseek";
const DEFAULT_TRANSLATE_BASE_URL: &str = "https://api.deepseek.com/v1";
/// Keep in lockstep with EggTranslate `llmProviders.ts` (and FE `llmProviders.ts`).
const DEFAULT_TRANSLATE_MODEL: &str = "deepseek-v4-flash";

/// Built-in provider slots. Keep in sync with frontend `llmProviders.ts` /
/// EggTranslate defaults (minus Agnes/Zhipu).
/// Adding a vendor: append here + frontend preset table + optional icon.
pub fn default_llm_profiles() -> Vec<LlmProfile> {
    vec![
        profile("custom", "自定义", "", "", true),
        profile(
            "deepseek",
            "DeepSeek",
            DEFAULT_TRANSLATE_BASE_URL,
            DEFAULT_TRANSLATE_MODEL,
            true,
        ),
        profile(
            "qwen",
            "通义千问",
            "https://dashscope.aliyuncs.com/compatible-mode/v1",
            "qwen3.6-flash",
            true,
        ),
        profile(
            "doubao",
            "豆包",
            "https://ark.cn-beijing.volces.com/api/v3",
            "doubao-seed-2-1-turbo-260628",
            true,
        ),
        profile(
            "chatgpt",
            "OpenAI",
            "https://api.openai.com/v1",
            "gpt-5-mini",
            true,
        ),
        profile(
            "gemini",
            "Google Gemini",
            "https://generativelanguage.googleapis.com/v1beta/openai",
            "gemini-3.5-flash",
            true,
        ),
        profile(
            "openrouter",
            "OpenRouter",
            "https://openrouter.ai/api/v1",
            "google/gemini-3.5-flash",
            true,
        ),
        profile(
            "ollama",
            "Ollama",
            "http://localhost:11434/v1",
            "qwen3:8b",
            false,
        ),
    ]
}

fn profile(id: &str, name: &str, base_url: &str, model: &str, requires_key: bool) -> LlmProfile {
    LlmProfile {
        id: id.to_string(),
        name: name.to_string(),
        base_url: base_url.to_string(),
        api_key: String::new(),
        model: model.to_string(),
        preset_id: id.to_string(),
        requires_key,
    }
}

pub fn default_settings() -> SavedSettings {
    let profiles = default_llm_profiles();
    let active = profiles
        .iter()
        .find(|p| p.id == DEFAULT_ACTIVE_LLM_PROFILE_ID)
        .cloned()
        .unwrap_or_else(|| profiles[0].clone());
    SavedSettings {
        provider: Provider::Cpu,
        chunk_target_seconds: 30,
        subtitle_length_preset: SubtitleLengthPreset::Standard,
        asr_model: AsrModel::default(),
        align_model: AlignModel::default(),
        demucs_model: DemucsModel::default(),
        enable_vocal_separation: false,
        translate_api_key: effective_api_key(&active),
        translate_base_url: active.base_url.clone(),
        translate_model: active.model.clone(),
        llm_profiles: profiles,
        active_llm_profile_id: DEFAULT_ACTIVE_LLM_PROFILE_ID.to_string(),
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
        locale: Locale::default(),
        models_dir: None,
    }
}

pub fn normalize_saved_settings(settings: SavedSettings) -> SavedSettings {
    let mut settings = settings;
    let (profiles, active_id) = ensure_llm_profiles(
        settings.llm_profiles,
        settings.active_llm_profile_id,
        &settings.translate_api_key,
        &settings.translate_base_url,
        &settings.translate_model,
    );
    settings.llm_profiles = profiles;
    settings.active_llm_profile_id = active_id;
    sync_translate_fields_from_active(&mut settings);

    SavedSettings {
        provider: settings.provider,
        chunk_target_seconds: settings.chunk_target_seconds.clamp(30, 60),
        subtitle_length_preset: settings.subtitle_length_preset,
        asr_model: settings.asr_model,
        align_model: settings.align_model,
        demucs_model: settings.demucs_model,
        enable_vocal_separation: settings.enable_vocal_separation,
        // Already mirrored from active profile in sync_translate_fields_from_active —
        // do not invent DEFAULT_TRANSLATE_* here (would cross-mix vendors when empty).
        translate_api_key: settings.translate_api_key.trim().to_string(),
        translate_base_url: settings.translate_base_url.trim().to_string(),
        translate_model: settings.translate_model.trim().to_string(),
        llm_profiles: settings.llm_profiles,
        active_llm_profile_id: settings.active_llm_profile_id,
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
        locale: settings.locale,
        models_dir: settings.models_dir.map(|d| d.trim().to_string()).filter(|d| !d.is_empty()),
    }
}

fn effective_api_key(profile: &LlmProfile) -> String {
    let key = profile.api_key.trim();
    if key.is_empty() && !profile.requires_key {
        "ollama".to_string()
    } else {
        key.to_string()
    }
}

/// Models we previously shipped as catalog defaults before aligning with Egg.
fn is_obsolete_catalog_model(preset_id: &str, model: &str) -> bool {
    match preset_id {
        "deepseek" => model == "deepseek-chat",
        "qwen" => model == "qwen-flash",
        "doubao" => model == "doubao-seed-1-6-flash-250828",
        "chatgpt" => model == "gpt-4.1-mini",
        "gemini" => model == "gemini-2.5-flash",
        "openrouter" => model == "google/gemini-2.5-flash",
        "ollama" => model == "qwen2.5:7b",
        _ => false,
    }
}

fn ensure_llm_profiles(
    mut profiles: Vec<LlmProfile>,
    active_id: String,
    legacy_key: &str,
    legacy_base_url: &str,
    legacy_model: &str,
) -> (Vec<LlmProfile>, String) {
    let catalog = default_llm_profiles();
    let was_empty = profiles.is_empty();
    let mut seeded_active: Option<String> = None;

    if was_empty {
        profiles = catalog.clone();
        // Migrate pre-multi-profile settings into the matching slot.
        seeded_active =
            seed_legacy_into_profiles(&mut profiles, legacy_key, legacy_base_url, legacy_model);
        // One-shot: if legacy model was an old catalog default, bump to current preset.
        if let Some(ref target) = seeded_active {
            if let Some(p) = profiles.iter_mut().find(|p| p.id == *target) {
                if is_obsolete_catalog_model(&p.id, &p.model) {
                    if let Some(catalog_p) = catalog.iter().find(|c| c.id == p.id) {
                        p.model = catalog_p.model.clone();
                    }
                }
            }
        }
    } else {
        // Fill missing vendor slots when catalog grows (add-provider is data-only).
        for preset in &catalog {
            if !profiles.iter().any(|p| p.id == preset.id) {
                profiles.push(preset.clone());
            }
        }
    }

    // Shared post-process for empty and non-empty paths (trim / requiresKey / empty model).
    // Free-form contract: never overwrite a non-empty user model with catalog defaults.
    for p in &mut profiles {
        p.id = p.id.trim().to_string();
        p.name = p.name.trim().to_string();
        p.base_url = p.base_url.trim().to_string();
        p.api_key = p.api_key.trim().to_string();
        p.model = p.model.trim().to_string();
        if p.preset_id.trim().is_empty() {
            p.preset_id = p.id.clone();
        } else {
            p.preset_id = p.preset_id.trim().to_string();
        }
        if let Some(catalog_p) = catalog.iter().find(|c| c.id == p.id) {
            if p.name.is_empty() {
                p.name = catalog_p.name.clone();
            }
            // Known presets: requires_key is authoritative (e.g. ollama is keyless).
            if p.id != "custom" {
                p.requires_key = catalog_p.requires_key;
            }
            // Only fill empty model — user free-form choices are preserved.
            if p.model.is_empty() {
                p.model = catalog_p.model.clone();
            }
        }
    }

    let active = active_id.trim().to_string();
    let active_id = if let Some(seeded) = seeded_active.filter(|id| profiles.iter().any(|p| p.id == *id))
    {
        // Legacy migration: land on the slot that received the old key/url.
        seeded
    } else if profiles.iter().any(|p| p.id == active) {
        active
    } else if profiles.iter().any(|p| p.id == DEFAULT_ACTIVE_LLM_PROFILE_ID) {
        DEFAULT_ACTIVE_LLM_PROFILE_ID.to_string()
    } else {
        profiles.first().map(|p| p.id.clone()).unwrap_or_default()
    };

    (profiles, active_id)
}

/// Returns the target profile id when anything was written, else `None`.
fn seed_legacy_into_profiles(
    profiles: &mut [LlmProfile],
    legacy_key: &str,
    legacy_base_url: &str,
    legacy_model: &str,
) -> Option<String> {
    let key = legacy_key.trim();
    let base = legacy_base_url.trim();
    let model = legacy_model.trim();
    if key.is_empty() && base.is_empty() && model.is_empty() {
        return None;
    }

    // Prefer normalized base URL match; fall back to deepseek then custom.
    let base_norm = normalize_endpoint_url(base);
    let target_id = profiles
        .iter()
        .find(|p| !base_norm.is_empty() && normalize_endpoint_url(&p.base_url) == base_norm)
        .map(|p| p.id.clone())
        .or_else(|| {
            if base_norm.contains("deepseek") {
                Some("deepseek".to_string())
            } else if !base.is_empty() {
                Some("custom".to_string())
            } else {
                Some(DEFAULT_ACTIVE_LLM_PROFILE_ID.to_string())
            }
        })
        .unwrap_or_else(|| DEFAULT_ACTIVE_LLM_PROFILE_ID.to_string());

    if let Some(p) = profiles.iter_mut().find(|p| p.id == target_id) {
        if !key.is_empty() {
            p.api_key = key.to_string();
        }
        if !base.is_empty() {
            p.base_url = base.trim_end_matches('/').to_string();
        }
        if !model.is_empty() {
            p.model = model.to_string();
        }
        Some(target_id)
    } else {
        None
    }
}

/// Trim, strip trailing `/`, lower-case for catalog matching (no extra deps).
fn normalize_endpoint_url(url: &str) -> String {
    url.trim().trim_end_matches('/').to_ascii_lowercase()
}

fn sync_translate_fields_from_active(settings: &mut SavedSettings) {
    let active = settings
        .llm_profiles
        .iter()
        .find(|p| p.id == settings.active_llm_profile_id)
        .cloned();
    if let Some(p) = active {
        // Always mirror the active slot (including empty) so denormalized
        // fields never keep a previous vendor's URL/model.
        settings.translate_api_key = effective_api_key(&p);
        settings.translate_base_url = p.base_url.trim().to_string();
        settings.translate_model = p.model.trim().to_string();
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
                "Default".to_string()
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
        name: "Default".to_string(),
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

    #[test]
    fn empty_archive_seeds_openai_url_onto_chatgpt_and_activates_it() {
        let mut settings = default_settings();
        settings.llm_profiles = Vec::new();
        settings.active_llm_profile_id = "deepseek".into();
        settings.translate_api_key = "openai-key".into();
        settings.translate_base_url = "https://api.openai.com/v1/".into();
        settings.translate_model = "gpt-5-mini".into();

        let normalized = normalize_saved_settings(settings);
        assert_eq!(normalized.active_llm_profile_id, "chatgpt");
        let slot = normalized
            .llm_profiles
            .iter()
            .find(|p| p.id == "chatgpt")
            .expect("chatgpt slot");
        assert_eq!(slot.api_key, "openai-key");
        assert_eq!(normalized.translate_api_key, "openai-key");
        assert!(normalized.translate_base_url.contains("openai.com"));
    }

    #[test]
    fn empty_archive_unknown_url_seeds_custom_and_activates_it() {
        let mut settings = default_settings();
        settings.llm_profiles = Vec::new();
        settings.active_llm_profile_id = "deepseek".into();
        settings.translate_api_key = "proxy-key".into();
        settings.translate_base_url = "https://my-proxy.example/v1".into();
        settings.translate_model = "my-model".into();

        let normalized = normalize_saved_settings(settings);
        assert_eq!(normalized.active_llm_profile_id, "custom");
        assert_eq!(normalized.translate_api_key, "proxy-key");
        assert_eq!(normalized.translate_base_url, "https://my-proxy.example/v1");
        assert_eq!(normalized.translate_model, "my-model");
    }

    #[test]
    fn free_form_model_is_not_overwritten_on_non_empty_profiles() {
        let mut settings = default_settings();
        if let Some(p) = settings.llm_profiles.iter_mut().find(|p| p.id == "deepseek") {
            p.model = "deepseek-chat".into();
            p.api_key = "k".into();
        }
        settings.active_llm_profile_id = "deepseek".into();

        let normalized = normalize_saved_settings(settings);
        let slot = normalized
            .llm_profiles
            .iter()
            .find(|p| p.id == "deepseek")
            .unwrap();
        assert_eq!(slot.model, "deepseek-chat");
        assert_eq!(normalized.translate_model, "deepseek-chat");
    }

    #[test]
    fn sync_does_not_keep_previous_vendor_url_when_active_base_is_empty() {
        let mut settings = default_settings();
        settings.translate_base_url = "https://api.deepseek.com/v1".into();
        settings.translate_model = "deepseek-v4-flash".into();
        settings.translate_api_key = "stale".into();
        if let Some(p) = settings.llm_profiles.iter_mut().find(|p| p.id == "custom") {
            p.api_key = "new-key".into();
            p.base_url = String::new();
            p.model = "m".into();
        }
        settings.active_llm_profile_id = "custom".into();

        let normalized = normalize_saved_settings(settings);
        assert_eq!(normalized.translate_api_key, "new-key");
        assert_eq!(normalized.translate_base_url, "");
        assert_eq!(normalized.translate_model, "m");
    }
}
