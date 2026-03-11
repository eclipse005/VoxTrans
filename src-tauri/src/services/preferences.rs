use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

const KEY_PROVIDER: &str = "settings.provider";
const KEY_CHUNK_TARGET_SECONDS: &str = "settings.chunkTargetSeconds";
const KEY_LLM_API_KEY: &str = "llm.apiKey";
const KEY_LLM_API_BASE: &str = "llm.apiBase";
const KEY_LLM_API_MODEL: &str = "llm.apiModel";

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedSettings {
    pub provider: String,
    pub chunk_target_seconds: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LlmSettings {
    pub api_key: String,
    pub api_base: String,
    pub api_model: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TermEntry {
    pub id: String,
    pub source: String,
    pub target: String,
    pub note: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HotwordGroup {
    pub id: String,
    pub name: String,
    pub keyterms: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HotwordCorrection {
    pub enabled: bool,
    pub active_group_id: String,
    pub groups: Vec<HotwordGroup>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserPreferencesResponse {
    pub settings: SavedSettings,
    pub llm: LlmSettings,
    pub terms: Vec<TermEntry>,
    pub hotword_correction: HotwordCorrection,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveAppSettingsRequest {
    pub settings: SavedSettings,
    pub llm: LlmSettings,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveTermsRequest {
    pub terms: Vec<TermEntry>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveHotwordCorrectionRequest {
    pub hotword_correction: HotwordCorrection,
}

pub async fn load_user_preferences(pool: &SqlitePool) -> Result<UserPreferencesResponse, String> {
    let settings = load_settings(pool).await?;
    let llm = load_llm(pool).await?;
    let terms = load_terms(pool).await?;
    let hotword_correction = load_hotword_correction(pool).await?;

    Ok(UserPreferencesResponse {
        settings,
        llm,
        terms,
        hotword_correction,
    })
}

pub async fn save_app_settings(
    pool: &SqlitePool,
    request: SaveAppSettingsRequest,
) -> Result<(), String> {
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    set_setting(&mut *tx, KEY_PROVIDER, &request.settings.provider).await?;
    set_setting(
        &mut *tx,
        KEY_CHUNK_TARGET_SECONDS,
        &request.settings.chunk_target_seconds.to_string(),
    )
    .await?;
    set_setting(&mut *tx, KEY_LLM_API_KEY, &request.llm.api_key).await?;
    set_setting(&mut *tx, KEY_LLM_API_BASE, &request.llm.api_base).await?;
    set_setting(&mut *tx, KEY_LLM_API_MODEL, &request.llm.api_model).await?;
    tx.commit().await.map_err(|e| e.to_string())
}

pub async fn save_terms(pool: &SqlitePool, request: SaveTermsRequest) -> Result<(), String> {
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    sqlx::query("DELETE FROM terms")
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;

    for (index, term) in request.terms.iter().enumerate() {
        sqlx::query(
            "INSERT INTO terms (id, source, target, note, sort_order) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&term.id)
        .bind(&term.source)
        .bind(&term.target)
        .bind(&term.note)
        .bind(index as i64)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    }

    tx.commit().await.map_err(|e| e.to_string())
}

pub async fn save_hotword_correction(
    pool: &SqlitePool,
    request: SaveHotwordCorrectionRequest,
) -> Result<(), String> {
    if request.hotword_correction.groups.is_empty() {
        return Err("hotwordCorrection.groups must not be empty".to_string());
    }

    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;

    sqlx::query("DELETE FROM hotword_terms")
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    sqlx::query("DELETE FROM hotword_groups")
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    sqlx::query("DELETE FROM hotword_meta")
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;

    for (group_index, group) in request.hotword_correction.groups.iter().enumerate() {
        sqlx::query("INSERT INTO hotword_groups (id, name, sort_order) VALUES (?, ?, ?)")
            .bind(&group.id)
            .bind(&group.name)
            .bind(group_index as i64)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;

        for (term_index, term) in group.keyterms.iter().enumerate() {
            sqlx::query("INSERT INTO hotword_terms (group_id, term, sort_order) VALUES (?, ?, ?)")
                .bind(&group.id)
                .bind(term)
                .bind(term_index as i64)
                .execute(&mut *tx)
                .await
                .map_err(|e| e.to_string())?;
        }
    }

    sqlx::query(
        "INSERT INTO hotword_meta (singleton_id, enabled, active_group_id) VALUES (1, ?, ?)",
    )
    .bind(if request.hotword_correction.enabled {
        1_i64
    } else {
        0_i64
    })
    .bind(&request.hotword_correction.active_group_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| e.to_string())?;

    tx.commit().await.map_err(|e| e.to_string())
}

async fn load_settings(pool: &SqlitePool) -> Result<SavedSettings, String> {
    let provider = get_setting(pool, KEY_PROVIDER)
        .await?
        .unwrap_or_else(|| "cuda".to_string());
    let chunk_target_seconds = get_setting(pool, KEY_CHUNK_TARGET_SECONDS)
        .await?
        .and_then(|v| v.parse::<u32>().ok())
        .map(|v| v.clamp(60, 1800))
        .unwrap_or(300);

    Ok(SavedSettings {
        provider,
        chunk_target_seconds,
    })
}

async fn load_llm(pool: &SqlitePool) -> Result<LlmSettings, String> {
    Ok(LlmSettings {
        api_key: get_setting(pool, KEY_LLM_API_KEY)
            .await?
            .unwrap_or_default(),
        api_base: get_setting(pool, KEY_LLM_API_BASE)
            .await?
            .unwrap_or_default(),
        api_model: get_setting(pool, KEY_LLM_API_MODEL)
            .await?
            .unwrap_or_default(),
    })
}

async fn load_terms(pool: &SqlitePool) -> Result<Vec<TermEntry>, String> {
    let rows = sqlx::query_as::<_, (String, String, String, String)>(
        "SELECT id, source, target, note FROM terms ORDER BY sort_order ASC, id ASC",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(rows
        .into_iter()
        .map(|(id, source, target, note)| TermEntry {
            id,
            source,
            target,
            note,
        })
        .collect())
}

async fn load_hotword_correction(pool: &SqlitePool) -> Result<HotwordCorrection, String> {
    let groups_rows = sqlx::query_as::<_, (String, String)>(
        "SELECT id, name FROM hotword_groups ORDER BY sort_order ASC, id ASC",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    let mut groups = Vec::new();
    for (group_id, group_name) in groups_rows {
        let keyterms = sqlx::query_scalar::<_, String>(
            "SELECT term FROM hotword_terms WHERE group_id = ? ORDER BY sort_order ASC, id ASC",
        )
        .bind(&group_id)
        .fetch_all(pool)
        .await
        .map_err(|e| e.to_string())?;
        groups.push(HotwordGroup {
            id: group_id,
            name: group_name,
            keyterms,
        });
    }

    if groups.is_empty() {
        groups.push(HotwordGroup {
            id: "group-0".to_string(),
            name: "默认分组".to_string(),
            keyterms: vec![],
        });
    }

    let meta = sqlx::query_as::<_, (i64, String)>(
        "SELECT enabled, active_group_id FROM hotword_meta WHERE singleton_id = 1",
    )
    .fetch_optional(pool)
    .await
    .map_err(|e| e.to_string())?;

    let default_active_group_id = groups
        .first()
        .map(|g| g.id.clone())
        .unwrap_or_else(|| "group-0".to_string());

    let (enabled, active_group_id) = match meta {
        Some((enabled, active_group_id)) => (enabled != 0, active_group_id),
        None => (true, default_active_group_id),
    };

    let active_group_id = if groups.iter().any(|g| g.id == active_group_id) {
        active_group_id
    } else {
        groups
            .first()
            .map(|g| g.id.clone())
            .unwrap_or_else(|| "group-0".to_string())
    };

    Ok(HotwordCorrection {
        enabled,
        active_group_id,
        groups,
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
    executor: &mut sqlx::SqliteConnection,
    key: &str,
    value: &str,
) -> Result<(), String> {
    sqlx::query(
        "INSERT INTO app_settings (key, value, updated_at) VALUES (?, ?, strftime('%s','now'))
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
    )
    .bind(key)
    .bind(value)
    .execute(executor)
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}
