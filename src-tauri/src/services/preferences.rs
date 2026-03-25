use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};

const KEY_PROVIDER: &str = "settings.provider";
const KEY_CHUNK_TARGET_SECONDS: &str = "settings.chunkTargetSeconds";
const KEY_SUBTITLE_MAX_WORDS_PER_SEGMENT: &str = "settings.subtitleMaxWordsPerSegment";
const KEY_SUBTITLE_LENGTH_REFERENCE: &str = "settings.subtitleLengthReference";
const KEY_ASR_MODEL: &str = "settings.asrModel";
const KEY_DEMUCS_MODEL: &str = "settings.demucsModel";
const KEY_ENABLE_VOCAL_SEPARATION: &str = "settings.enableVocalSeparation";
const KEY_TRANSLATE_API_KEY: &str = "settings.translateApiKey";
const KEY_TRANSLATE_BASE_URL: &str = "settings.translateBaseUrl";
const KEY_TRANSLATE_MODEL: &str = "settings.translateModel";
const KEY_LLM_CONCURRENCY: &str = "settings.llmConcurrency";
const KEY_TERMINOLOGY_GROUPS: &str = "settings.terminologyGroups";
const KEY_ENABLE_TERMINOLOGY: &str = "settings.enableTerminology";
const KEY_ENABLE_PUNCTUATION_OPTIMIZATION: &str = "settings.enablePunctuationOptimization";
const KEY_ENABLE_SUBTITLE_BEAUTIFY: &str = "settings.enableSubtitleBeautify";
const KEY_AUTO_BURN_HARD_SUBTITLE: &str = "settings.autoBurnHardSubtitle";
const KEY_SUBTITLE_BURN_MODE: &str = "settings.subtitleBurnMode";
const KEY_SUBTITLE_RENDER_STYLE: &str = "settings.subtitleRenderStyle";

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TerminologyTerm {
    pub id: String,
    pub origin: String,
    pub target: String,
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TerminologyGroup {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub terms: Vec<TerminologyTerm>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleLineStyle {
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

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleLayoutStyle {
    pub margin_v: u32,
    pub alignment: u8,
    pub bilingual_line_gap: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleRenderStyle {
    pub source: SubtitleLineStyle,
    pub target: SubtitleLineStyle,
    pub layout: SubtitleLayoutStyle,
}

impl Default for SubtitleRenderStyle {
    fn default() -> Self {
        Self {
            source: SubtitleLineStyle {
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
            target: SubtitleLineStyle {
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
            layout: SubtitleLayoutStyle {
                margin_v: 40,
                alignment: 2,
                bilingual_line_gap: 10,
            },
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedSettings {
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
    pub terminology_groups: Vec<TerminologyGroup>,
    #[serde(default = "default_true")]
    pub enable_terminology: bool,
    pub enable_punctuation_optimization: bool,
    #[serde(default = "default_true")]
    pub enable_subtitle_beautify: bool,
    #[serde(default)]
    pub auto_burn_hard_subtitle: bool,
    #[serde(default = "default_subtitle_burn_mode")]
    pub subtitle_burn_mode: String,
    #[serde(default)]
    pub subtitle_render_style: SubtitleRenderStyle,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserPreferencesResponse {
    pub settings: SavedSettings,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveAppSettingsRequest {
    pub settings: SavedSettings,
}

pub async fn load_user_preferences(pool: &SqlitePool) -> Result<UserPreferencesResponse, String> {
    let settings = load_settings(pool).await?;
    Ok(UserPreferencesResponse { settings })
}

pub async fn save_app_settings(
    pool: &SqlitePool,
    request: &SaveAppSettingsRequest,
) -> Result<(), String> {
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    set_setting(&mut tx, KEY_PROVIDER, &request.settings.provider).await?;
    set_setting(
        &mut tx,
        KEY_CHUNK_TARGET_SECONDS,
        &request
            .settings
            .chunk_target_seconds
            .clamp(30, 300)
            .to_string(),
    )
    .await?;
    set_setting(
        &mut tx,
        KEY_SUBTITLE_MAX_WORDS_PER_SEGMENT,
        &request
            .settings
            .subtitle_max_words_per_segment
            .clamp(8, 40)
            .to_string(),
    )
    .await?;
    set_setting(&mut tx, KEY_ASR_MODEL, &request.settings.asr_model).await?;
    set_setting(&mut tx, KEY_DEMUCS_MODEL, &request.settings.demucs_model).await?;
    set_setting(
        &mut tx,
        KEY_ENABLE_VOCAL_SEPARATION,
        if request.settings.enable_vocal_separation {
            "1"
        } else {
            "0"
        },
    )
    .await?;
    set_setting(
        &mut tx,
        KEY_SUBTITLE_LENGTH_REFERENCE,
        &request
            .settings
            .subtitle_length_reference
            .clamp(8, 80)
            .to_string(),
    )
    .await?;
    set_setting(
        &mut tx,
        KEY_TRANSLATE_API_KEY,
        request.settings.translate_api_key.trim(),
    )
    .await?;
    set_setting(
        &mut tx,
        KEY_TRANSLATE_BASE_URL,
        request.settings.translate_base_url.trim(),
    )
    .await?;
    set_setting(
        &mut tx,
        KEY_TRANSLATE_MODEL,
        request.settings.translate_model.trim(),
    )
    .await?;
    set_setting(
        &mut tx,
        KEY_LLM_CONCURRENCY,
        &request.settings.llm_concurrency.clamp(1, 16).to_string(),
    )
    .await?;
    let terminology_groups = normalize_terminology_groups(request.settings.terminology_groups.clone());
    let terminology_json = serde_json::to_string(&terminology_groups).map_err(|err| err.to_string())?;
    set_setting(&mut tx, KEY_TERMINOLOGY_GROUPS, &terminology_json).await?;
    set_setting(
        &mut tx,
        KEY_ENABLE_TERMINOLOGY,
        if request.settings.enable_terminology {
            "1"
        } else {
            "0"
        },
    )
    .await?;
    set_setting(
        &mut tx,
        KEY_ENABLE_PUNCTUATION_OPTIMIZATION,
        if request.settings.enable_punctuation_optimization {
            "1"
        } else {
            "0"
        },
    )
    .await?;
    set_setting(
        &mut tx,
        KEY_ENABLE_SUBTITLE_BEAUTIFY,
        if request.settings.enable_subtitle_beautify {
            "1"
        } else {
            "0"
        },
    )
    .await?;
    set_setting(
        &mut tx,
        KEY_AUTO_BURN_HARD_SUBTITLE,
        if request.settings.auto_burn_hard_subtitle {
            "1"
        } else {
            "0"
        },
    )
    .await?;
    set_setting(
        &mut tx,
        KEY_SUBTITLE_BURN_MODE,
        normalize_subtitle_burn_mode(&request.settings.subtitle_burn_mode),
    )
    .await?;
    let subtitle_render_style = normalize_subtitle_render_style(request.settings.subtitle_render_style.clone());
    let subtitle_render_style_json = serde_json::to_string(&subtitle_render_style).map_err(|err| err.to_string())?;
    set_setting(&mut tx, KEY_SUBTITLE_RENDER_STYLE, &subtitle_render_style_json).await?;
    tx.commit().await.map_err(|e| e.to_string())
}

async fn load_settings(pool: &SqlitePool) -> Result<SavedSettings, String> {
    let provider = get_setting(pool, KEY_PROVIDER)
        .await?
        .unwrap_or_else(|| "cpu".to_string());
    let chunk_target_seconds = get_setting(pool, KEY_CHUNK_TARGET_SECONDS)
        .await?
        .and_then(|v| v.parse::<u32>().ok())
        .map(|v| v.clamp(30, 300))
        .unwrap_or(180);
    let subtitle_max_words_per_segment = get_setting(pool, KEY_SUBTITLE_MAX_WORDS_PER_SEGMENT)
        .await?
        .and_then(|v| v.parse::<u32>().ok())
        .map(|v| v.clamp(8, 40))
        .unwrap_or(20);
    let subtitle_length_reference = get_setting(pool, KEY_SUBTITLE_LENGTH_REFERENCE)
        .await?
        .and_then(|v| v.parse::<u32>().ok())
        .map(|v| v.clamp(8, 80))
        .unwrap_or(28);
    let asr_model = get_setting(pool, KEY_ASR_MODEL)
        .await?
        .unwrap_or_else(|| "parakeet-tdt-0.6b-v2".to_string());
    let demucs_model = match get_setting(pool, KEY_DEMUCS_MODEL).await?.as_deref() {
        Some("htdemucs_ft") => "htdemucs_ft".to_string(),
        _ => "htdemucs_ft".to_string(),
    };
    let enable_vocal_separation = get_setting(pool, KEY_ENABLE_VOCAL_SEPARATION)
        .await?
        .map(|v| matches!(v.trim(), "1" | "true" | "TRUE" | "True"))
        .unwrap_or(false);
    let translate_api_key = get_setting(pool, KEY_TRANSLATE_API_KEY)
        .await?
        .unwrap_or_default();
    let translate_base_url = get_setting(pool, KEY_TRANSLATE_BASE_URL)
        .await?
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| "https://api.openai.com/v1".to_string());
    let translate_model = get_setting(pool, KEY_TRANSLATE_MODEL)
        .await?
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| "gpt-4.1-mini".to_string());
    let llm_concurrency = get_setting(pool, KEY_LLM_CONCURRENCY)
        .await?
        .and_then(|v| v.parse::<u32>().ok())
        .map(|v| v.clamp(1, 16))
        .unwrap_or(4);
    let terminology_groups = get_setting(pool, KEY_TERMINOLOGY_GROUPS)
        .await?
        .and_then(|v| serde_json::from_str::<Vec<TerminologyGroup>>(&v).ok())
        .map(normalize_terminology_groups)
        .unwrap_or_else(|| normalize_terminology_groups(Vec::new()));
    let enable_terminology = get_setting(pool, KEY_ENABLE_TERMINOLOGY)
        .await?
        .map(|v| matches!(v.trim(), "1" | "true" | "TRUE" | "True"))
        .unwrap_or(true);
    let enable_punctuation_optimization = get_setting(pool, KEY_ENABLE_PUNCTUATION_OPTIMIZATION)
        .await?
        .map(|v| matches!(v.trim(), "1" | "true" | "TRUE" | "True"))
        .unwrap_or(false);
    let enable_subtitle_beautify = get_setting(pool, KEY_ENABLE_SUBTITLE_BEAUTIFY)
        .await?
        .map(|v| matches!(v.trim(), "1" | "true" | "TRUE" | "True"))
        .unwrap_or(true);
    let auto_burn_hard_subtitle = get_setting(pool, KEY_AUTO_BURN_HARD_SUBTITLE)
        .await?
        .map(|v| matches!(v.trim(), "1" | "true" | "TRUE" | "True"))
        .unwrap_or(false);
    let subtitle_burn_mode = get_setting(pool, KEY_SUBTITLE_BURN_MODE)
        .await?
        .map(|v| normalize_subtitle_burn_mode(&v).to_string())
        .unwrap_or_else(default_subtitle_burn_mode);
    let subtitle_render_style = get_setting(pool, KEY_SUBTITLE_RENDER_STYLE)
        .await?
        .and_then(|v| serde_json::from_str::<SubtitleRenderStyle>(&v).ok())
        .map(normalize_subtitle_render_style)
        .unwrap_or_default();

    Ok(SavedSettings {
        provider,
        chunk_target_seconds,
        subtitle_max_words_per_segment,
        subtitle_length_reference,
        asr_model,
        demucs_model,
        enable_vocal_separation,
        translate_api_key,
        translate_base_url,
        translate_model,
        llm_concurrency,
        terminology_groups,
        enable_terminology,
        enable_punctuation_optimization,
        enable_subtitle_beautify,
        auto_burn_hard_subtitle,
        subtitle_burn_mode,
        subtitle_render_style,
    })
}

fn default_true() -> bool {
    true
}

fn default_subtitle_burn_mode() -> String {
    "bilingualSourceFirst".to_string()
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

fn normalize_subtitle_line_style(style: SubtitleLineStyle, fallback: SubtitleLineStyle) -> SubtitleLineStyle {
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

fn normalize_terminology_terms(terms: Vec<TerminologyTerm>, group_idx: usize) -> Vec<TerminologyTerm> {
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

async fn get_setting(pool: &SqlitePool, key: &str) -> Result<Option<String>, String> {
    sqlx::query_scalar::<_, String>("SELECT value FROM app_settings WHERE key = ?")
        .bind(key)
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())
}

async fn set_setting(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    key: &str,
    value: &str,
) -> Result<(), String> {
    sqlx::query(
        "INSERT INTO app_settings (key, value, updated_at) VALUES (?, ?, strftime('%s','now'))\n         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
    )
    .bind(key)
    .bind(value)
    .execute(tx.as_mut())
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}
