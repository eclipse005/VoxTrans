use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

const KEY_PROVIDER: &str = "settings.provider";
const KEY_CHUNK_TARGET_SECONDS: &str = "settings.chunkTargetSeconds";
const KEY_SUBTITLE_MAX_WORDS_PER_SEGMENT: &str = "settings.subtitleMaxWordsPerSegment";

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedSettings {
    pub provider: String,
    pub chunk_target_seconds: u32,
    pub subtitle_max_words_per_segment: u32,
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
        &request.settings.chunk_target_seconds.clamp(60, 300).to_string(),
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
    tx.commit().await.map_err(|e| e.to_string())
}

async fn load_settings(pool: &SqlitePool) -> Result<SavedSettings, String> {
    let provider = get_setting(pool, KEY_PROVIDER)
        .await?
        .unwrap_or_else(|| "cpu".to_string());
    let chunk_target_seconds = get_setting(pool, KEY_CHUNK_TARGET_SECONDS)
        .await?
        .and_then(|v| v.parse::<u32>().ok())
        .map(|v| v.clamp(60, 300))
        .unwrap_or(300);
    let subtitle_max_words_per_segment = get_setting(pool, KEY_SUBTITLE_MAX_WORDS_PER_SEGMENT)
        .await?
        .and_then(|v| v.parse::<u32>().ok())
        .map(|v| v.clamp(8, 40))
        .unwrap_or(20);
    Ok(SavedSettings {
        provider,
        chunk_target_seconds,
        subtitle_max_words_per_segment,
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
