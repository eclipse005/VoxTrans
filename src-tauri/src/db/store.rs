//! Centralized SQL operations against the voxtrans SQLite pool.

use sqlx::{Row, SqlitePool};

use crate::commands::workspace::WorkspaceQueueItem;
use crate::db::conversion::{
    TaskMetaExtras, row_from_segment, row_from_settings, row_from_task, settings_from_row,
    task_from_row,
};
use crate::db::models::{SettingsRow, TaskRow};
use crate::services::preferences_normalize::default_settings;
use crate::services::preferences_types::SavedSettings;

fn now_ms() -> i64 {
    crate::db::now_ms()
}

#[derive(Debug, Clone)]
pub struct TaskStore {
    pool: SqlitePool,
}

impl TaskStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    #[allow(dead_code)]
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

    // ---- subtitle segments + words ----

    /// Replace all segments + words for a task (delete + insert in one tx).
    pub async fn replace_segments(
        &self,
        task_id: &str,
        segments: &[crate::services::workspace_subtitle::WorkspaceSubtitleSegment],
    ) -> Result<(), String> {
        let mut tx = self.pool.begin().await.map_err(|e| format!("begin tx: {e}"))?;
        // CASCADE on subtitle_segments deletes words too.
        sqlx::query("DELETE FROM subtitle_segments WHERE task_id = ?")
            .bind(task_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| format!("delete segments: {e}"))?;
        for (idx, seg) in segments.iter().enumerate() {
            let (seg_row, word_rows) = row_from_segment(task_id, idx as i32, seg);
            sqlx::query(
                "INSERT INTO subtitle_segments (id, task_id, idx, start_ms, end_ms, \
                 source_text, translated_text, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(&seg_row.id)
            .bind(&seg_row.task_id)
            .bind(seg_row.idx)
            .bind(seg_row.start_ms as i64)
            .bind(seg_row.end_ms as i64)
            .bind(&seg_row.source_text)
            .bind(&seg_row.translated_text)
            .bind(seg_row.updated_at)
            .execute(&mut *tx)
            .await
            .map_err(|e| format!("insert segment: {e}"))?;
            for w in &word_rows {
                sqlx::query(
                    "INSERT INTO subtitle_words (id, segment_id, idx, start_ms, end_ms, word, updated_at) \
                     VALUES (?, ?, ?, ?, ?, ?, ?)",
                )
                .bind(&w.id)
                .bind(&w.segment_id)
                .bind(w.idx)
                .bind(w.start_ms as i64)
                .bind(w.end_ms as i64)
                .bind(&w.word)
                .bind(w.updated_at)
                .execute(&mut *tx)
                .await
                .map_err(|e| format!("insert word: {e}"))?;
            }
        }
        tx.commit().await.map_err(|e| format!("commit tx: {e}"))?;
        Ok(())
    }

    /// Load all segments + words for a task, joined into WorkspaceSubtitleSegment.
    /// Returns empty vec if task has no segments.
    pub async fn load_segments(
        &self,
        task_id: &str,
    ) -> Result<Vec<crate::services::workspace_subtitle::WorkspaceSubtitleSegment>, String> {
        let seg_rows = sqlx::query(
            "SELECT id, task_id, idx, start_ms, end_ms, source_text, translated_text, updated_at \
             FROM subtitle_segments WHERE task_id = ? ORDER BY idx",
        )
        .bind(task_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| format!("load segments: {e}"))?;

        let word_rows = sqlx::query(
            "SELECT w.id, w.segment_id, w.idx, w.start_ms, w.end_ms, w.word, w.updated_at \
             FROM subtitle_words w JOIN subtitle_segments s ON w.segment_id = s.id \
             WHERE s.task_id = ? ORDER BY w.segment_id, w.idx",
        )
        .bind(task_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| format!("load words: {e}"))?;

        use std::collections::HashMap;
        let mut words_by_seg: HashMap<String, Vec<crate::services::workspace_subtitle::WorkspaceSubtitleWord>> = HashMap::new();
        for r in word_rows {
            let seg_id: String = r.get("segment_id");
            words_by_seg.entry(seg_id).or_default().push(
                crate::services::workspace_subtitle::WorkspaceSubtitleWord {
                    start_ms: r.get::<i64, _>("start_ms") as u64,
                    end_ms: r.get::<i64, _>("end_ms") as u64,
                    word: r.get("word"),
                },
            );
        }

        let mut out = Vec::with_capacity(seg_rows.len());
        for r in seg_rows {
            let id: String = r.get("id");
            out.push(crate::services::workspace_subtitle::WorkspaceSubtitleSegment {
                start_ms: r.get::<i64, _>("start_ms") as u64,
                end_ms: r.get::<i64, _>("end_ms") as u64,
                source_text: r.get("source_text"),
                translated_text: r.get("translated_text"),
                source_words: words_by_seg.remove(&id).unwrap_or_default(),
            });
        }
        Ok(out)
    }

    // ---- tasks ----

    pub async fn load_all_tasks(&self) -> Result<Vec<(WorkspaceQueueItem, TaskMetaExtras)>, String> {
        let rows = sqlx::query(
            "SELECT id, media_path, name, media_kind, size_bytes, source_lang, target_lang, \
             transcribe_status, task_progress_stage_code, task_progress_stage_label, \
             task_progress_stage_order, task_progress_detail, task_progress_current, \
             task_progress_total, transcribe_error, result_text, result_srt, llm_total_tokens, \
             intent, max_retries, subtitle_length_preset, enable_terminology, \
             enable_subtitle_beautify, terminology_groups_json, updated_at \
             FROM tasks ORDER BY updated_at DESC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| format!("load tasks: {e}"))?;

        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            let row = TaskRow {
                id: r.get("id"),
                media_path: r.get("media_path"),
                name: r.get("name"),
                media_kind: r.get("media_kind"),
                size_bytes: r.get::<i64, _>("size_bytes") as u64,
                source_lang: r.get("source_lang"),
                target_lang: r.get("target_lang"),
                transcribe_status: r.get("transcribe_status"),
                task_progress_stage_code: r.get("task_progress_stage_code"),
                task_progress_stage_label: r.get("task_progress_stage_label"),
                task_progress_stage_order: r.get::<i64, _>("task_progress_stage_order") as u32,
                task_progress_detail: r.get("task_progress_detail"),
                task_progress_current: r.get::<i64, _>("task_progress_current") as u32,
                task_progress_total: r.get::<i64, _>("task_progress_total") as u32,
                transcribe_error: r.get("transcribe_error"),
                result_text: r.get("result_text"),
                result_srt: r.get("result_srt"),
                llm_total_tokens: r.get::<i64, _>("llm_total_tokens") as u64,
                intent: r.get("intent"),
                max_retries: r.get::<i64, _>("max_retries") as u32,
                subtitle_length_preset: r.get("subtitle_length_preset"),
                enable_terminology: r.get::<i64, _>("enable_terminology") != 0,
                enable_subtitle_beautify: r.get::<i64, _>("enable_subtitle_beautify") != 0,
                terminology_groups_json: r.get("terminology_groups_json"),
                updated_at: r.get("updated_at"),
            };
            out.push(task_from_row(row));
        }
        Ok(out)
    }

    pub async fn upsert_task(
        &self,
        item: &WorkspaceQueueItem,
        extras: &TaskMetaExtras,
    ) -> Result<(), String> {
        let row = row_from_task(item, extras);
        sqlx::query(
            "INSERT INTO tasks (id, media_path, name, media_kind, size_bytes, source_lang, \
             target_lang, transcribe_status, task_progress_stage_code, \
             task_progress_stage_label, task_progress_stage_order, task_progress_detail, \
             task_progress_current, task_progress_total, transcribe_error, result_text, \
             result_srt, llm_total_tokens, intent, max_retries, subtitle_length_preset, \
             enable_terminology, enable_subtitle_beautify, terminology_groups_json, \
             updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, \
             ?, ?, ?, ?, ?, ?, ?, ?) ON CONFLICT(id) DO UPDATE SET \
             media_path=excluded.media_path, name=excluded.name, media_kind=excluded.media_kind, \
             size_bytes=excluded.size_bytes, source_lang=excluded.source_lang, \
             target_lang=excluded.target_lang, transcribe_status=excluded.transcribe_status, \
             task_progress_stage_code=excluded.task_progress_stage_code, \
             task_progress_stage_label=excluded.task_progress_stage_label, \
             task_progress_stage_order=excluded.task_progress_stage_order, \
             task_progress_detail=excluded.task_progress_detail, \
             task_progress_current=excluded.task_progress_current, \
             task_progress_total=excluded.task_progress_total, \
             transcribe_error=excluded.transcribe_error, result_text=excluded.result_text, \
             result_srt=excluded.result_srt, llm_total_tokens=excluded.llm_total_tokens, \
             intent=excluded.intent, max_retries=excluded.max_retries, \
             subtitle_length_preset=excluded.subtitle_length_preset, \
             enable_terminology=excluded.enable_terminology, \
             enable_subtitle_beautify=excluded.enable_subtitle_beautify, \
             terminology_groups_json=excluded.terminology_groups_json, \
             updated_at=excluded.updated_at",
        )
        .bind(&row.id)
        .bind(&row.media_path)
        .bind(&row.name)
        .bind(&row.media_kind)
        .bind(row.size_bytes as i64)
        .bind(&row.source_lang)
        .bind(&row.target_lang)
        .bind(&row.transcribe_status)
        .bind(&row.task_progress_stage_code)
        .bind(&row.task_progress_stage_label)
        .bind(row.task_progress_stage_order as i64)
        .bind(&row.task_progress_detail)
        .bind(row.task_progress_current as i64)
        .bind(row.task_progress_total as i64)
        .bind(&row.transcribe_error)
        .bind(&row.result_text)
        .bind(&row.result_srt)
        .bind(row.llm_total_tokens as i64)
        .bind(&row.intent)
        .bind(row.max_retries as i64)
        .bind(&row.subtitle_length_preset)
        .bind(row.enable_terminology as i64)
        .bind(row.enable_subtitle_beautify as i64)
        .bind(&row.terminology_groups_json)
        .bind(row.updated_at)
        .execute(&self.pool)
        .await
        .map_err(|e| format!("upsert task: {e}"))?;
        Ok(())
    }

    pub async fn update_task_tokens(&self, id: &str, total_tokens: u64) -> Result<(), String> {
        sqlx::query("UPDATE tasks SET llm_total_tokens = ?, updated_at = ? WHERE id = ?")
            .bind(total_tokens as i64)
            .bind(now_ms())
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| format!("update task tokens: {e}"))?;
        Ok(())
    }

    pub async fn delete_task(&self, id: &str) -> Result<(), String> {
        sqlx::query("DELETE FROM tasks WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| format!("delete task: {e}"))?;
        Ok(())
    }

    /// Mark all tasks with transcribe_status='processing' as 'error' (startup recovery).
    /// Returns the count of affected rows.
    pub async fn mark_orphan_processing_as_error(&self) -> Result<u64, String> {
        let n = sqlx::query(
            "UPDATE tasks SET transcribe_status = 'error', \
             transcribe_error = '任务在运行中被中断，请重新开始', \
             updated_at = ? WHERE transcribe_status = 'processing'",
        )
        .bind(now_ms())
        .execute(&self.pool)
        .await
        .map_err(|e| format!("recover orphan tasks: {e}"))?
        .rows_affected();
        Ok(n)
    }

    // ---- task_artifacts (pipeline step checkpoints) ----

    /// Load a cached artifact JSON for a given task + step, or None if not cached.
    pub async fn load_artifact(
        &self,
        task_id: &str,
        step_name: &str,
    ) -> Result<Option<String>, String> {
        let row = sqlx::query(
            "SELECT payload_json FROM task_artifacts WHERE task_id = ? AND step_name = ?",
        )
        .bind(task_id)
        .bind(step_name)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| format!("load artifact: {e}"))?;

        Ok(row.map(|r| r.get("payload_json")))
    }

    /// Save (upsert) a pipeline step artifact.
    pub async fn save_artifact(
        &self,
        task_id: &str,
        step_name: &str,
        payload_json: &str,
    ) -> Result<(), String> {
        sqlx::query(
            "INSERT INTO task_artifacts (task_id, step_name, payload_json, updated_at) \
             VALUES (?, ?, ?, ?) ON CONFLICT(task_id, step_name) DO UPDATE SET \
             payload_json=excluded.payload_json, updated_at=excluded.updated_at",
        )
        .bind(task_id)
        .bind(step_name)
        .bind(payload_json)
        .bind(now_ms())
        .execute(&self.pool)
        .await
        .map_err(|e| format!("save artifact: {e}"))?;
        Ok(())
    }

    // ── Domain-specific pipeline resume tables ─────────────────────────

    // Step 1: ASR transcripts

    pub async fn load_asr_transcripts(
        &self,
        task_id: &str,
    ) -> Result<Vec<crate::services::pipeline::AsrTranscriptRow>, String> {
        let rows = sqlx::query(
            "SELECT segment_index, text FROM asr_transcripts \
             WHERE task_id = ? ORDER BY segment_index",
        )
        .bind(task_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| format!("load asr transcripts: {e}"))?;

        Ok(rows
            .into_iter()
            .map(|r| crate::services::pipeline::AsrTranscriptRow {
                segment_index: r.get::<i64, _>("segment_index") as usize,
                text: r.get("text"),
            })
            .collect())
    }

    pub async fn save_asr_transcript(
        &self,
        task_id: &str,
        row: &crate::services::pipeline::AsrTranscriptRow,
    ) -> Result<(), String> {
        sqlx::query(
            "INSERT INTO asr_transcripts (task_id, segment_index, text) \
             VALUES (?, ?, ?) ON CONFLICT(task_id, segment_index) DO UPDATE SET \
             text=excluded.text",
        )
        .bind(task_id)
        .bind(row.segment_index as i64)
        .bind(&row.text)
        .execute(&self.pool)
        .await
        .map_err(|e| format!("save asr transcript: {e}"))?;
        Ok(())
    }

    // Step 1 alignment: Cached ForcedAlignResult items per segment.

    pub async fn load_alignment_results(
        &self,
        task_id: &str,
    ) -> Result<Vec<crate::services::pipeline::AlignmentResultRow>, String> {
        let rows = sqlx::query(
            "SELECT segment_index, result_json FROM alignment_results \
             WHERE task_id = ? ORDER BY segment_index",
        )
        .bind(task_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| format!("load alignment results: {e}"))?;

        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            let json: String = r.get("result_json");
            let items: Vec<qwen_forced_aligner_rs::ForcedAlignItem> = serde_json::from_str(&json)
                .map_err(|e| format!("deserialize alignment result: {e}"))?;
            out.push(crate::services::pipeline::AlignmentResultRow {
                segment_index: r.get::<i64, _>("segment_index") as usize,
                items,
            });
        }
        Ok(out)
    }

    pub async fn save_alignment_result(
        &self,
        task_id: &str,
        row: &crate::services::pipeline::AlignmentResultRow,
    ) -> Result<(), String> {
        let json = serde_json::to_string(&row.items)
            .map_err(|e| format!("serialize alignment result: {e}"))?;
        sqlx::query(
            "INSERT INTO alignment_results (task_id, segment_index, result_json) \
             VALUES (?, ?, ?) ON CONFLICT(task_id, segment_index) DO UPDATE SET \
             result_json=excluded.result_json",
        )
        .bind(task_id)
        .bind(row.segment_index as i64)
        .bind(&json)
        .execute(&self.pool)
        .await
        .map_err(|e| format!("save alignment result: {e}"))?;
        Ok(())
    }

    // Step 4: Translation batch results

    pub async fn load_translation_batches(
        &self,
        task_id: &str,
    ) -> Result<Vec<crate::services::pipeline::TranslationBatchRow>, String> {
        let rows = sqlx::query(
            "SELECT batch_index, segment_translations FROM translation_batch_results \
             WHERE task_id = ? ORDER BY batch_index",
        )
        .bind(task_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| format!("load translation batches: {e}"))?;

        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            let json: String = r.get("segment_translations");
            let segment_translations: std::collections::HashMap<usize, String> =
                serde_json::from_str(&json)
                    .map_err(|e| format!("deserialize translation batch: {e}"))?;
            out.push(crate::services::pipeline::TranslationBatchRow {
                batch_index: r.get::<i64, _>("batch_index") as usize,
                segment_translations,
            });
        }
        Ok(out)
    }

    pub async fn save_translation_batch(
        &self,
        task_id: &str,
        row: &crate::services::pipeline::TranslationBatchRow,
    ) -> Result<(), String> {
        let json = serde_json::to_string(&row.segment_translations)
            .map_err(|e| format!("serialize translation batch: {e}"))?;
        sqlx::query(
            "INSERT INTO translation_batch_results \
             (task_id, batch_index, segment_translations) \
             VALUES (?, ?, ?) ON CONFLICT(task_id, batch_index) DO UPDATE SET \
             segment_translations=excluded.segment_translations",
        )
        .bind(task_id)
        .bind(row.batch_index as i64)
        .bind(&json)
        .execute(&self.pool)
        .await
        .map_err(|e| format!("save translation batch: {e}"))?;
        Ok(())
    }

    // Step 5 combined: Split + align per segment

    pub async fn load_step5_split_aligns(
        &self,
        task_id: &str,
    ) -> Result<Vec<crate::services::pipeline::Step5SplitAlignRow>, String> {
        let rows = sqlx::query(
            "SELECT segment_index, parent_json FROM step5_split_align_results \
             WHERE task_id = ? ORDER BY segment_index",
        )
        .bind(task_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| format!("load step5 split aligns: {e}"))?;

        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            out.push(crate::services::pipeline::Step5SplitAlignRow {
                segment_index: r.get::<i64, _>("segment_index") as usize,
                parent_json: r.get("parent_json"),
            });
        }
        Ok(out)
    }

    pub async fn save_step5_split_align(
        &self,
        task_id: &str,
        row: &crate::services::pipeline::Step5SplitAlignRow,
    ) -> Result<(), String> {
        sqlx::query(
            "INSERT INTO step5_split_align_results \
             (task_id, segment_index, parent_json) \
             VALUES (?, ?, ?) ON CONFLICT(task_id, segment_index) DO UPDATE SET \
             parent_json=excluded.parent_json",
        )
        .bind(task_id)
        .bind(row.segment_index as i64)
        .bind(&row.parent_json)
        .execute(&self.pool)
        .await
        .map_err(|e| format!("save step5 split align: {e}"))?;
        Ok(())
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
    use crate::commands::workspace::{WorkspaceTaskProgressState, WorkspaceTaskStageState};
    use crate::services::preferences_types::{
        SavedSettings, SubtitleRenderStyle, TerminologyGroup, TerminologyTerm,
    };
    use crate::services::workspace_subtitle::{WorkspaceSubtitleSegment, WorkspaceSubtitleWord};

    async fn store() -> TaskStore {
        let pool = super::test_pool_with_migrations().await;
        TaskStore::new(pool)
    }

    /// Insert a minimal task row to satisfy FK for subtitle_segments. Task CRUD
    /// (and a real `upsert_task` helper) lands in Task 10; this is just enough
    /// for segments/words tests to exercise CASCADE.
    async fn insert_blank_task(s: &TaskStore, id: &str) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        sqlx::query(
            "INSERT INTO tasks (id, media_path, name, media_kind, size_bytes, source_lang, \
             target_lang, transcribe_status, updated_at) \
             VALUES (?, '', ?, '', 0, '', '', '', ?)",
        )
        .bind(id)
        .bind(id)
        .bind(now)
        .execute(s.pool())
        .await
        .expect("insert blank task");
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

    fn sample_segments() -> Vec<WorkspaceSubtitleSegment> {
        vec![
            WorkspaceSubtitleSegment {
                start_ms: 0,
                end_ms: 1000,
                source_text: "hello".into(),
                translated_text: "你好".into(),
                source_words: vec![WorkspaceSubtitleWord {
                    start_ms: 0,
                    end_ms: 500,
                    word: "hello".into(),
                }],
            },
            WorkspaceSubtitleSegment {
                start_ms: 1000,
                end_ms: 2000,
                source_text: "world".into(),
                translated_text: "世界".into(),
                source_words: vec![],
            },
        ]
    }

    #[tokio::test]
    async fn segments_replace_and_load_roundtrip() {
        let s = store().await;
        insert_blank_task(&s, "t1").await;
        s.replace_segments("t1", &sample_segments()).await.unwrap();

        let loaded = s.load_segments("t1").await.unwrap();
        assert_eq!(loaded.len(), 2);
        // Segment 0 has words.
        assert_eq!(loaded[0].start_ms, 0);
        assert_eq!(loaded[0].end_ms, 1000);
        assert_eq!(loaded[0].source_text, "hello");
        assert_eq!(loaded[0].translated_text, "你好");
        assert_eq!(loaded[0].source_words.len(), 1);
        assert_eq!(loaded[0].source_words[0].word, "hello");
        assert_eq!(loaded[0].source_words[0].start_ms, 0);
        assert_eq!(loaded[0].source_words[0].end_ms, 500);
        // Segment 1 has no words.
        assert_eq!(loaded[1].source_text, "world");
        assert_eq!(loaded[1].translated_text, "世界");
        assert!(loaded[1].source_words.is_empty());

        // Replace with a shorter list — verifies CASCADE: old segment 1 and
        // segment 0's words are deleted, then fresh inserts succeed.
        let replacement = vec![WorkspaceSubtitleSegment {
            start_ms: 5000,
            end_ms: 6000,
            source_text: "again".into(),
            translated_text: "再次".into(),
            source_words: vec![],
        }];
        s.replace_segments("t1", &replacement).await.unwrap();

        let loaded2 = s.load_segments("t1").await.unwrap();
        assert_eq!(loaded2.len(), 1);
        assert_eq!(loaded2[0].source_text, "again");
        assert!(loaded2[0].source_words.is_empty());

        // No orphan words from the previous "hello" segment remain.
        let word_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM subtitle_words")
            .fetch_one(s.pool())
            .await
            .unwrap();
        assert_eq!(word_count, 0);
    }

    fn sample_task(id: &str) -> WorkspaceQueueItem {
        WorkspaceQueueItem {
            id: id.into(),
            path: "/tmp/a.mp3".into(),
            name: "a.mp3".into(),
            media_kind: "audio".into(),
            size_bytes: 1024,
            source_lang: "en".into(),
            target_lang: "zh-CN".into(),
            transcribe_status: "processing".into(),
            task_progress: WorkspaceTaskProgressState {
                stage: WorkspaceTaskStageState {
                    code: "recognizing".into(),
                    label: "语音识别中".into(),
                    order: 30,
                    detail: "".into(),
                    current: 0,
                    total: 0,
                },
            },
            transcribe_error: "".into(),
            result_text: "".into(),
            result_srt: "".into(),
            subtitle_segments_json: "[]".into(),
            llm_total_tokens: 0,
        }
    }

    #[tokio::test]
    async fn task_upsert_and_load_roundtrip() {
        let s = store().await;
        let original = sample_task("task-1");
        let extras = TaskMetaExtras {
            intent: "TRANSCRIBE_TRANSLATE".to_string(),
            max_retries: 2,
            subtitle_length_preset: "long".to_string(),
            enable_terminology: false,
            enable_subtitle_beautify: false,
            terminology_groups_json: r#"[{"id":"g","name":"x","terms":[]}]"#.to_string(),
        };
        s.upsert_task(&original, &extras).await.expect("upsert");

        let loaded = s.load_all_tasks().await.expect("load");
        assert_eq!(loaded.len(), 1);
        let (item, extras) = &loaded[0];
        assert_eq!(item.id, "task-1");
        assert_eq!(item.transcribe_status, "processing");
        assert_eq!(item.task_progress.stage.code, "recognizing");
        assert_eq!(extras.intent, "TRANSCRIBE_TRANSLATE");
        assert_eq!(extras.max_retries, 2);
        assert_eq!(extras.subtitle_length_preset, "long");
        assert!(!extras.enable_terminology);
        assert!(!extras.enable_subtitle_beautify);
        assert_eq!(
            extras.terminology_groups_json,
            r#"[{"id":"g","name":"x","terms":[]}]"#
        );
    }

    #[tokio::test]
    async fn mark_orphan_processing_as_error_recovers_residuals() {
        let s = store().await;
        let extras = TaskMetaExtras::default();
        s.upsert_task(&sample_task("a"), &extras).await.unwrap();
        s.upsert_task(&sample_task("b"), &extras).await.unwrap();

        let n = s.mark_orphan_processing_as_error().await.unwrap();
        assert_eq!(n, 2);

        let loaded = s.load_all_tasks().await.unwrap();
        for (item, _) in &loaded {
            assert_eq!(item.transcribe_status, "error");
            assert!(!item.transcribe_error.is_empty());
        }
    }

    #[tokio::test]
    async fn delete_task_removes_row() {
        let s = store().await;
        s.upsert_task(&sample_task("a"), &TaskMetaExtras::default())
            .await
            .unwrap();
        s.delete_task("a").await.unwrap();
        assert!(s.load_all_tasks().await.unwrap().is_empty());
    }
}
