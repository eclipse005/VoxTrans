use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

const KEY_PROVIDER: &str = "settings.provider";
const KEY_CHUNK_TARGET_SECONDS: &str = "settings.chunkTargetSeconds";
const KEY_SUBTITLE_MAX_WORDS_PER_SEGMENT: &str = "settings.subtitleMaxWordsPerSegment";
const KEY_ASR_MODEL: &str = "settings.asrModel";
const KEY_DEMUCS_MODEL: &str = "settings.demucsModel";
const KEY_ENABLE_VOCAL_SEPARATION: &str = "settings.enableVocalSeparation";
const KEY_TRANSLATE_API_KEY: &str = "settings.translateApiKey";
const KEY_TRANSLATE_BASE_URL: &str = "settings.translateBaseUrl";
const KEY_TRANSLATE_MODEL: &str = "settings.translateModel";
const KEY_ENABLE_PUNCTUATION_OPTIMIZATION: &str = "settings.enablePunctuationOptimization";

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedSettings {
    pub provider: String,
    pub chunk_target_seconds: u32,
    pub subtitle_max_words_per_segment: u32,
    pub asr_model: String,
    pub demucs_model: String,
    pub enable_vocal_separation: bool,
    pub translate_api_key: String,
    pub translate_base_url: String,
    pub translate_model: String,
    pub enable_punctuation_optimization: bool,
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
        KEY_ENABLE_PUNCTUATION_OPTIMIZATION,
        if request.settings.enable_punctuation_optimization {
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
        .unwrap_or(300);
    let subtitle_max_words_per_segment = get_setting(pool, KEY_SUBTITLE_MAX_WORDS_PER_SEGMENT)
        .await?
        .and_then(|v| v.parse::<u32>().ok())
        .map(|v| v.clamp(8, 40))
        .unwrap_or(20);
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
    let enable_punctuation_optimization = get_setting(pool, KEY_ENABLE_PUNCTUATION_OPTIMIZATION)
        .await?
        .map(|v| matches!(v.trim(), "1" | "true" | "TRUE" | "True"))
        .unwrap_or(false);
    Ok(SavedSettings {
        provider,
        chunk_target_seconds,
        subtitle_max_words_per_segment,
        asr_model,
        demucs_model,
        enable_vocal_separation,
        translate_api_key,
        translate_base_url,
        translate_model,
        enable_punctuation_optimization,
    })
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
