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
    let terminology_json = serde_json::to_string(&terminology_groups)
        .map_err(|err| err.to_string())?;
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
    })
}

fn default_true() -> bool {
    true
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
        "INSERT INTO app_settings (key, value, updated_at) VALUES (?, ?, strftime('%s','now'))
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
    )
    .bind(key)
    .bind(value)
    .execute(tx.as_mut())
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}
