//! Centralized SQL operations against the voxtrans SQLite pool.

use sqlx::{Row, SqlitePool};

use crate::db::conversion::{row_from_settings, settings_from_row};
use crate::db::models::SettingsRow;
use crate::services::preferences_normalize::default_settings;
use crate::services::preferences_types::SavedSettings;

#[derive(Clone)]
pub struct TaskStore {
    pool: SqlitePool,
}

impl TaskStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    // ---- settings ----

    /// Load the single settings row, plus flat_srt_items and terminology_groups.
    /// If the row is missing (first launch), returns default settings.
    pub async fn load_settings(&self) -> Result<SavedSettings, String> {
        let row = sqlx::query(
            "SELECT provider, chunk_target_seconds, subtitle_length_preset, asr_model, \
             align_model, demucs_model, enable_vocal_separation, translate_api_key, \
             translate_base_url, translate_model, llm_concurrency, enable_terminology, \
             enable_subtitle_beautify, enable_click_sound, auto_burn_hard_subtitle, \
             subtitle_burn_mode, source_font_family, source_font_size, source_primary_color, \
             source_outline_color, source_back_color, source_outline, source_shadow, \
             source_border_style, source_border_opacity, target_font_family, target_font_size, \
             target_primary_color, target_outline_color, target_back_color, target_outline, \
             target_shadow, target_border_style, target_border_opacity, margin_v, alignment, \
             bilingual_line_gap, flat_srt_output, updated_at FROM settings WHERE id = 1",
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| format!("load settings: {e}"))?;

        let mut settings = match row {
            None => default_settings(),
            Some(r) => settings_from_row(SettingsRow {
                provider: r.get("provider"),
                chunk_target_seconds: r.get::<i64, _>("chunk_target_seconds") as u32,
                subtitle_length_preset: r.get("subtitle_length_preset"),
                asr_model: r.get("asr_model"),
                align_model: r.get("align_model"),
                demucs_model: r.get("demucs_model"),
                enable_vocal_separation: r.get::<i64, _>("enable_vocal_separation") != 0,
                translate_api_key: r.get("translate_api_key"),
                translate_base_url: r.get("translate_base_url"),
                translate_model: r.get("translate_model"),
                llm_concurrency: r.get::<i64, _>("llm_concurrency") as u32,
                enable_terminology: r.get::<i64, _>("enable_terminology") != 0,
                enable_subtitle_beautify: r.get::<i64, _>("enable_subtitle_beautify") != 0,
                enable_click_sound: r.get::<i64, _>("enable_click_sound") != 0,
                auto_burn_hard_subtitle: r.get::<i64, _>("auto_burn_hard_subtitle") != 0,
                subtitle_burn_mode: r.get("subtitle_burn_mode"),
                source_font_family: r.get("source_font_family"),
                source_font_size: r.get::<i64, _>("source_font_size") as u32,
                source_primary_color: r.get("source_primary_color"),
                source_outline_color: r.get("source_outline_color"),
                source_back_color: r.get("source_back_color"),
                source_outline: r.get("source_outline"),
                source_shadow: r.get("source_shadow"),
                source_border_style: r.get("source_border_style"),
                source_border_opacity: r.get::<i64, _>("source_border_opacity") as u8,
                target_font_family: r.get("target_font_family"),
                target_font_size: r.get::<i64, _>("target_font_size") as u32,
                target_primary_color: r.get("target_primary_color"),
                target_outline_color: r.get("target_outline_color"),
                target_back_color: r.get("target_back_color"),
                target_outline: r.get("target_outline"),
                target_shadow: r.get("target_shadow"),
                target_border_style: r.get("target_border_style"),
                target_border_opacity: r.get::<i64, _>("target_border_opacity") as u8,
                margin_v: r.get::<i64, _>("margin_v") as u32,
                alignment: r.get::<i64, _>("alignment") as u8,
                bilingual_line_gap: r.get::<i64, _>("bilingual_line_gap") as u32,
                flat_srt_output: r.get::<i64, _>("flat_srt_output") != 0,
                updated_at: r.get("updated_at"),
            }),
        };

        // Compose flat_srt_items.
        let items = sqlx::query("SELECT value FROM flat_srt_items ORDER BY id")
            .fetch_all(&self.pool)
            .await
            .map_err(|e| format!("load flat_srt_items: {e}"))?;
        settings.flat_srt_items = items
            .into_iter()
            .map(|r| r.get::<String, _>("value"))
            .collect();

        // Compose terminology_groups + terms.
        settings.terminology_groups = self.load_terminology_groups_internal().await?;

        Ok(settings)
    }

    /// Persist settings: UPSERT row + replace flat_srt_items + replace terminology.
    pub async fn save_settings(&self, settings: &SavedSettings) -> Result<(), String> {
        let mut tx = self.pool.begin().await.map_err(|e| format!("begin tx: {e}"))?;

        let row = row_from_settings(settings);
        sqlx::query(
            "INSERT INTO settings (id, provider, chunk_target_seconds, subtitle_length_preset, \
             asr_model, align_model, demucs_model, enable_vocal_separation, translate_api_key, \
             translate_base_url, translate_model, llm_concurrency, enable_terminology, \
             enable_subtitle_beautify, enable_click_sound, auto_burn_hard_subtitle, \
             subtitle_burn_mode, source_font_family, source_font_size, source_primary_color, \
             source_outline_color, source_back_color, source_outline, source_shadow, \
             source_border_style, source_border_opacity, target_font_family, target_font_size, \
             target_primary_color, target_outline_color, target_back_color, target_outline, \
             target_shadow, target_border_style, target_border_opacity, margin_v, alignment, \
             bilingual_line_gap, flat_srt_output, updated_at) VALUES (1, ?, ?, ?, ?, ?, ?, \
             ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, \
             ?, ?, ?, ?, ?, ?, ?) ON CONFLICT(id) DO UPDATE SET \
             provider=excluded.provider, chunk_target_seconds=excluded.chunk_target_seconds, \
             subtitle_length_preset=excluded.subtitle_length_preset, asr_model=excluded.asr_model, \
             align_model=excluded.align_model, demucs_model=excluded.demucs_model, \
             enable_vocal_separation=excluded.enable_vocal_separation, \
             translate_api_key=excluded.translate_api_key, translate_base_url=excluded.translate_base_url, \
             translate_model=excluded.translate_model, llm_concurrency=excluded.llm_concurrency, \
             enable_terminology=excluded.enable_terminology, \
             enable_subtitle_beautify=excluded.enable_subtitle_beautify, \
             enable_click_sound=excluded.enable_click_sound, \
             auto_burn_hard_subtitle=excluded.auto_burn_hard_subtitle, \
             subtitle_burn_mode=excluded.subtitle_burn_mode, \
             source_font_family=excluded.source_font_family, source_font_size=excluded.source_font_size, \
             source_primary_color=excluded.source_primary_color, \
             source_outline_color=excluded.source_outline_color, \
             source_back_color=excluded.source_back_color, source_outline=excluded.source_outline, \
             source_shadow=excluded.source_shadow, source_border_style=excluded.source_border_style, \
             source_border_opacity=excluded.source_border_opacity, \
             target_font_family=excluded.target_font_family, target_font_size=excluded.target_font_size, \
             target_primary_color=excluded.target_primary_color, \
             target_outline_color=excluded.target_outline_color, \
             target_back_color=excluded.target_back_color, target_outline=excluded.target_outline, \
             target_shadow=excluded.target_shadow, target_border_style=excluded.target_border_style, \
             target_border_opacity=excluded.target_border_opacity, margin_v=excluded.margin_v, \
             alignment=excluded.alignment, bilingual_line_gap=excluded.bilingual_line_gap, \
             flat_srt_output=excluded.flat_srt_output, updated_at=excluded.updated_at",
        )
        .bind(&row.provider)
        .bind(row.chunk_target_seconds as i64)
        .bind(&row.subtitle_length_preset)
        .bind(&row.asr_model)
        .bind(&row.align_model)
        .bind(&row.demucs_model)
        .bind(row.enable_vocal_separation as i64)
        .bind(&row.translate_api_key)
        .bind(&row.translate_base_url)
        .bind(&row.translate_model)
        .bind(row.llm_concurrency as i64)
        .bind(row.enable_terminology as i64)
        .bind(row.enable_subtitle_beautify as i64)
        .bind(row.enable_click_sound as i64)
        .bind(row.auto_burn_hard_subtitle as i64)
        .bind(&row.subtitle_burn_mode)
        .bind(&row.source_font_family)
        .bind(row.source_font_size as i64)
        .bind(&row.source_primary_color)
        .bind(&row.source_outline_color)
        .bind(&row.source_back_color)
        .bind(row.source_outline)
        .bind(row.source_shadow)
        .bind(&row.source_border_style)
        .bind(row.source_border_opacity as i64)
        .bind(&row.target_font_family)
        .bind(row.target_font_size as i64)
        .bind(&row.target_primary_color)
        .bind(&row.target_outline_color)
        .bind(&row.target_back_color)
        .bind(row.target_outline)
        .bind(row.target_shadow)
        .bind(&row.target_border_style)
        .bind(row.target_border_opacity as i64)
        .bind(row.margin_v as i64)
        .bind(row.alignment as i64)
        .bind(row.bilingual_line_gap as i64)
        .bind(row.flat_srt_output as i64)
        .bind(row.updated_at)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("upsert settings: {e}"))?;

        // Replace flat_srt_items.
        sqlx::query("DELETE FROM flat_srt_items")
            .execute(&mut *tx)
            .await
            .map_err(|e| format!("delete flat_srt_items: {e}"))?;
        for value in &settings.flat_srt_items {
            sqlx::query("INSERT INTO flat_srt_items (value, updated_at) VALUES (?, ?)")
                .bind(value)
                .bind(row.updated_at)
                .execute(&mut *tx)
                .await
                .map_err(|e| format!("insert flat_srt_item: {e}"))?;
        }

        // Replace terminology_groups (CASCADE deletes terms).
        sqlx::query("DELETE FROM terminology_groups")
            .execute(&mut *tx)
            .await
            .map_err(|e| format!("delete terminology_groups: {e}"))?;
        for group in &settings.terminology_groups {
            sqlx::query("INSERT INTO terminology_groups (id, name, updated_at) VALUES (?, ?, ?)")
                .bind(&group.id)
                .bind(&group.name)
                .bind(row.updated_at)
                .execute(&mut *tx)
                .await
                .map_err(|e| format!("insert terminology_group: {e}"))?;
            for term in &group.terms {
                sqlx::query(
                    "INSERT INTO terminology_terms (id, group_id, origin, target, note, updated_at) \
                     VALUES (?, ?, ?, ?, ?, ?)",
                )
                .bind(&term.id)
                .bind(&group.id)
                .bind(&term.origin)
                .bind(&term.target)
                .bind(&term.note)
                .bind(row.updated_at)
                .execute(&mut *tx)
                .await
                .map_err(|e| format!("insert terminology_term: {e}"))?;
            }
        }

        tx.commit().await.map_err(|e| format!("commit tx: {e}"))?;
        Ok(())
    }

    async fn load_terminology_groups_internal(
        &self,
    ) -> Result<Vec<crate::services::preferences_types::TerminologyGroup>, String> {
        use std::collections::HashMap;
        use crate::services::preferences_types::{TerminologyGroup, TerminologyTerm};

        let group_rows = sqlx::query("SELECT id, name FROM terminology_groups")
            .fetch_all(&self.pool)
            .await
            .map_err(|e| format!("load terminology_groups: {e}"))?;

        let term_rows = sqlx::query(
            "SELECT id, group_id, origin, target, note FROM terminology_terms",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| format!("load terminology_terms: {e}"))?;

        let mut terms_by_group: HashMap<String, Vec<TerminologyTerm>> = HashMap::new();
        for r in term_rows {
            let group_id: String = r.get("group_id");
            let term = TerminologyTerm {
                id: r.get("id"),
                origin: r.get("origin"),
                target: r.get("target"),
                note: r.get("note"),
            };
            terms_by_group.entry(group_id).or_default().push(term);
        }

        let mut groups: Vec<TerminologyGroup> = group_rows
            .into_iter()
            .map(|r| {
                let id: String = r.get("id");
                let name: String = r.get("name");
                let terms = terms_by_group.remove(&id).unwrap_or_default();
                TerminologyGroup { id, name, terms }
            })
            .collect();

        // Stable order by id for deterministic round-trip.
        groups.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(groups)
    }
}

#[cfg(test)]
pub async fn test_pool() -> SqlitePool {
    // In-memory SQLite, no migrations needed for unit tests of pure conversion.
    // For tests that need full schema, use test_pool_with_migrations.
    sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            sqlx::sqlite::SqliteConnectOptions::new()
                .filename(":memory:")
                .create_if_missing(true),
        )
        .await
        .expect("connect in-memory sqlite")
}

#[cfg(test)]
pub async fn test_pool_with_migrations() -> SqlitePool {
    let pool = test_pool().await;
    sqlx::query("PRAGMA foreign_keys = ON")
        .execute(&pool)
        .await
        .expect("enable FK");
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("run migrations");
    pool
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::preferences_types::{
        SavedSettings, SubtitleRenderStyle, TerminologyGroup, TerminologyTerm,
    };

    async fn store() -> TaskStore {
        let pool = super::test_pool_with_migrations().await;
        TaskStore::new(pool)
    }

    fn sample_settings() -> SavedSettings {
        let mut s = SavedSettings {
            provider: "openai".into(),
            chunk_target_seconds: 30,
            subtitle_length_preset: "default".into(),
            asr_model: "Qwen3-ASR".into(),
            align_model: "Qwen3-ForcedAligner-0.6B".into(),
            demucs_model: "htdemucs".into(),
            enable_vocal_separation: true,
            translate_api_key: "k".into(),
            translate_base_url: "https://api.example.com".into(),
            translate_model: "gpt-4o".into(),
            llm_concurrency: 4,
            terminology_groups: Vec::new(),
            enable_terminology: true,
            enable_subtitle_beautify: true,
            enable_click_sound: true,
            auto_burn_hard_subtitle: false,
            subtitle_burn_mode: "bilingualSourceFirst".into(),
            subtitle_render_style: SubtitleRenderStyle::default(),
            flat_srt_output: false,
            flat_srt_items: Vec::new(),
        };
        s.flat_srt_items = vec!["source".into(), "target".into()];
        s.terminology_groups = vec![TerminologyGroup {
            id: "g1".into(),
            name: "Default".into(),
            terms: vec![TerminologyTerm {
                id: "t1".into(),
                origin: "machine learning".into(),
                target: "机器学习".into(),
                note: "".into(),
            }],
        }];
        s
    }

    #[tokio::test]
    async fn load_settings_returns_default_when_empty() {
        let s = store().await;
        let settings = s.load_settings().await.expect("load");
        // Default has flat_srt_items from defaults; assert provider default
        assert!(!settings.provider.is_empty() || settings.provider == "openai");
    }

    #[tokio::test]
    async fn save_then_load_roundtrips_settings() {
        let s = store().await;
        let original = sample_settings();
        s.save_settings(&original).await.expect("save");

        let loaded = s.load_settings().await.expect("load");
        assert_eq!(loaded.provider, original.provider);
        assert_eq!(loaded.translate_model, original.translate_model);
        assert_eq!(loaded.flat_srt_items, original.flat_srt_items);
        assert_eq!(loaded.terminology_groups.len(), 1);
        assert_eq!(loaded.terminology_groups[0].name, "Default");
        assert_eq!(loaded.terminology_groups[0].terms.len(), 1);
        assert_eq!(loaded.terminology_groups[0].terms[0].origin, "machine learning");
    }
}
