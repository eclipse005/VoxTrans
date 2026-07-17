//! Centralized SQL operations against the voxtrans SQLite pool.
//!
//! Row reads use `sqlx::query_as::<_, RowStruct>` so column→field mapping
//! is handled by `#[derive(sqlx::FromRow)]` in `db::models`. Writes still
//! spell out columns explicitly because UPSERT needs the column list in
//! two spots (`VALUES (...)` and `DO UPDATE SET ...`).

use sqlx::SqlitePool;

use crate::commands::workspace::WorkspaceQueueItem;
use crate::db::conversion::{
    TaskMetaExtras, row_from_segment, row_from_settings, row_from_task, settings_from_row,
    task_from_row,
};
use crate::db::models::{SettingsRow, SubtitleSegmentRow, SubtitleWordRow, TaskRow};
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
        let row: Option<SettingsRow> = sqlx::query_as(
            "SELECT provider, chunk_target_seconds, subtitle_length_preset, asr_model, \
             align_model, demucs_model, enable_vocal_separation, translate_api_key, \
             translate_base_url, translate_model, llm_profiles_json, active_llm_profile_id, \
             llm_concurrency, active_terminology_group_id, \
             enable_subtitle_beautify, enable_click_sound, auto_burn_hard_subtitle, \
             subtitle_burn_mode, subtitle_render_style_json, flat_srt_output, \
             enable_vision_assist, locale, models_dir, default_review_source, \
             default_review_target, updated_at \
             FROM settings WHERE id = 1",
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| format!("load settings: {e}"))?;

        let mut settings = match row {
            None => default_settings(),
            Some(row) => settings_from_row(row),
        };

        // Compose flat_srt_items.
        let items: Vec<String> = sqlx::query_scalar("SELECT value FROM flat_srt_items ORDER BY id")
            .fetch_all(&self.pool)
            .await
            .map_err(|e| format!("load flat_srt_items: {e}"))?;
        settings.flat_srt_items = items
            .iter()
            .map(|s| crate::services::preferences_types::SubtitleBurnMode::parse(s))
            .collect();

        // Compose terminology_groups + terms.
        settings.terminology_groups = self.load_terminology_groups_internal().await?;

        Ok(settings)
    }

    /// Persist settings: UPSERT row + replace flat_srt_items + replace terminology.
    pub async fn save_settings(&self, settings: &SavedSettings) -> Result<(), String> {
        let mut tx = self.pool.begin().await.map_err(|e| format!("begin tx: {e}"))?;

        let row = row_from_settings(settings);
        let render_style_json = serde_json::to_string(&row.subtitle_render_style)
            .map_err(|e| format!("serialize subtitle_render_style: {e}"))?;
        sqlx::query(
            "INSERT INTO settings (id, provider, chunk_target_seconds, subtitle_length_preset, \
             asr_model, align_model, demucs_model, enable_vocal_separation, translate_api_key, \
             translate_base_url, translate_model, llm_profiles_json, active_llm_profile_id, \
             llm_concurrency, active_terminology_group_id, \
             enable_subtitle_beautify, enable_click_sound, auto_burn_hard_subtitle, \
             subtitle_burn_mode, subtitle_render_style_json, flat_srt_output, \
             enable_vision_assist, locale, models_dir, default_review_source, \
             default_review_target, updated_at) \
             VALUES (1, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(id) DO UPDATE SET \
             provider=excluded.provider, chunk_target_seconds=excluded.chunk_target_seconds, \
             subtitle_length_preset=excluded.subtitle_length_preset, asr_model=excluded.asr_model, \
             align_model=excluded.align_model, demucs_model=excluded.demucs_model, \
             enable_vocal_separation=excluded.enable_vocal_separation, \
             translate_api_key=excluded.translate_api_key, \
             translate_base_url=excluded.translate_base_url, \
             translate_model=excluded.translate_model, \
             llm_profiles_json=excluded.llm_profiles_json, \
             active_llm_profile_id=excluded.active_llm_profile_id, \
             llm_concurrency=excluded.llm_concurrency, \
             active_terminology_group_id=excluded.active_terminology_group_id, \
             enable_subtitle_beautify=excluded.enable_subtitle_beautify, \
             enable_click_sound=excluded.enable_click_sound, \
             auto_burn_hard_subtitle=excluded.auto_burn_hard_subtitle, \
             subtitle_burn_mode=excluded.subtitle_burn_mode, \
             subtitle_render_style_json=excluded.subtitle_render_style_json, \
             flat_srt_output=excluded.flat_srt_output, \
             enable_vision_assist=excluded.enable_vision_assist, \
             locale=excluded.locale, \
             models_dir=excluded.models_dir, \
             default_review_source=excluded.default_review_source, \
             default_review_target=excluded.default_review_target, \
             updated_at=excluded.updated_at",
        )
        .bind(&row.provider)
        .bind(row.chunk_target_seconds)
        .bind(&row.subtitle_length_preset)
        .bind(&row.asr_model)
        .bind(&row.align_model)
        .bind(&row.demucs_model)
        .bind(row.enable_vocal_separation)
        .bind(&row.translate_api_key)
        .bind(&row.translate_base_url)
        .bind(&row.translate_model)
        .bind(&row.llm_profiles_json)
        .bind(&row.active_llm_profile_id)
        .bind(row.llm_concurrency)
        .bind(&row.active_terminology_group_id)
        .bind(row.enable_subtitle_beautify)
        .bind(row.enable_click_sound)
        .bind(row.auto_burn_hard_subtitle)
        .bind(&row.subtitle_burn_mode)
        .bind(&render_style_json)
        .bind(row.flat_srt_output)
        .bind(row.enable_vision_assist)
        .bind(&row.locale)
        .bind(&row.models_dir)
        .bind(row.default_review_source)
        .bind(row.default_review_target)
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
                .bind(value.as_str())
                .bind(row.updated_at)
                .execute(&mut *tx)
                .await
                .map_err(|e| format!("insert flat_srt_item: {e}"))?;
        }

        // Replace terminology_groups (CASCADE deletes terms).
        // NB: this writes the LIVE terminology library. The frozen-at-enqueue
        // snapshot lives in tasks.terminology_groups_json and must NEVER be
        // touched here -- see terminology frozen contract doc on save_settings
        // and the matching invariant test in this module.
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
        use crate::db::models::{TerminologyGroupRow, TerminologyTermRow};
        use crate::services::preferences_types::{TerminologyGroup, TerminologyTerm};
        use std::collections::HashMap;

        let group_rows: Vec<TerminologyGroupRow> =
            sqlx::query_as("SELECT id, name, updated_at FROM terminology_groups")
                .fetch_all(&self.pool)
                .await
                .map_err(|e| format!("load terminology_groups: {e}"))?;

        let term_rows: Vec<TerminologyTermRow> = sqlx::query_as(
            "SELECT id, group_id, origin, target, note, updated_at FROM terminology_terms",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| format!("load terminology_terms: {e}"))?;

        let mut terms_by_group: HashMap<String, Vec<TerminologyTerm>> = HashMap::new();
        for row in term_rows {
            terms_by_group
                .entry(row.group_id)
                .or_default()
                .push(TerminologyTerm {
                    id: row.id,
                    origin: row.origin,
                    target: row.target,
                    note: row.note,
                });
        }

        let mut groups: Vec<TerminologyGroup> = group_rows
            .into_iter()
            .map(|row| {
                let terms = terms_by_group.remove(&row.id).unwrap_or_default();
                TerminologyGroup {
                    id: row.id,
                    name: row.name,
                    terms,
                }
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
        use std::collections::HashMap;

        let seg_rows: Vec<SubtitleSegmentRow> = sqlx::query_as(
            "SELECT id, task_id, idx, start_ms, end_ms, source_text, translated_text, updated_at \
             FROM subtitle_segments WHERE task_id = ? ORDER BY idx",
        )
        .bind(task_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| format!("load segments: {e}"))?;

        let word_rows: Vec<SubtitleWordRow> = sqlx::query_as(
            "SELECT w.id, w.segment_id, w.idx, w.start_ms, w.end_ms, w.word, w.updated_at \
             FROM subtitle_words w JOIN subtitle_segments s ON w.segment_id = s.id \
             WHERE s.task_id = ? ORDER BY w.segment_id, w.idx",
        )
        .bind(task_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| format!("load words: {e}"))?;

        let mut words_by_seg: HashMap<
            String,
            Vec<crate::services::workspace_subtitle::WorkspaceSubtitleWord>,
        > = HashMap::new();
        for w in word_rows {
            words_by_seg.entry(w.segment_id).or_default().push(
                crate::services::workspace_subtitle::WorkspaceSubtitleWord {
                    start_ms: w.start_ms,
                    end_ms: w.end_ms,
                    word: w.word,
                },
            );
        }

        let out = seg_rows
            .into_iter()
            .map(|s| crate::services::workspace_subtitle::WorkspaceSubtitleSegment {
                start_ms: s.start_ms,
                end_ms: s.end_ms,
                source_text: s.source_text,
                translated_text: s.translated_text,
                source_words: words_by_seg.remove(&s.id).unwrap_or_default(),
            })
            .collect();
        Ok(out)
    }

    // ---- tasks ----

    pub async fn load_all_tasks(&self) -> Result<Vec<(WorkspaceQueueItem, TaskMetaExtras)>, String> {
        let rows: Vec<TaskRow> = sqlx::query_as(
            "SELECT id, media_path, name, media_kind, size_bytes, source_lang, target_lang, \
             transcribe_status, task_progress_stage_code, task_progress_stage_label, \
             task_progress_stage_order, task_progress_detail, task_progress_current, \
             task_progress_total, transcribe_error, result_text, result_srt, llm_total_tokens, \
             intent, max_retries, subtitle_length_preset, \
             enable_subtitle_beautify, terminology_groups_json, terminology_group_id, \
             review_source, review_target, resume_from, enqueue_seq, updated_at \
             FROM tasks ORDER BY enqueue_seq ASC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| format!("load tasks: {e}"))?;

        Ok(rows.into_iter().map(task_from_row).collect())
    }

    /// Upsert a task row. `llm_total_tokens` and `enqueue_seq` are INSERT-only;
    /// on conflict the existing values are preserved. `llm_total_tokens` is
    /// mutated via `update_task_tokens`; `enqueue_seq` is never updated after
    /// initial enqueue (see `next_enqueue_seq`).
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
             enable_subtitle_beautify, terminology_groups_json, terminology_group_id, \
             review_source, review_target, resume_from, enqueue_seq, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, \
             ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) ON CONFLICT(id) DO UPDATE SET \
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
             result_srt=excluded.result_srt, \
             intent=excluded.intent, max_retries=excluded.max_retries, \
             subtitle_length_preset=excluded.subtitle_length_preset, \
             enable_subtitle_beautify=excluded.enable_subtitle_beautify, \
             terminology_groups_json=excluded.terminology_groups_json, \
             terminology_group_id=excluded.terminology_group_id, \
             review_source=excluded.review_source, \
             review_target=excluded.review_target, \
             resume_from=excluded.resume_from, \
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
        .bind(row.task_progress_stage_order)
        .bind(&row.task_progress_detail)
        .bind(row.task_progress_current)
        .bind(row.task_progress_total)
        .bind(&row.transcribe_error)
        .bind(&row.result_text)
        .bind(&row.result_srt)
        .bind(row.llm_total_tokens as i64)
        .bind(&row.intent)
        .bind(row.max_retries)
        .bind(&row.subtitle_length_preset)
        .bind(row.enable_subtitle_beautify)
        .bind(&row.terminology_groups_json)
        .bind(&row.terminology_group_id)
        .bind(row.review_source)
        .bind(row.review_target)
        .bind(&row.resume_from)
        .bind(row.enqueue_seq)
        .bind(row.updated_at)
        .execute(&self.pool)
        .await
        .map_err(|e| format!("upsert task: {e}"))?;
        Ok(())
    }

    /// Move a task to the head of the run order (smaller `enqueue_seq` first).
    /// Used when a review-paused task continues translation and must stay current.
    pub async fn prioritize_task_enqueue(&self, task_id: &str) -> Result<(), String> {
        let min_seq: Option<i64> = sqlx::query_scalar("SELECT MIN(enqueue_seq) FROM tasks")
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| format!("min enqueue seq: {e}"))?;
        let head = min_seq.unwrap_or(0) - 1;
        sqlx::query("UPDATE tasks SET enqueue_seq = ?, updated_at = ? WHERE id = ?")
            .bind(head)
            .bind(now_ms())
            .bind(task_id)
            .execute(&self.pool)
            .await
            .map_err(|e| format!("prioritize task enqueue: {e}"))?;
        Ok(())
    }

    /// 原子取号：返回单调递增的 `enqueue_seq`。
    /// 仅在入队新建任务时调用；后续 upsert 不会更新该列。
    ///
    /// 若调用方在取号后 upsert 失败，seq 仍会递增（跳号不影响排序单调性，
    /// 仅是审美问题）。
    pub async fn next_enqueue_seq(&self) -> Result<i64, String> {
        let row: (i64,) = sqlx::query_as(
            "INSERT INTO task_seq_counter (id, next_seq) VALUES (1, 1) \
             ON CONFLICT(id) DO UPDATE SET next_seq = next_seq + 1 \
             RETURNING next_seq - 1",
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|e| format!("next enqueue seq: {e}"))?;
        Ok(row.0)
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

    /// DB-first read for the running LLM token total. Returns 0 if the
    /// row doesn't exist (e.g. task was deleted concurrently).
    pub async fn get_task_total_tokens(&self, id: &str) -> Result<u64, String> {
        let total: Option<i64> =
            sqlx::query_scalar("SELECT llm_total_tokens FROM tasks WHERE id = ?")
                .bind(id)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| format!("get task tokens: {e}"))?;
        Ok(total.unwrap_or(0) as u64)
    }

    pub async fn delete_task(&self, id: &str) -> Result<(), String> {
        sqlx::query("DELETE FROM tasks WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| format!("delete task: {e}"))?;
        Ok(())
    }

    /// On startup, mark orphaned 'processing' tasks as 'error'.
    ///
    /// `processing` means a runner thread owns the task. After a restart the
    /// thread is gone, so any 'processing' task is an orphan — it was killed
    /// mid-run and its output may be incomplete. Mark it as 'error' so the
    /// user knows it needs to be re-run.
    ///
    /// 'queued' tasks are left untouched: they were never running, so they're
    /// not orphans. They simply wait for the user to start the queue again.
    pub async fn recover_orphan_processing(&self) -> Result<u64, String> {
        let n = sqlx::query(
            "UPDATE tasks SET transcribe_status = 'error', \
             transcribe_error = 'TASK_INTERRUPTED', \
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
        let payload: Option<String> = sqlx::query_scalar(
            "SELECT payload_json FROM task_artifacts WHERE task_id = ? AND step_name = ?",
        )
        .bind(task_id)
        .bind(step_name)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| format!("load artifact: {e}"))?;
        Ok(payload)
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

    /// Remove one pipeline step artifact (e.g. after source review edits).
    pub async fn delete_artifact(&self, task_id: &str, step_name: &str) -> Result<(), String> {
        sqlx::query("DELETE FROM task_artifacts WHERE task_id = ? AND step_name = ?")
            .bind(task_id)
            .bind(step_name)
            .execute(&self.pool)
            .await
            .map_err(|e| format!("delete artifact: {e}"))?;
        Ok(())
    }

    /// Clear all translation batch unit results for a task.
    pub async fn delete_translation_batches(&self, task_id: &str) -> Result<(), String> {
        sqlx::query("DELETE FROM translation_batch_results WHERE task_id = ?")
            .bind(task_id)
            .execute(&self.pool)
            .await
            .map_err(|e| format!("delete translation batches: {e}"))?;
        Ok(())
    }

    // ── Domain-specific pipeline resume tables ─────────────────────────

    // Step 1: ASR transcripts

    pub async fn load_asr_transcripts(
        &self,
        task_id: &str,
    ) -> Result<Vec<crate::services::pipeline::AsrTranscriptRow>, String> {
        let rows: Vec<AsrTranscriptInternalRow> = sqlx::query_as(
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
                segment_index: r.segment_index as usize,
                text: r.text,
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

    // Step 1 alignment: Cached AlignedSpan items per segment (engine-agnostic).
    // Payload is versioned by `align_model` so switching CTC ↔ Qwen never resumes
    // the other engine's spans (and skips untagged legacy rows once).

    pub async fn load_alignment_results(
        &self,
        task_id: &str,
        align_model: &str,
    ) -> Result<Vec<crate::services::pipeline::AlignmentResultRow>, String> {
        let rows: Vec<AlignmentInternalRow> = sqlx::query_as(
            "SELECT segment_index, result_json FROM alignment_results \
             WHERE task_id = ? ORDER BY segment_index",
        )
        .bind(task_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| format!("load alignment results: {e}"))?;

        let want = align_model.trim();
        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            match parse_alignment_cache_payload(&r.result_json) {
                Some((cached_model, items)) if cached_model == want && !items.is_empty() => {
                    out.push(crate::services::pipeline::AlignmentResultRow {
                        segment_index: r.segment_index as usize,
                        items,
                    });
                }
                _ => {
                    // Wrong engine, legacy bare array, or corrupt → recompute this segment.
                }
            }
        }
        Ok(out)
    }

    pub async fn save_alignment_result(
        &self,
        task_id: &str,
        align_model: &str,
        row: &crate::services::pipeline::AlignmentResultRow,
    ) -> Result<(), String> {
        let payload = AlignmentCachePayload {
            align_model: align_model.trim().to_string(),
            items: row.items.clone(),
        };
        let json = serde_json::to_string(&payload)
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
        let rows: Vec<TranslationBatchInternalRow> = sqlx::query_as(
            "SELECT batch_index, segment_translations FROM translation_batch_results \
             WHERE task_id = ? ORDER BY batch_index",
        )
        .bind(task_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| format!("load translation batches: {e}"))?;

        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            let segment_translations: std::collections::HashMap<usize, String> =
                serde_json::from_str(&r.segment_translations)
                    .map_err(|e| format!("deserialize translation batch: {e}"))?;
            out.push(crate::services::pipeline::TranslationBatchRow {
                batch_index: r.batch_index as usize,
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
}

// ── Internal step-checkpoint rows (private to this module) ────────────
//
// These mirror raw column shapes for the domain checkpoint tables.
// They're separate from `db::models` because the public `pipeline::*`
// row types live in `domain/pipeline` and would create a circular dep
// if `db::models` had to know about them.

#[derive(sqlx::FromRow)]
struct AsrTranscriptInternalRow {
    segment_index: i64,
    text: String,
}

#[derive(sqlx::FromRow)]
struct AlignmentInternalRow {
    segment_index: i64,
    result_json: String,
}

/// Versioned alignment cache blob stored in `alignment_results.result_json`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct AlignmentCachePayload {
    align_model: String,
    items: Vec<crate::domain::AlignedSpan>,
}

/// Parse cache JSON. Returns `None` for legacy bare arrays or corrupt data
/// so callers recompute instead of mixing engines / old punct policies.
fn parse_alignment_cache_payload(
    raw: &str,
) -> Option<(String, Vec<crate::domain::AlignedSpan>)> {
    let value: serde_json::Value = serde_json::from_str(raw).ok()?;
    // Legacy: bare array of spans (pre multi-align) — never reuse for resume.
    if value.is_array() {
        return None;
    }
    let payload: AlignmentCachePayload = serde_json::from_value(value).ok()?;
    if payload.align_model.trim().is_empty() {
        return None;
    }
    Some((payload.align_model, payload.items))
}

#[cfg(test)]
mod alignment_cache_tests {
    use super::*;
    use crate::domain::AlignedSpan;

    #[test]
    fn rejects_legacy_bare_array() {
        let raw = r#"[{"text":"你","start":0.0,"end":0.1}]"#;
        assert!(parse_alignment_cache_payload(raw).is_none());
    }

    #[test]
    fn accepts_versioned_payload() {
        let payload = AlignmentCachePayload {
            align_model: "mms-300m-1130-forced-aligner".into(),
            items: vec![AlignedSpan::new("你", 0.0, 0.1)],
        };
        let raw = serde_json::to_string(&payload).unwrap();
        let (model, items) = parse_alignment_cache_payload(&raw).expect("ok");
        assert_eq!(model, "mms-300m-1130-forced-aligner");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].text, "你");
    }
}

#[derive(sqlx::FromRow)]
struct TranslationBatchInternalRow {
    batch_index: i64,
    segment_translations: String,
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
    sqlx::query(super::SCHEMA_SQL)
        .execute(&pool)
        .await
        .expect("apply schema");
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
            provider: crate::services::preferences_types::Provider::Cpu,
            chunk_target_seconds: 60,
            subtitle_length_preset: crate::services::preferences_types::SubtitleLengthPreset::Standard,
            asr_model: crate::services::preferences_types::AsrModel::Qwen3Asr06B,
            align_model: crate::services::preferences_types::AlignModel::Qwen3ForcedAligner06B,
            demucs_model: crate::services::preferences_types::DemucsModel::HtdemucsFt,
            enable_vocal_separation: true,
            translate_api_key: "k".into(),
            translate_base_url: "https://api.example.com".into(),
            translate_model: "gpt-4o".into(),
            llm_profiles: crate::services::preferences_normalize::default_llm_profiles(),
            active_llm_profile_id: "deepseek".into(),
            llm_concurrency: 4,
            terminology_groups: Vec::new(),
            active_terminology_group_id: String::new(),
            enable_subtitle_beautify: true,
            enable_click_sound: true,
            auto_burn_hard_subtitle: false,
            default_review_source: false,
            default_review_target: false,
            subtitle_burn_mode: crate::services::preferences_types::SubtitleBurnMode::BilingualSourceFirst,
            subtitle_render_style: SubtitleRenderStyle::default(),
            flat_srt_output: false,
            flat_srt_items: Vec::new(),
            enable_vision_assist: false,
            locale: crate::services::preferences_types::Locale::ZhCn,
            models_dir: None,
        };
        s.flat_srt_items = vec![crate::services::preferences_types::SubtitleBurnMode::Source, crate::services::preferences_types::SubtitleBurnMode::Target];
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
        assert_eq!(settings.provider, crate::services::preferences_types::Provider::Cpu);
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
            terminology_group_id: "".into(),
            review_source: false,
            review_target: false,
            resume_from: String::new(),
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
            enable_subtitle_beautify: false,
            terminology_groups_json: r#"[{"id":"g","name":"x","terms":[]}]"#.to_string(),
            enqueue_seq: 0,
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
        assert!(!extras.enable_subtitle_beautify);
        assert_eq!(
            extras.terminology_groups_json,
            r#"[{"id":"g","name":"x","terms":[]}]"#
        );
    }

    #[tokio::test]
    async fn recover_orphan_processing_marks_error() {
        let s = store().await;
        let extras = TaskMetaExtras::default();
        s.upsert_task(&sample_task("a"), &extras).await.unwrap();
        s.upsert_task(&sample_task("b"), &extras).await.unwrap();

        let n = s.recover_orphan_processing().await.unwrap();
        assert_eq!(n, 2);

        let loaded = s.load_all_tasks().await.unwrap();
        for (item, _) in &loaded {
            assert_eq!(item.transcribe_status, "error");
            assert!(!item.transcribe_error.is_empty());
        }
    }

    #[tokio::test]
    async fn next_enqueue_seq_is_monotonic() {
        let s = store().await;
        let a = s.next_enqueue_seq().await.unwrap();
        let b = s.next_enqueue_seq().await.unwrap();
        let c = s.next_enqueue_seq().await.unwrap();
        assert_eq!(a, 0);
        assert_eq!(b, 1);
        assert_eq!(c, 2);
    }

    #[tokio::test]
    async fn upsert_preserves_enqueue_seq_on_conflict() {
        let s = store().await;
        let mut item = sample_task("task-1");
        let extras = TaskMetaExtras {
            enqueue_seq: 5,
            ..TaskMetaExtras::default()
        };
        s.upsert_task(&item, &extras).await.unwrap();
        let first_updated_at: i64 = sqlx::query_scalar("SELECT updated_at FROM tasks WHERE id = ?")
            .bind("task-1")
            .fetch_one(s.pool())
            .await
            .unwrap();

        // 等待 5ms 保证 updated_at 一定变化（now_ms 精度为 ms）
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;

        // 再次 upsert 同 id，状态变更，但 enqueue_seq 传不同值也应被忽略。
        item.transcribe_status = "completed".into();
        let extras2 = TaskMetaExtras {
            enqueue_seq: 999, // 应被忽略
            ..TaskMetaExtras::default()
        };
        s.upsert_task(&item, &extras2).await.unwrap();

        let loaded = s.load_all_tasks().await.unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].1.enqueue_seq, 5);
        // updated_at 应刷新（语义为"最后修改时间"，虽不参与排序但应正常更新）
        let second_updated_at: i64 =
            sqlx::query_scalar("SELECT updated_at FROM tasks WHERE id = ?")
                .bind("task-1")
                .fetch_one(s.pool())
                .await
                .unwrap();
        assert!(second_updated_at > first_updated_at);
    }

    #[tokio::test]
    async fn load_all_tasks_orders_by_enqueue_seq_asc() {
        let s = store().await;
        // 故意乱序插入 seq=10、5、8
        for seq in [10, 5, 8] {
            let item = sample_task(&format!("task-{seq}"));
            let extras = TaskMetaExtras {
                enqueue_seq: seq as i64,
                ..TaskMetaExtras::default()
            };
            s.upsert_task(&item, &extras).await.unwrap();
        }
        let loaded = s.load_all_tasks().await.unwrap();
        let ids: Vec<&str> = loaded.iter().map(|(i, _)| i.id.as_str()).collect();
        assert_eq!(ids, vec!["task-5", "task-8", "task-10"]);
    }

    #[tokio::test]
    async fn recover_orphan_does_not_change_order() {
        let s = store().await;
        // A(seq=1, processing) 在前；B(seq=2, completed) 在后。
        let mut a = sample_task("a");
        a.transcribe_status = "processing".into();
        s.upsert_task(
            &a,
            &TaskMetaExtras {
                enqueue_seq: 1,
                ..TaskMetaExtras::default()
            },
        )
        .await
        .unwrap();

        let mut b = sample_task("b");
        b.transcribe_status = "completed".into();
        s.upsert_task(
            &b,
            &TaskMetaExtras {
                enqueue_seq: 2,
                ..TaskMetaExtras::default()
            },
        )
        .await
        .unwrap();

        s.recover_orphan_processing().await.unwrap();

        let loaded = s.load_all_tasks().await.unwrap();
        assert_eq!(loaded.len(), 2);
        // 顺序仍为 [a, b]，不会因 recover 刷新 updated_at 而翻转。
        assert_eq!(loaded[0].0.id, "a");
        assert_eq!(loaded[0].0.transcribe_status, "error");
        assert_eq!(loaded[1].0.id, "b");
        assert_eq!(loaded[1].0.transcribe_status, "completed");
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

    /// upsert_task must preserve llm_total_tokens when a row already exists.
    /// Token writes go through update_task_tokens; if upsert_task included
    /// the token column in its UPDATE SET it would stomp in-flight progress.
    #[tokio::test]
    async fn upsert_task_preserves_existing_tokens() {
        let s = store().await;
        // Initial insert (tokens default 0 from sample_task).
        s.upsert_task(&sample_task("t"), &TaskMetaExtras::default())
            .await
            .unwrap();
        // Token writer updates the running total.
        s.update_task_tokens("t", 12_345).await.unwrap();
        assert_eq!(s.get_task_total_tokens("t").await.unwrap(), 12_345);

        // Another upsert (e.g. progress patch) MUST NOT reset tokens to 0.
        let mut task = sample_task("t");
        task.transcribe_status = "done".into();
        // Note: WorkspaceQueueItem.llm_total_tokens is whatever upstream
        // observed, but the DB layer is the source of truth.
        task.llm_total_tokens = 0; // simulate stale in-memory value
        s.upsert_task(&task, &TaskMetaExtras::default())
            .await
            .unwrap();
        assert_eq!(
            s.get_task_total_tokens("t").await.unwrap(),
            12_345,
            "upsert_task stomped tokens — the UPDATE SET must omit llm_total_tokens"
        );
    }

    /// Terminology frozen contract:
    /// - `save_settings` writes the LIVE library (terminology_groups/_terms
    ///   tables) — it MUST NOT touch tasks.terminology_groups_json.
    /// - `upsert_task` writes the FROZEN snapshot
    ///   (tasks.terminology_groups_json) — it MUST NOT touch the live
    ///   library tables.
    /// Editing one side has zero effect on the other; this test pins the
    /// no-cross-write invariant by exercising both directions.
    #[tokio::test]
    async fn terminology_frozen_contract_no_cross_writes() {
        let s = store().await;

        // 1) Seed both sides with distinguishable values.
        let mut library = sample_settings();
        library.terminology_groups = vec![TerminologyGroup {
            id: "lib-grp".into(),
            name: "library-name".into(),
            terms: vec![TerminologyTerm {
                id: "lib-term".into(),
                origin: "alpha".into(),
                target: "甲".into(),
                note: "".into(),
            }],
        }];
        s.save_settings(&library).await.unwrap();

        let frozen_extras = TaskMetaExtras {
            intent: "TRANSCRIBE_TRANSLATE".into(),
            max_retries: 0,
            subtitle_length_preset: "default".into(),
            enable_subtitle_beautify: true,
            terminology_groups_json:
                r#"[{"id":"frozen-grp","name":"frozen-name","terms":[]}]"#.into(),
            enqueue_seq: 0,
        };
        s.upsert_task(&sample_task("t-frozen"), &frozen_extras)
            .await
            .unwrap();

        // 2) upsert_task did NOT mutate the live terminology library.
        let after_upsert = s.load_settings().await.unwrap();
        assert_eq!(after_upsert.terminology_groups.len(), 1);
        assert_eq!(after_upsert.terminology_groups[0].id, "lib-grp");
        assert_eq!(after_upsert.terminology_groups[0].name, "library-name");

        // 3) save_settings (re-saving the library with a NEW snapshot) did
        //    NOT mutate the task's frozen snapshot.
        let mut updated_library = library.clone();
        updated_library.terminology_groups = vec![TerminologyGroup {
            id: "lib-grp-v2".into(),
            name: "library-name-v2".into(),
            terms: vec![],
        }];
        s.save_settings(&updated_library).await.unwrap();

        let tasks = s.load_all_tasks().await.unwrap();
        let (_, frozen_extras_loaded) = tasks
            .iter()
            .find(|(item, _)| item.id == "t-frozen")
            .expect("task missing");
        assert_eq!(
            frozen_extras_loaded.terminology_groups_json,
            r#"[{"id":"frozen-grp","name":"frozen-name","terms":[]}]"#,
            "save_settings clobbered the frozen task snapshot — terminology contract violated"
        );
    }
}
