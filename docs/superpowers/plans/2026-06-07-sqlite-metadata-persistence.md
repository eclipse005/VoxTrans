# VoxTrans SQLite 元数据层持久化实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 `settings.json` 和 `task_meta.json` 切换到 SQLite 唯一真源，DB 接入 `voxtrans` crate main，建立项目级 SQLite 建表准则。破坏性升级，无迁移。

**Architecture:** 新增 `db::{store,models,conversion}` 三模块；store 集中所有 SQL、models 定义 row struct、conversion 负责 row ↔ 业务对象映射。`services/preferences.rs` 和 `commands/workspace/*` 改走 store 替换原 JSON 读写。`db/mod.rs` 当前未被 main 加载（孤儿代码），本版接入。

**Tech Stack:** Rust 1.83+, sqlx 0.8 (sqlite, runtime-tokio-rustls, macros, migrate), tauri 2.x, serde / serde_json, ts-rs（已存在）

---

## 重要：实施前置条件

- 必须在分支 `feat/sqlite-metadata-persistence` 上（`git branch` 确认）
- Rust 工具链就绪（`cargo --version` 正常）
- 当前 spec 已提交在 `1dc22aa docs: SQLite 元数据层持久化设计 spec`
- **本计划涉及 7 张表 + 5 个集成点，每个 task 一个 commit，commit message 用 conventional commits**

---

## Task 1: 接入 db 模块 + 写迁移 SQL

**Files:**
- Create: `src-tauri/migrations/20260607000001_init.sql`
- Modify: `src-tauri/src/main.rs:3-6`
- Modify: `src-tauri/src/db/mod.rs`

- [ ] **Step 1: 创建迁移目录并写 SQL**

创建 `src-tauri/migrations/20260607000001_init.sql`，包含 7 张表和 8 个索引：

```sql
-- settings: 单行配置（1:1 全平铺 SavedSettings）
CREATE TABLE settings (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    provider TEXT NOT NULL,
    chunk_target_seconds INTEGER NOT NULL,
    subtitle_length_preset TEXT NOT NULL,
    asr_model TEXT NOT NULL,
    align_model TEXT NOT NULL,
    demucs_model TEXT NOT NULL,
    enable_vocal_separation INTEGER NOT NULL,
    translate_api_key TEXT NOT NULL,
    translate_base_url TEXT NOT NULL,
    translate_model TEXT NOT NULL,
    llm_concurrency INTEGER NOT NULL,
    enable_terminology INTEGER NOT NULL,
    enable_subtitle_beautify INTEGER NOT NULL,
    enable_click_sound INTEGER NOT NULL,
    auto_burn_hard_subtitle INTEGER NOT NULL,
    subtitle_burn_mode TEXT NOT NULL,
    source_font_family TEXT NOT NULL,
    source_font_size INTEGER NOT NULL,
    source_primary_color TEXT NOT NULL,
    source_outline_color TEXT NOT NULL,
    source_back_color TEXT NOT NULL,
    source_outline REAL NOT NULL,
    source_shadow REAL NOT NULL,
    source_border_style TEXT NOT NULL,
    source_border_opacity INTEGER NOT NULL,
    target_font_family TEXT NOT NULL,
    target_font_size INTEGER NOT NULL,
    target_primary_color TEXT NOT NULL,
    target_outline_color TEXT NOT NULL,
    target_back_color TEXT NOT NULL,
    target_outline REAL NOT NULL,
    target_shadow REAL NOT NULL,
    target_border_style TEXT NOT NULL,
    target_border_opacity INTEGER NOT NULL,
    margin_v INTEGER NOT NULL,
    alignment INTEGER NOT NULL,
    bilingual_line_gap INTEGER NOT NULL,
    flat_srt_output INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

-- flat_srt_items: settings 的 1:N 子表
CREATE TABLE flat_srt_items (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    value TEXT NOT NULL UNIQUE,
    updated_at INTEGER NOT NULL
);

-- tasks: 1:1 全平铺 WorkspaceQueueItem + wrapper
CREATE TABLE tasks (
    id TEXT PRIMARY KEY,
    media_path TEXT NOT NULL,
    name TEXT NOT NULL,
    media_kind TEXT NOT NULL,
    size_bytes INTEGER NOT NULL,
    source_lang TEXT NOT NULL,
    target_lang TEXT NOT NULL,
    transcribe_status TEXT NOT NULL,
    task_progress_stage_code TEXT NOT NULL DEFAULT '',
    task_progress_stage_label TEXT NOT NULL DEFAULT '',
    task_progress_stage_order INTEGER NOT NULL DEFAULT 0,
    task_progress_detail TEXT NOT NULL DEFAULT '',
    task_progress_current INTEGER NOT NULL DEFAULT 0,
    task_progress_total INTEGER NOT NULL DEFAULT 0,
    transcribe_error TEXT NOT NULL DEFAULT '',
    result_text TEXT NOT NULL DEFAULT '',
    result_srt TEXT NOT NULL DEFAULT '',
    llm_total_tokens INTEGER NOT NULL DEFAULT 0,
    intent TEXT NOT NULL DEFAULT '',
    max_retries INTEGER NOT NULL DEFAULT 0,
    settings_snapshot_provider TEXT NOT NULL DEFAULT '',
    settings_snapshot_asr_model TEXT NOT NULL DEFAULT '',
    settings_snapshot_align_model TEXT NOT NULL DEFAULT '',
    settings_snapshot_demucs_model TEXT NOT NULL DEFAULT '',
    settings_snapshot_translate_api_key TEXT NOT NULL DEFAULT '',
    settings_snapshot_translate_base_url TEXT NOT NULL DEFAULT '',
    settings_snapshot_translate_model TEXT NOT NULL DEFAULT '',
    settings_snapshot_llm_concurrency INTEGER NOT NULL DEFAULT 0,
    settings_snapshot_chunk_target_seconds INTEGER NOT NULL DEFAULT 0,
    settings_snapshot_enable_vocal_separation INTEGER NOT NULL DEFAULT 0,
    updated_at INTEGER NOT NULL
);

CREATE INDEX idx_tasks_status ON tasks(transcribe_status);
CREATE INDEX idx_tasks_updated_at ON tasks(updated_at DESC);
CREATE INDEX idx_tasks_langs ON tasks(source_lang, target_lang);
CREATE INDEX idx_tasks_media_kind ON tasks(media_kind);

-- subtitle_segments: tasks 的 1:N
CREATE TABLE subtitle_segments (
    id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    idx INTEGER NOT NULL,
    start_ms INTEGER NOT NULL,
    end_ms INTEGER NOT NULL,
    source_text TEXT NOT NULL DEFAULT '',
    translated_text TEXT NOT NULL DEFAULT '',
    updated_at INTEGER NOT NULL
);

CREATE INDEX idx_segments_task_idx ON subtitle_segments(task_id, idx);

-- subtitle_words: segments 的 1:N（无豁免）
CREATE TABLE subtitle_words (
    id TEXT PRIMARY KEY,
    segment_id TEXT NOT NULL REFERENCES subtitle_segments(id) ON DELETE CASCADE,
    idx INTEGER NOT NULL,
    start_ms INTEGER NOT NULL,
    end_ms INTEGER NOT NULL,
    word TEXT NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX idx_words_segment_idx ON subtitle_words(segment_id, idx);

-- terminology_groups
CREATE TABLE terminology_groups (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    updated_at INTEGER NOT NULL
);

-- terminology_terms
CREATE TABLE terminology_terms (
    id TEXT PRIMARY KEY,
    group_id TEXT NOT NULL REFERENCES terminology_groups(id) ON DELETE CASCADE,
    origin TEXT NOT NULL,
    target TEXT NOT NULL,
    note TEXT NOT NULL DEFAULT '',
    updated_at INTEGER NOT NULL
);

CREATE INDEX idx_terms_group_id ON terminology_terms(group_id);
```

- [ ] **Step 2: 接入 db 模块到 main.rs**

修改 `src-tauri/src/main.rs:3-6`，在 `mod services;` 后加一行：

```rust
mod app_state;
mod commands;
mod db;
mod domain;
mod services;
```

- [ ] **Step 3: db/mod.rs 导出三个子模块**

修改 `src-tauri/src/db/mod.rs`，在文件开头添加子模块声明，并把现有 `init_pool` 标为 `pub`（保持原状），文件最终内容：

```rust
pub mod conversion;
pub mod models;
pub mod store;

use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use std::path::PathBuf;
use tauri::Manager;

pub async fn init_pool(app: &tauri::AppHandle) -> Result<SqlitePool, String> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("failed to resolve app data dir: {e}"))?;

    std::fs::create_dir_all(&app_data_dir)
        .map_err(|e| format!("failed to create app data dir {:?}: {e}", app_data_dir))?;

    let db_path = app_data_dir.join("voxtrans.db");
    let options = connect_options(db_path)?;

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await
        .map_err(|e| format!("failed to connect sqlite: {e}"))?;

    sqlx::query("PRAGMA foreign_keys = ON")
        .execute(&pool)
        .await
        .map_err(|e| format!("failed to enable foreign keys: {e}"))?;

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .map_err(|e| format!("failed to run sqlite migrations: {e}"))?;

    Ok(pool)
}

fn connect_options(path: PathBuf) -> Result<SqliteConnectOptions, String> {
    if path.as_os_str().is_empty() {
        return Err("sqlite path is empty".to_string());
    }

    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal);

    Ok(options)
}
```

- [ ] **Step 4: 创建三个子模块的空文件**

分别创建并写入：

`src-tauri/src/db/models.rs`：

```rust
// Row structs for the 7 tables. Filled in by later tasks.
```

`src-tauri/src/db/store.rs`：

```rust
// Centralized SQL operations. Filled in by later tasks.
```

`src-tauri/src/db/conversion.rs`：

```rust
// Row <-> business object conversion. Filled in by later tasks.
```

- [ ] **Step 5: 编译验证**

Run: `cd D:/voxtrans/src-tauri && cargo check 2>&1 | tail -20`

Expected: 编译通过（无 error）。warning 可以接受（dead_code 等属预期——这些模块暂时空）。

如果报错"找不到模块 db"，检查 `main.rs` 第 6 行 `mod db;` 是否已加。

- [ ] **Step 6: 跑迁移（在测试里临时验证）**

跳过——`cargo check` 通过 + 后续 task 会通过 store 调用验证迁移。本步只保证编译过。

- [ ] **Step 7: Commit**

```bash
cd D:/voxtrans && git add src-tauri/migrations/20260607000001_init.sql src-tauri/src/main.rs src-tauri/src/db/
git commit -m "feat(db): wire db module into main + add init migration"
```

---

## Task 2: models.rs — 7 个 row struct

**Files:**
- Modify: `src-tauri/src/db/models.rs`

- [ ] **Step 1: 写 models.rs**

替换 `src-tauri/src/db/models.rs` 内容为：

```rust
//! Row structs mirroring the 7 tables in migrations/20260607000001_init.sql.
//!
//! Each struct is `pub` and derives `Debug, Clone`. Field names match SQL
//! column names exactly. Conversions to/from business objects live in
//! `super::conversion`.

#[derive(Debug, Clone)]
pub struct SettingsRow {
    pub provider: String,
    pub chunk_target_seconds: u32,
    pub subtitle_length_preset: String,
    pub asr_model: String,
    pub align_model: String,
    pub demucs_model: String,
    pub enable_vocal_separation: bool,
    pub translate_api_key: String,
    pub translate_base_url: String,
    pub translate_model: String,
    pub llm_concurrency: u32,
    pub enable_terminology: bool,
    pub enable_subtitle_beautify: bool,
    pub enable_click_sound: bool,
    pub auto_burn_hard_subtitle: bool,
    pub subtitle_burn_mode: String,
    pub source_font_family: String,
    pub source_font_size: u32,
    pub source_primary_color: String,
    pub source_outline_color: String,
    pub source_back_color: String,
    pub source_outline: f64,
    pub source_shadow: f64,
    pub source_border_style: String,
    pub source_border_opacity: u8,
    pub target_font_family: String,
    pub target_font_size: u32,
    pub target_primary_color: String,
    pub target_outline_color: String,
    pub target_back_color: String,
    pub target_outline: f64,
    pub target_shadow: f64,
    pub target_border_style: String,
    pub target_border_opacity: u8,
    pub margin_v: u32,
    pub alignment: u8,
    pub bilingual_line_gap: u32,
    pub flat_srt_output: bool,
    pub updated_at: i64,
}

#[derive(Debug, Clone)]
pub struct FlatSrtItemRow {
    pub id: i64,
    pub value: String,
    pub updated_at: i64,
}

#[derive(Debug, Clone)]
pub struct TaskRow {
    pub id: String,
    pub media_path: String,
    pub name: String,
    pub media_kind: String,
    pub size_bytes: u64,
    pub source_lang: String,
    pub target_lang: String,
    pub transcribe_status: String,
    pub task_progress_stage_code: String,
    pub task_progress_stage_label: String,
    pub task_progress_stage_order: u32,
    pub task_progress_detail: String,
    pub task_progress_current: u32,
    pub task_progress_total: u32,
    pub transcribe_error: String,
    pub result_text: String,
    pub result_srt: String,
    pub llm_total_tokens: u64,
    pub intent: String,
    pub max_retries: u32,
    pub settings_snapshot_provider: String,
    pub settings_snapshot_asr_model: String,
    pub settings_snapshot_align_model: String,
    pub settings_snapshot_demucs_model: String,
    pub settings_snapshot_translate_api_key: String,
    pub settings_snapshot_translate_base_url: String,
    pub settings_snapshot_translate_model: String,
    pub settings_snapshot_llm_concurrency: u32,
    pub settings_snapshot_chunk_target_seconds: u32,
    pub settings_snapshot_enable_vocal_separation: bool,
    pub updated_at: i64,
}

#[derive(Debug, Clone)]
pub struct SubtitleSegmentRow {
    pub id: String,
    pub task_id: String,
    pub idx: i32,
    pub start_ms: u64,
    pub end_ms: u64,
    pub source_text: String,
    pub translated_text: String,
    pub updated_at: i64,
}

#[derive(Debug, Clone)]
pub struct SubtitleWordRow {
    pub id: String,
    pub segment_id: String,
    pub idx: i32,
    pub start_ms: u64,
    pub end_ms: u64,
    pub word: String,
    pub updated_at: i64,
}

#[derive(Debug, Clone)]
pub struct TerminologyGroupRow {
    pub id: String,
    pub name: String,
    pub updated_at: i64,
}

#[derive(Debug, Clone)]
pub struct TerminologyTermRow {
    pub id: String,
    pub group_id: String,
    pub origin: String,
    pub target: String,
    pub note: String,
    pub updated_at: i64,
}
```

- [ ] **Step 2: 编译**

Run: `cd D:/voxtrans/src-tauri && cargo check 2>&1 | tail -10`

Expected: 编译通过。

- [ ] **Step 3: Commit**

```bash
cd D:/voxtrans && git add src-tauri/src/db/models.rs
git commit -m "feat(db): add row struct models for 7 tables"
```

---

## Task 3: store.rs — TaskStore 骨架 + 内存测试模式

**Files:**
- Modify: `src-tauri/src/db/store.rs`

- [ ] **Step 1: 写 store.rs 骨架**

替换 `src-tauri/src/db/store.rs` 内容为：

```rust
//! Centralized SQL operations against the voxtrans SQLite pool.
//!
//! All persistence-side logic (CRUD on settings / tasks / segments / words /
//! terminology) lives here. The rest of the codebase calls into this module
//! rather than constructing SQL directly.

use sqlx::SqlitePool;

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
```

- [ ] **Step 2: 编译**

Run: `cd D:/voxtrans/src-tauri && cargo check 2>&1 | tail -10`

Expected: 编译通过（`migrations` 在 `#[cfg(test)]` 里才被引用——如果 `cargo check` 默认是 test 模式，没问题；如果是 build 模式，路径解析可能失败，看情况调整）。

如果 `cargo check` 不跑 test 编译，验证：`cd D:/voxtrans/src-tauri && cargo check --tests 2>&1 | tail -10` —— 应该通过。

- [ ] **Step 3: Commit**

```bash
cd D:/voxtrans && git add src-tauri/src/db/store.rs
git commit -m "feat(db): add TaskStore skeleton with test helpers"
```

---

## Task 4: conversion.rs — settings row ↔ SavedSettings

**Files:**
- Modify: `src-tauri/src/db/conversion.rs`

- [ ] **Step 1: 写失败测试**

在 `src-tauri/src/db/conversion.rs` 写：

```rust
//! Row <-> business object conversion.
//!
//! `from_business` and `to_business` are pure functions, tested in isolation.
//! They do not touch the database.

use crate::db::models::SettingsRow;
use crate::services::preferences_types::{
    SavedSettings, SubtitleLayoutStyle, SubtitleLineStyle, SubtitleRenderStyle,
};

pub fn settings_from_row(row: SettingsRow) -> SavedSettings {
    SavedSettings {
        provider: row.provider,
        chunk_target_seconds: row.chunk_target_seconds,
        subtitle_length_preset: row.subtitle_length_preset,
        asr_model: row.asr_model,
        align_model: row.align_model,
        demucs_model: row.demucs_model,
        enable_vocal_separation: row.enable_vocal_separation,
        translate_api_key: row.translate_api_key,
        translate_base_url: row.translate_base_url,
        translate_model: row.translate_model,
        llm_concurrency: row.llm_concurrency,
        // terminology_groups is composed in store.rs from terminology tables.
        terminology_groups: Vec::new(),
        enable_terminology: row.enable_terminology,
        enable_subtitle_beautify: row.enable_subtitle_beautify,
        enable_click_sound: row.enable_click_sound,
        auto_burn_hard_subtitle: row.auto_burn_hard_subtitle,
        subtitle_burn_mode: row.subtitle_burn_mode,
        subtitle_render_style: SubtitleRenderStyle {
            source: SubtitleLineStyle {
                font_family: row.source_font_family,
                font_size: row.source_font_size,
                primary_color: row.source_primary_color,
                outline_color: row.source_outline_color,
                back_color: row.source_back_color,
                outline: row.source_outline,
                shadow: row.source_shadow,
                border_style: row.source_border_style,
                border_opacity: row.source_border_opacity,
            },
            target: SubtitleLineStyle {
                font_family: row.target_font_family,
                font_size: row.target_font_size,
                primary_color: row.target_primary_color,
                outline_color: row.target_outline_color,
                back_color: row.target_back_color,
                outline: row.target_outline,
                shadow: row.target_shadow,
                border_style: row.target_border_style,
                border_opacity: row.target_border_opacity,
            },
            layout: SubtitleLayoutStyle {
                margin_v: row.margin_v,
                alignment: row.alignment,
                bilingual_line_gap: row.bilingual_line_gap,
            },
        },
        flat_srt_output: row.flat_srt_output,
        // flat_srt_items is composed in store.rs from the flat_srt_items table.
        flat_srt_items: Vec::new(),
    }
}

pub fn row_from_settings(settings: &SavedSettings) -> SettingsRow {
    SettingsRow {
        provider: settings.provider.clone(),
        chunk_target_seconds: settings.chunk_target_seconds,
        subtitle_length_preset: settings.subtitle_length_preset.clone(),
        asr_model: settings.asr_model.clone(),
        align_model: settings.align_model.clone(),
        demucs_model: settings.demucs_model.clone(),
        enable_vocal_separation: settings.enable_vocal_separation,
        translate_api_key: settings.translate_api_key.clone(),
        translate_base_url: settings.translate_base_url.clone(),
        translate_model: settings.translate_model.clone(),
        llm_concurrency: settings.llm_concurrency,
        enable_terminology: settings.enable_terminology,
        enable_subtitle_beautify: settings.enable_subtitle_beautify,
        enable_click_sound: settings.enable_click_sound,
        auto_burn_hard_subtitle: settings.auto_burn_hard_subtitle,
        subtitle_burn_mode: settings.subtitle_burn_mode.clone(),
        source_font_family: settings.subtitle_render_style.source.font_family.clone(),
        source_font_size: settings.subtitle_render_style.source.font_size,
        source_primary_color: settings.subtitle_render_style.source.primary_color.clone(),
        source_outline_color: settings.subtitle_render_style.source.outline_color.clone(),
        source_back_color: settings.subtitle_render_style.source.back_color.clone(),
        source_outline: settings.subtitle_render_style.source.outline,
        source_shadow: settings.subtitle_render_style.source.shadow,
        source_border_style: settings.subtitle_render_style.source.border_style.clone(),
        source_border_opacity: settings.subtitle_render_style.source.border_opacity,
        target_font_family: settings.subtitle_render_style.target.font_family.clone(),
        target_font_size: settings.subtitle_render_style.target.font_size,
        target_primary_color: settings.subtitle_render_style.target.primary_color.clone(),
        target_outline_color: settings.subtitle_render_style.target.outline_color.clone(),
        target_back_color: settings.subtitle_render_style.target.back_color.clone(),
        target_outline: settings.subtitle_render_style.target.outline,
        target_shadow: settings.subtitle_render_style.target.shadow,
        target_border_style: settings.subtitle_render_style.target.border_style.clone(),
        target_border_opacity: settings.subtitle_render_style.target.border_opacity,
        margin_v: settings.subtitle_render_style.layout.margin_v,
        alignment: settings.subtitle_render_style.layout.alignment,
        bilingual_line_gap: settings.subtitle_render_style.layout.bilingual_line_gap,
        flat_srt_output: settings.flat_srt_output,
        updated_at: now_ms(),
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_settings() -> SavedSettings {
        SavedSettings {
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
        }
    }

    #[test]
    fn settings_roundtrip_preserves_all_fields() {
        let original = sample_settings();
        let row = row_from_settings(&original);
        let restored = settings_from_row(row);

        assert_eq!(restored.provider, original.provider);
        assert_eq!(restored.chunk_target_seconds, original.chunk_target_seconds);
        assert_eq!(restored.subtitle_length_preset, original.subtitle_length_preset);
        assert_eq!(restored.asr_model, original.asr_model);
        assert_eq!(restored.align_model, original.align_model);
        assert_eq!(restored.demucs_model, original.demucs_model);
        assert_eq!(restored.enable_vocal_separation, original.enable_vocal_separation);
        assert_eq!(restored.translate_api_key, original.translate_api_key);
        assert_eq!(restored.translate_base_url, original.translate_base_url);
        assert_eq!(restored.translate_model, original.translate_model);
        assert_eq!(restored.llm_concurrency, original.llm_concurrency);
        assert_eq!(restored.enable_terminology, original.enable_terminology);
        assert_eq!(restored.enable_subtitle_beautify, original.enable_subtitle_beautify);
        assert_eq!(restored.enable_click_sound, original.enable_click_sound);
        assert_eq!(restored.auto_burn_hard_subtitle, original.auto_burn_hard_subtitle);
        assert_eq!(restored.subtitle_burn_mode, original.subtitle_burn_mode);
        assert_eq!(restored.flat_srt_output, original.flat_srt_output);

        // nested SubtitleRenderStyle.source
        assert_eq!(restored.subtitle_render_style.source.font_family, original.subtitle_render_style.source.font_family);
        assert_eq!(restored.subtitle_render_style.source.font_size, original.subtitle_render_style.source.font_size);
        assert_eq!(restored.subtitle_render_style.source.outline, original.subtitle_render_style.source.outline);

        // nested SubtitleRenderStyle.target
        assert_eq!(restored.subtitle_render_style.target.font_family, original.subtitle_render_style.target.font_family);

        // nested SubtitleRenderStyle.layout
        assert_eq!(restored.subtitle_render_style.layout.margin_v, original.subtitle_render_style.layout.margin_v);
        assert_eq!(restored.subtitle_render_style.layout.alignment, original.subtitle_render_style.layout.alignment);
        assert_eq!(restored.subtitle_render_style.layout.bilingual_line_gap, original.subtitle_render_style.layout.bilingual_line_gap);

        // composed fields are empty here (filled by store.rs)
        assert!(restored.terminology_groups.is_empty());
        assert!(restored.flat_srt_items.is_empty());
    }
}
```

- [ ] **Step 2: 跑测试**

Run: `cd D:/voxtrans/src-tauri && cargo test --lib db::conversion 2>&1 | tail -20`

Expected: 1 passed; 0 failed.

- [ ] **Step 3: Commit**

```bash
cd D:/voxtrans && git add src-tauri/src/db/conversion.rs
git commit -m "feat(db): settings row <-> SavedSettings conversion"
```

---

## Task 5: store.rs — settings CRUD

**Files:**
- Modify: `src-tauri/src/db/store.rs`

- [ ] **Step 1: 写失败测试**

在 `src-tauri/src/db/store.rs` 的 `#[cfg(test)]` 模块之前，添加 settings CRUD 公开方法和对应测试。**完整替换** `src-tauri/src/db/store.rs`：

```rust
//! Centralized SQL operations against the voxtrans SQLite pool.

use sqlx::{Row, SqlitePool};

use crate::db::conversion::{row_from_settings, settings_from_row};
use crate::db::models::{FlatSrtItemRow, SettingsRow};
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
        // Implemented in Task 8.
        Ok(Vec::new())
    }
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
```

- [ ] **Step 2: 编译 + 跑测试**

Run: `cd D:/voxtrans/src-tauri && cargo test --lib db::store 2>&1 | tail -30`

Expected: 2 passed. 可能 warning 提示 `load_terminology_groups_internal` 未使用——正常，下个 task 补。

如果 `tokio::test` 失败（runtime 缺失），检查 `Cargo.toml` 里 `tokio` 是否有 `macros` 特性。当前 `Cargo.toml` 只有 `["rt", "sync", "time"]`，需要加 `macros` —— 见 Step 2.5。

- [ ] **Step 2.5: 加 tokio macros 特性（如需要）**

如果测试编译报"cannot find macro `tokio::test`"，修改 `src-tauri/Cargo.toml:25`：

```toml
tokio = { version = "1", features = ["rt", "sync", "time", "macros", "rt-multi-thread"] }
```

- [ ] **Step 3: Commit**

```bash
cd D:/voxtrans && git add src-tauri/src/db/store.rs src-tauri/Cargo.toml
git commit -m "feat(db): settings + flat_srt_items + terminology CRUD"
```

---

## Task 6: services/preferences.rs — 切换到 store

**Files:**
- Modify: `src-tauri/src/services/preferences.rs`

- [ ] **Step 1: 改 preferences.rs 走 store**

替换 `src-tauri/src/services/preferences.rs`：

```rust
use tauri::{AppHandle, Manager};

pub use super::preferences_types::{
    SaveAppSettingsRequest, SavedSettings, SubtitleLayoutStyle, SubtitleLineStyle,
    SubtitleRenderStyle, TerminologyGroup, TerminologyTerm, UserPreferencesResponse,
};

pub async fn load_user_preferences(
    store: &crate::db::store::TaskStore,
) -> Result<UserPreferencesResponse, String> {
    let settings = store.load_settings().await?;
    Ok(UserPreferencesResponse { settings })
}

pub async fn save_app_settings(
    store: &crate::db::store::TaskStore,
    request: &SaveAppSettingsRequest,
) -> Result<(), String> {
    let normalized = super::preferences_normalize::normalize_saved_settings(request.settings.clone());
    store.save_settings(&normalized).await
}

pub fn load_saved_settings_from_default_path(
    store: &crate::db::store::TaskStore,
) -> Result<SavedSettings, String> {
    // Synchronous wrapper for legacy callers. Uses tokio's block_in_place + handle.
    // First-run fallback returns defaults if the pool is unreachable.
    tauri::async_runtime::block_on(async move { store.load_settings().await })
        .or_else(|_| Ok(super::preferences_normalize::default_settings()))
}
```

注意：原 `default_settings_path` 等 helper **被删除**（不再需要读 JSON 路径）。

- [ ] **Step 2: 修复 callers**

需要修改的 callers（搜索全工程）：

- `commands/preferences.rs:load_user_preferences` —— 接受 store 参数
- `commands/preferences.rs:save_app_settings` —— 接受 store 参数
- `commands/translate_terms.rs:load_terminology_entries_from_saved_settings` —— 接受 store 参数
- 任何其它 `load_saved_settings_from_default_path()` 调用点

每个 caller 修改为：从 `AppHandle` 通过 `app.state::<TaskStore>()` 取 store，再调用新签名。**这一步会牵动多个文件**，按搜索结果逐个改。

- [ ] **Step 3: 编译**

Run: `cd D:/voxtrans/src-tauri && cargo check 2>&1 | tail -20`

Expected: 全部 caller 修改完才能编译通过。先编译，按报错逐个修 caller。

- [ ] **Step 4: 跑测试**

Run: `cd D:/voxtrans/src-tauri && cargo test --lib 2>&1 | tail -30`

Expected: 之前的测试仍通过。

- [ ] **Step 5: Commit**

```bash
cd D:/voxtrans && git add -A
git commit -m "feat(preferences): switch settings persistence to SQLite"
```

---

## Task 7: conversion.rs — terminology 转换

**Files:**
- Modify: `src-tauri/src/db/conversion.rs`

- [ ] **Step 1: 在 conversion.rs 追加 terminology 转换函数**

在 `src-tauri/src/db/conversion.rs` 末尾添加：

```rust
use crate::db::models::{TerminologyGroupRow, TerminologyTermRow};
use crate::services::preferences_types::{TerminologyGroup, TerminologyTerm};

pub fn terminology_group_from_row(row: TerminologyGroupRow) -> TerminologyGroup {
    TerminologyGroup {
        id: row.id,
        name: row.name,
        terms: Vec::new(), // composed in store.rs
    }
}

pub fn row_from_terminology_group(group: &TerminologyGroup) -> TerminologyGroupRow {
    TerminologyGroupRow {
        id: group.id.clone(),
        name: group.name.clone(),
        updated_at: now_ms(),
    }
}

pub fn terminology_term_from_row(row: TerminologyTermRow) -> TerminologyTerm {
    TerminologyTerm {
        id: row.id,
        origin: row.origin,
        target: row.target,
        note: row.note,
    }
}

pub fn row_from_terminology_term(group_id: &str, term: &TerminologyTerm) -> TerminologyTermRow {
    TerminologyTermRow {
        id: term.id.clone(),
        group_id: group_id.to_string(),
        origin: term.origin.clone(),
        target: term.target.clone(),
        note: term.note.clone(),
        updated_at: now_ms(),
    }
}
```

- [ ] **Step 2: 跑测试**

Run: `cd D:/voxtrans/src-tauri && cargo test --lib db::conversion 2>&1 | tail -20`

Expected: 之前 settings 测试仍通过；新函数无独立测试（被 store 集成测试覆盖）。

- [ ] **Step 3: Commit**

```bash
cd D:/voxtrans && git add src-tauri/src/db/conversion.rs
git commit -m "feat(db): terminology row <-> business object conversion"
```

---

## Task 8: store.rs — terminology 完整 load

**Files:**
- Modify: `src-tauri/src/db/store.rs`

- [ ] **Step 1: 实现 load_terminology_groups_internal**

在 `src-tauri/src/db/store.rs` 里替换 Task 5 里写的占位实现：

```rust
    async fn load_terminology_groups_internal(
        &self,
    ) -> Result<Vec<crate::services::preferences_types::TerminologyGroup>, String> {
        use std::collections::HashMap;

        let group_rows = sqlx::query("SELECT id, name, updated_at FROM terminology_groups ORDER BY id")
            .fetch_all(&self.pool)
            .await
            .map_err(|e| format!("load terminology_groups: {e}"))?;

        let term_rows = sqlx::query(
            "SELECT id, group_id, origin, target, note, updated_at FROM terminology_terms ORDER BY group_id, id",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| format!("load terminology_terms: {e}"))?;

        let mut terms_by_group: HashMap<String, Vec<crate::services::preferences_types::TerminologyTerm>> = HashMap::new();
        for r in term_rows {
            let group_id: String = r.get("group_id");
            terms_by_group.entry(group_id).or_default().push(
                crate::services::preferences_types::TerminologyTerm {
                    id: r.get("id"),
                    origin: r.get("origin"),
                    target: r.get("target"),
                    note: r.get("note"),
                },
            );
        }

        let mut groups = Vec::with_capacity(group_rows.len());
        for r in group_rows {
            let id: String = r.get("id");
            let name: String = r.get("name");
            let terms = terms_by_group.remove(&id).unwrap_or_default();
            groups.push(crate::services::preferences_types::TerminologyGroup { id, name, terms });
        }
        Ok(groups)
    }
```

- [ ] **Step 2: 跑测试**

Run: `cd D:/voxtrans/src-tauri && cargo test --lib db::store 2>&1 | tail -20`

Expected: `save_then_load_roundtrips_settings` 测试通过（terminology_groups 现在能 round-trip）。

- [ ] **Step 3: Commit**

```bash
cd D:/voxtrans && git add src-tauri/src/db/store.rs
git commit -m "feat(db): implement terminology_groups loader"
```

---

## Task 8.5: conversion + store — subtitle_segments / subtitle_words

**Files:**
- Modify: `src-tauri/src/db/conversion.rs`
- Modify: `src-tauri/src/db/store.rs`

- [ ] **Step 1: 在 conversion.rs 追加 segments/words 转换**

追加：

```rust
use crate::db::models::{SubtitleSegmentRow, SubtitleWordRow};
use crate::services::workspace_subtitle::{WorkspaceSubtitleSegment, WorkspaceSubtitleWord};

pub fn segment_from_row(row: SubtitleSegmentRow) -> WorkspaceSubtitleSegment {
    WorkspaceSubtitleSegment {
        start_ms: row.start_ms,
        end_ms: row.end_ms,
        source_text: row.source_text,
        translated_text: row.translated_text,
        source_words: Vec::new(), // composed in store.rs
    }
}

pub fn row_from_segment(task_id: &str, idx: i32, seg: &WorkspaceSubtitleSegment) -> (SubtitleSegmentRow, Vec<SubtitleWordRow>) {
    let segment_id = format!("{task_id}-seg-{idx}");
    let now = now_ms();
    let seg_row = SubtitleSegmentRow {
        id: segment_id.clone(),
        task_id: task_id.to_string(),
        idx,
        start_ms: seg.start_ms,
        end_ms: seg.end_ms,
        source_text: seg.source_text.clone(),
        translated_text: seg.translated_text.clone(),
        updated_at: now,
    };
    let word_rows: Vec<SubtitleWordRow> = seg.source_words.iter().enumerate().map(|(i, w)| SubtitleWordRow {
        id: format!("{segment_id}-word-{i}"),
        segment_id: segment_id.clone(),
        idx: i as i32,
        start_ms: w.start_ms,
        end_ms: w.end_ms,
        word: w.word.clone(),
        updated_at: now,
    }).collect();
    (seg_row, word_rows)
}
```

- [ ] **Step 2: 在 store.rs 追加 segments/words CRUD**

```rust
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
```

- [ ] **Step 3: 写测试**

```rust
    use crate::services::workspace_subtitle::{WorkspaceSubtitleSegment, WorkspaceSubtitleWord};

    fn sample_segments() -> Vec<WorkspaceSubtitleSegment> {
        vec![
            WorkspaceSubtitleSegment {
                start_ms: 0,
                end_ms: 1000,
                source_text: "hello".into(),
                translated_text: "你好".into(),
                source_words: vec![
                    WorkspaceSubtitleWord { start_ms: 0, end_ms: 500, word: "hello".into() },
                ],
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
        s.upsert_task(&sample_task("t1")).await.unwrap();
        s.replace_segments("t1", &sample_segments()).await.unwrap();

        let loaded = s.load_segments("t1").await.unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].source_text, "hello");
        assert_eq!(loaded[0].translated_text, "你好");
        assert_eq!(loaded[0].source_words.len(), 1);
        assert_eq!(loaded[0].source_words[0].word, "hello");
        assert_eq!(loaded[1].source_text, "world");
    }

    #[tokio::test]
    async fn segments_replaced_on_second_call() {
        let s = store().await;
        s.upsert_task(&sample_task("t1")).await.unwrap();
        s.replace_segments("t1", &sample_segments()).await.unwrap();
        s.replace_segments("t1", &sample_segments()[..1]).await.unwrap();

        let loaded = s.load_segments("t1").await.unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].source_text, "hello");
    }
```

- [ ] **Step 4: 跑测试**

Run: `cd D:/voxtrans/src-tauri && cargo test --lib db::store 2>&1 | tail -20`

Expected: 7 passed (2 settings + 3 task + 2 segments).

- [ ] **Step 5: Commit**

```bash
cd D:/voxtrans && git add src-tauri/src/db/conversion.rs src-tauri/src/db/store.rs
git commit -m "feat(db): subtitle_segments + subtitle_words CRUD with CASCADE"
```

---

## Task 9: conversion.rs — task / task_progress 转换

**Files:**
- Modify: `src-tauri/src/db/conversion.rs`

- [ ] **Step 1: 追加 task 转换函数**

在 `src-tauri/src/db/conversion.rs` 末尾添加：

```rust
use crate::commands::workspace::types::{
    WorkspaceQueueItem, WorkspaceTaskProgressState, WorkspaceTaskStageState,
};
use crate::db::models::TaskRow;

pub fn task_from_row(row: TaskRow) -> WorkspaceQueueItem {
    WorkspaceQueueItem {
        id: row.id,
        path: row.media_path,
        name: row.name,
        media_kind: row.media_kind,
        size_bytes: row.size_bytes,
        source_lang: row.source_lang,
        target_lang: row.target_lang,
        transcribe_status: row.transcribe_status,
        task_progress: WorkspaceTaskProgressState {
            stage: WorkspaceTaskStageState {
                code: row.task_progress_stage_code,
                label: row.task_progress_stage_label,
                order: row.task_progress_stage_order,
                detail: row.task_progress_detail,
                current: row.task_progress_current,
                total: row.task_progress_total,
            },
        },
        transcribe_error: row.transcribe_error,
        result_text: row.result_text,
        result_srt: row.result_srt,
        subtitle_segments_json: String::new(), // composed in store.rs from subtitle_segments
        llm_total_tokens: row.llm_total_tokens,
    }
}

pub fn row_from_task(item: &WorkspaceQueueItem) -> TaskRow {
    TaskRow {
        id: item.id.clone(),
        media_path: item.path.clone(),
        name: item.name.clone(),
        media_kind: item.media_kind.clone(),
        size_bytes: item.size_bytes,
        source_lang: item.source_lang.clone(),
        target_lang: item.target_lang.clone(),
        transcribe_status: item.transcribe_status.clone(),
        task_progress_stage_code: item.task_progress.stage.code.clone(),
        task_progress_stage_label: item.task_progress.stage.label.clone(),
        task_progress_stage_order: item.task_progress.stage.order,
        task_progress_detail: item.task_progress.stage.detail.clone(),
        task_progress_current: item.task_progress.stage.current,
        task_progress_total: item.task_progress.stage.total,
        transcribe_error: item.transcribe_error.clone(),
        result_text: item.result_text.clone(),
        result_srt: item.result_srt.clone(),
        llm_total_tokens: item.llm_total_tokens,
        intent: String::new(),        // set by caller
        max_retries: 0,               // set by caller
        settings_snapshot_provider: String::new(),
        settings_snapshot_asr_model: String::new(),
        settings_snapshot_align_model: String::new(),
        settings_snapshot_demucs_model: String::new(),
        settings_snapshot_translate_api_key: String::new(),
        settings_snapshot_translate_base_url: String::new(),
        settings_snapshot_translate_model: String::new(),
        settings_snapshot_llm_concurrency: 0,
        settings_snapshot_chunk_target_seconds: 0,
        settings_snapshot_enable_vocal_separation: false,
        updated_at: now_ms(),
    }
}
```

- [ ] **Step 2: 写 round-trip 测试**

在 conversion.rs 的 tests 模块加：

```rust
    #[test]
    fn task_roundtrip_preserves_core_fields() {
        let original = WorkspaceQueueItem {
            id: "task-1".into(),
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
                    detail: "chunk 5/10".into(),
                    current: 5,
                    total: 10,
                },
            },
            transcribe_error: "".into(),
            result_text: "hello".into(),
            result_srt: "1\n00:00:00,000 --> 00:00:01,000\nhello".into(),
            subtitle_segments_json: "[]".into(),
            llm_total_tokens: 42,
        };
        let row = row_from_task(&original);
        let restored = task_from_row(row);

        assert_eq!(restored.id, original.id);
        assert_eq!(restored.media_path, original.path);
        assert_eq!(restored.transcribe_status, original.transcribe_status);
        assert_eq!(restored.task_progress.stage.code, original.task_progress.stage.code);
        assert_eq!(restored.task_progress.stage.order, original.task_progress.stage.order);
        assert_eq!(restored.task_progress.stage.current, original.task_progress.stage.current);
        assert_eq!(restored.llm_total_tokens, original.llm_total_tokens);
    }
```

- [ ] **Step 3: 跑测试**

Run: `cd D:/voxtrans/src-tauri && cargo test --lib db::conversion 2>&1 | tail -20`

Expected: 3 passed.

- [ ] **Step 4: Commit**

```bash
cd D:/voxtrans && git add src-tauri/src/db/conversion.rs
git commit -m "feat(db): task row <-> WorkspaceQueueItem conversion"
```

---

## Task 10: store.rs — task CRUD

**Files:**
- Modify: `src-tauri/src/db/store.rs`

- [ ] **Step 1: 在 TaskStore 添加 task CRUD**

在 `src-tauri/src/db/store.rs` 的 `impl TaskStore` 块里添加：

```rust
    // ---- tasks ----

    pub async fn load_all_tasks(&self) -> Result<Vec<crate::commands::workspace::types::WorkspaceQueueItem>, String> {
        let rows = sqlx::query(
            "SELECT id, media_path, name, media_kind, size_bytes, source_lang, target_lang, \
             transcribe_status, task_progress_stage_code, task_progress_stage_label, \
             task_progress_stage_order, task_progress_detail, task_progress_current, \
             task_progress_total, transcribe_error, result_text, result_srt, llm_total_tokens, \
             intent, max_retries, settings_snapshot_provider, settings_snapshot_asr_model, \
             settings_snapshot_align_model, settings_snapshot_demucs_model, \
             settings_snapshot_translate_api_key, settings_snapshot_translate_base_url, \
             settings_snapshot_translate_model, settings_snapshot_llm_concurrency, \
             settings_snapshot_chunk_target_seconds, settings_snapshot_enable_vocal_separation, \
             updated_at FROM tasks ORDER BY updated_at DESC",
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
                settings_snapshot_provider: r.get("settings_snapshot_provider"),
                settings_snapshot_asr_model: r.get("settings_snapshot_asr_model"),
                settings_snapshot_align_model: r.get("settings_snapshot_align_model"),
                settings_snapshot_demucs_model: r.get("settings_snapshot_demucs_model"),
                settings_snapshot_translate_api_key: r.get("settings_snapshot_translate_api_key"),
                settings_snapshot_translate_base_url: r.get("settings_snapshot_translate_base_url"),
                settings_snapshot_translate_model: r.get("settings_snapshot_translate_model"),
                settings_snapshot_llm_concurrency: r.get::<i64, _>("settings_snapshot_llm_concurrency") as u32,
                settings_snapshot_chunk_target_seconds: r.get::<i64, _>("settings_snapshot_chunk_target_seconds") as u32,
                settings_snapshot_enable_vocal_separation: r.get::<i64, _>("settings_snapshot_enable_vocal_separation") != 0,
                updated_at: r.get("updated_at"),
            };
            out.push(task_from_row(row));
        }
        Ok(out)
    }

    pub async fn upsert_task(
        &self,
        item: &crate::commands::workspace::types::WorkspaceQueueItem,
    ) -> Result<(), String> {
        let row = row_from_task(item);
        sqlx::query(
            "INSERT INTO tasks (id, media_path, name, media_kind, size_bytes, source_lang, \
             target_lang, transcribe_status, task_progress_stage_code, \
             task_progress_stage_label, task_progress_stage_order, task_progress_detail, \
             task_progress_current, task_progress_total, transcribe_error, result_text, \
             result_srt, llm_total_tokens, intent, max_retries, \
             settings_snapshot_provider, settings_snapshot_asr_model, \
             settings_snapshot_align_model, settings_snapshot_demucs_model, \
             settings_snapshot_translate_api_key, settings_snapshot_translate_base_url, \
             settings_snapshot_translate_model, settings_snapshot_llm_concurrency, \
             settings_snapshot_chunk_target_seconds, settings_snapshot_enable_vocal_separation, \
             updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, \
             ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) ON CONFLICT(id) DO UPDATE SET \
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
        .bind(&row.settings_snapshot_provider)
        .bind(&row.settings_snapshot_asr_model)
        .bind(&row.settings_snapshot_align_model)
        .bind(&row.settings_snapshot_demucs_model)
        .bind(&row.settings_snapshot_translate_api_key)
        .bind(&row.settings_snapshot_translate_base_url)
        .bind(&row.settings_snapshot_translate_model)
        .bind(row.settings_snapshot_llm_concurrency as i64)
        .bind(row.settings_snapshot_chunk_target_seconds as i64)
        .bind(row.settings_snapshot_enable_vocal_separation as i64)
        .bind(row.updated_at)
        .execute(&self.pool)
        .await
        .map_err(|e| format!("upsert task: {e}"))?;
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
```

并在文件顶部加 `use crate::db::conversion::{row_from_settings, row_from_task, settings_from_row, task_from_row};` 和 `use crate::db::models::TaskRow;` 调整。

并在 `#[cfg(test)]` 之前添加 `fn now_ms()` helper（与 conversion.rs 同步）：

```rust
fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
```

- [ ] **Step 2: 写 task 测试**

在 `mod tests` 里追加：

```rust
    fn sample_task(id: &str) -> crate::commands::workspace::types::WorkspaceQueueItem {
        crate::commands::workspace::types::WorkspaceQueueItem {
            id: id.into(),
            path: "/tmp/a.mp3".into(),
            name: "a.mp3".into(),
            media_kind: "audio".into(),
            size_bytes: 1024,
            source_lang: "en".into(),
            target_lang: "zh-CN".into(),
            transcribe_status: "processing".into(),
            task_progress: crate::commands::workspace::types::WorkspaceTaskProgressState {
                stage: crate::commands::workspace::types::WorkspaceTaskStageState {
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
        s.upsert_task(&original).await.expect("upsert");

        let loaded = s.load_all_tasks().await.expect("load");
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, "task-1");
        assert_eq!(loaded[0].transcribe_status, "processing");
        assert_eq!(loaded[0].task_progress.stage.code, "recognizing");
    }

    #[tokio::test]
    async fn mark_orphan_processing_as_error_recovers_residuals() {
        let s = store().await;
        s.upsert_task(&sample_task("a")).await.unwrap();
        s.upsert_task(&sample_task("b")).await.unwrap();

        let n = s.mark_orphan_processing_as_error().await.unwrap();
        assert_eq!(n, 2);

        let loaded = s.load_all_tasks().await.unwrap();
        for item in &loaded {
            assert_eq!(item.transcribe_status, "error");
            assert!(!item.transcribe_error.is_empty());
        }
    }

    #[tokio::test]
    async fn delete_task_removes_row() {
        let s = store().await;
        s.upsert_task(&sample_task("a")).await.unwrap();
        s.delete_task("a").await.unwrap();
        assert!(s.load_all_tasks().await.unwrap().is_empty());
    }
```

- [ ] **Step 3: 跑测试**

Run: `cd D:/voxtrans/src-tauri && cargo test --lib db::store 2>&1 | tail -20`

Expected: 5 passed (2 settings + 3 task).

- [ ] **Step 4: Commit**

```bash
cd D:/voxtrans && git add src-tauri/src/db/store.rs
git commit -m "feat(db): task CRUD with orphan processing recovery"
```

---

## Task 11: meta.rs — hydrate_workspace_from_db

**Files:**
- Modify: `src-tauri/src/commands/workspace/meta.rs`

- [ ] **Step 1: 重写 hydrate_workspace**

修改 `src-tauri/src/commands/workspace/meta.rs`：

- 删掉 `load_task_meta_artifact_from_task_dir` 和 `load_task_meta_artifacts_from_task_dirs`
- 删掉 `load_task_meta_artifact_from_task_dirs`、`load_task_meta_artifacts_from_output_dir` 的所有 JSON 文件 IO
- 删掉 `task_meta_path_for_item`、`TASK_META_FILE_NAME` 等 helper
- 改 `ensure_workspace_hydrated_from_disk` 为 `ensure_workspace_hydrated_from_db`：从 `AppHandle` 取 `TaskStore`，调 `load_all_tasks` + `mark_orphan_processing_as_error`，再重 `load_all_tasks`
- 改 `hydrate_workspace_from_disk` 为 `hydrate_workspace_from_db`
- `persist_task_meta` 改为通过 store.upsert_task
- `remove_task_meta` 改为通过 store.delete_task
- 删除文件存在性 / 路径解析代码

新 `ensure_workspace_hydrated_from_db` 大致：

```rust
pub(super) async fn ensure_workspace_hydrated_from_db(app: &AppHandle) -> WorkspaceResult<()> {
    {
        let hydrated = lock_workspace_hydrated()?;
        if *hydrated {
            return Ok(());
        }
    }
    let store = app.state::<crate::db::store::TaskStore>().inner().clone();
    store.mark_orphan_processing_as_error().await
        .map_err(|e| WorkspaceError::TaskFailed(format!("recover orphans: {e}")))?;
    hydrate_workspace_from_db(&store).await?;
    let mut hydrated = lock_workspace_hydrated()?;
    *hydrated = true;
    Ok(())
}

pub(super) async fn hydrate_workspace_from_db(
    store: &crate::db::store::TaskStore,
) -> WorkspaceResult<()> {
    let mut items = store.load_all_tasks().await
        .map_err(|e| WorkspaceError::TaskFailed(format!("load tasks: {e}")))?;
    let mut guard = lock_workspace_store()?;
    guard.queue.clear();
    for mut item in items.drain(..) {
        // Reconstruct subtitle_segments_json from the segments + words child tables.
        let segments = store.load_segments(&item.id).await
            .map_err(|e| WorkspaceError::TaskFailed(format!("load segments {}: {e}", item.id)))?;
        item.subtitle_segments_json = serialize_segments(&segments);
        guard.queue.push_back(WorkspaceTaskRecord {
            item,
            intent: String::new(),
            source_lang: String::new(),
            target_lang: String::new(),
            max_retries: 0,
            settings_snapshot: Value::Null,
        });
    }
    Ok(())
}
```

- [ ] **Step 2: 编译**

Run: `cd D:/voxtrans/src-tauri && cargo check 2>&1 | tail -20`

按报错逐个修 caller。可能 `TASK_META_FILE_NAME` 等常量被其它文件引用，需要一并删除或保留为 `pub(super) const _UNUSED: ...`。

- [ ] **Step 3: 跑测试**

Run: `cd D:/voxtrans/src-tauri && cargo test --lib 2>&1 | tail -30`

Expected: 之前的测试仍通过；旧的 `meta.rs` 测试可能因函数被删而 break，按情况删或迁移。

- [ ] **Step 4: Commit**

```bash
cd D:/voxtrans && git add -A
git commit -m "feat(workspace): hydrate workspace from DB instead of JSON files"
```

---

## Task 11.5: output_completion / preview / save_subtitle_editor 写 segments 到 DB

**Files:**
- Modify: `src-tauri/src/commands/workspace/output_completion.rs`
- Modify: `src-tauri/src/commands/workspace/preview.rs`
- Modify: `src-tauri/src/commands/workspace.rs`（save_subtitle_editor 函数）

- [ ] **Step 1: 在 output_completion.rs 的两处 `serialize_segments` 调用之后，加 DB 写**

每次 `task.item.subtitle_segments_json = subtitle_segments_json.clone();` 之后，加：

```rust
let store = app.state::<crate::db::store::TaskStore>().inner().clone();
let task_id = task.item.id.clone();
let segments = parse_segments(&subtitle_segments_json);
if let Err(e) = tauri::async_runtime::block_on(async {
    store.replace_segments(&task_id, &segments).await
}) {
    eprintln!("warn: persist segments {task_id} failed: {e}");
}
```

`parse_segments` 辅助函数（放在文件顶部或单独 helper）：

```rust
fn parse_segments(json: &str) -> Vec<crate::services::workspace_subtitle::WorkspaceSubtitleSegment> {
    serde_json::from_str(json).unwrap_or_default()
}
```

- [ ] **Step 2: 在 preview.rs 的 `serialize_segments` 调用后，同样加 DB 写**

- [ ] **Step 3: 在 save_subtitle_editor 处理后加 DB 写**

找到 `save_subtitle_editor` 命令，content 入库后：

```rust
let store = app.state::<crate::db::store::TaskStore>().inner().clone();
let task_id = request.task_id.clone();
let segments = parse_segments(&request.subtitle_segments_json);
if let Err(e) = tauri::async_runtime::block_on(async {
    store.replace_segments(&task_id, &segments).await
}) {
    eprintln!("warn: persist segments {task_id} failed: {e}");
}
```

- [ ] **Step 4: 编译**

Run: `cd D:/voxtrans/src-tauri && cargo check 2>&1 | tail -20`

- [ ] **Step 5: 跑测试**

Run: `cd D:/voxtrans/src-tauri && cargo test --lib 2>&1 | tail -30`

- [ ] **Step 6: Commit**

```bash
cd D:/voxtrans && git add -A
git commit -m "feat(workspace): persist subtitle segments to DB child tables"
```

---

## Task 12: queue_ops.rs + execution_flow.rs — 写 DB

**Files:**
- Modify: `src-tauri/src/commands/workspace/queue_ops.rs`
- Modify: `src-tauri/src/commands/workspace/execution_flow.rs`

- [ ] **Step 1: 找到所有 patch_task_item / 改 task 状态的代码点**

```bash
grep -rnE "patch_task_item|transcribe_status\s*=" D:/voxtrans/src-tauri/src/commands/workspace/ --include="*.rs"
```

- [ ] **Step 2: 在 queue_ops.rs 的 patch_task_item 之后调 store.upsert_task**

```rust
fn patch_task_item(
    app: &AppHandle,
    task_id: &str,
    mutator: impl FnOnce(&mut WorkspaceTaskRecord),
) -> WorkspaceResult<()> {
    let mut guard = lock_workspace_store()?;
    let record = guard.queue.iter_mut()
        .find(|r| r.item.id == task_id)
        .ok_or_else(|| WorkspaceError::TaskNotFound(task_id.to_string()))?;
    mutator(record);

    let store = app.state::<crate::db::store::TaskStore>().inner().clone();
    // Best-effort write; failure logged but not propagated.
    let item = record.item.clone();
    if let Err(e) = tauri::async_runtime::block_on(async { store.upsert_task(&item).await }) {
        eprintln!("warn: persist task {task_id} failed: {e}");
    }
    Ok(())
}
```

- [ ] **Step 3: enqueue_task_run 调 store.upsert_task**

找到 `enqueue_task_run` 函数（创建新 task 记录的位置），在 `lock_workspace_store` 写内存后调 `store.upsert_task`。

- [ ] **Step 4: delete_tasks 调 store.delete_task**

修改 `delete_tasks` 内部循环，对每个被删的 task id 调 `store.delete_task`。

- [ ] **Step 5: 编译**

Run: `cd D:/voxtrans/src-tauri && cargo check 2>&1 | tail -20`

- [ ] **Step 6: 跑测试**

Run: `cd D:/voxtrans/src-tauri && cargo test --lib 2>&1 | tail -30`

- [ ] **Step 7: Commit**

```bash
cd D:/voxtrans && git add -A
git commit -m "feat(workspace): persist task mutations to DB"
```

---

## Task 13: main.rs — 注入 TaskStore

**Files:**
- Modify: `src-tauri/src/main.rs`

- [ ] **Step 1: 在 setup 块初始化 store**

修改 `src-tauri/src/main.rs` 的 setup 块：

```rust
.setup(|app| {
    let app_handle = app.handle().clone();
    let pool = tauri::async_runtime::block_on(async {
        crate::db::init_pool(&app_handle).await
    }).map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    app.manage(crate::db::store::TaskStore::new(pool));
    app.manage(app_state::AppState { ... });
    Ok(())
})
```

如果 `setup` 不允许异步，**改用同步 `tauri::async_runtime::block_on`** 包住 `init_pool` 调用（已经这么写了）。

- [ ] **Step 2: 编译**

Run: `cd D:/voxtrans/src-tauri && cargo check 2>&1 | tail -20`

- [ ] **Step 3: Commit**

```bash
cd D:/voxtrans && git add src-tauri/src/main.rs
git commit -m "feat: initialize TaskStore at app startup"
```

---

## Task 14: 删除 settings.json 写入代码路径

**Files:**
- Modify: `src-tauri/src/services/preferences.rs`（已切走，确认无残留）
- Modify: `src-tauri/src/commands/workspace/meta.rs`（已切走，确认无残留）

- [ ] **Step 1: 全工程搜索 `settings.json` 引用**

```bash
grep -rnE "settings\.json|setttings_path|SETTINGS_FILE_NAME" D:/voxtrans/src-tauri/src/ --include="*.rs"
```

确认无残留。如果有，删除。

- [ ] **Step 2: 全工程搜索 `task_meta.json` 引用**

```bash
grep -rnE "task_meta\.json|TASK_META_FILE_NAME|task_meta_path" D:/voxtrans/src-tauri/src/ --include="*.rs"
```

确认无残留。如果有，删除。

- [ ] **Step 3: 编译**

Run: `cd D:/voxtrans/src-tauri && cargo check 2>&1 | tail -20`

Expected: 编译通过，零 JSON 文件 IO 路径。

- [ ] **Step 4: 跑全测试**

Run: `cd D:/voxtrans/src-tauri && cargo test 2>&1 | tail -30`

- [ ] **Step 5: Commit**

```bash
cd D:/voxtrans && git add -A
git commit -m "chore: remove all settings.json / task_meta.json IO code paths"
```

---

## Task 15: 端到端手测

- [ ] **Step 1: 全新启动**

```bash
cd D:/voxtrans/src-tauri && cargo run
```

- DB 文件应自动建在 `app_data_dir/voxtrans.db`
- 应用启动无错
- 关闭应用

- [ ] **Step 2: 验证 DB 存在 + 表已建**

```bash
ls "$(cygpath -u "$APPDATA")/com.voxtrans.desktop/" 2>&1 | head -10
# 应看到 voxtrans.db
```

打开 DB 工具（如 `sqlite3` 命令行）：

```sql
SELECT name FROM sqlite_master WHERE type='table';
```

Expected: 7 张表 + `__sqlx_migrations`。

- [ ] **Step 3: 验证 settings 走 DB**

进应用 → 改设置（启用术语库、加术语组、设字体大小）→ 关闭 → 重开

```sql
SELECT provider, source_font_size FROM settings;
SELECT COUNT(*) FROM terminology_groups;
SELECT COUNT(*) FROM terminology_terms;
SELECT value FROM flat_srt_items;
```

Expected: 值与界面一致。

- [ ] **Step 4: 验证 tasks 走 DB**

进应用 → 上传一个任务 → 跑完 → 关闭 → 重开

```sql
SELECT id, transcribe_status FROM tasks;
```

Expected: 任务在，状态 `done`。

- [ ] **Step 5: 验证 processing → error 修复**

进应用 → 上传任务 → 启动 → **强制 kill 应用进程** → 重开

```sql
SELECT id, transcribe_status, transcribe_error FROM tasks;
```

Expected: 状态 `error`，error 信息 = "任务在运行中被中断，请重新开始"。

- [ ] **Step 6: 验证老的 settings.json / task_meta.json 被忽略**

把 `app_data_dir/settings.json` 放回去（手工伪造一个），重启应用：

Expected: DB 里的设置不变，UI 不显示老 settings.json 的内容。

同样：把 `output/<task>/task_meta.json` 放回去，重启——UI 不应看到老任务。

- [ ] **Step 7: 验证字幕段编辑持久化**

进应用 → 跑一个任务到产生 segments → 编辑某段 → 关闭 → 重开

```sql
SELECT id, source_text, translated_text FROM subtitle_segments ORDER BY idx LIMIT 5;
SELECT id, segment_id, word FROM subtitle_words ORDER BY segment_id, idx LIMIT 10;
```

Expected: 编辑后的文本在 DB。

- [ ] **Step 8: Commit（如果手测有微调）**

```bash
cd D:/voxtrans && git add -A
git commit -m "test: end-to-end verification of SQLite persistence"
```

---

## 总结

12 个实施 task + 1 个手测 task。涉及 7 张表 + 4 个集成点（preferences / workspace / execution_flow / main）+ 1 个手测验证。

每个 task 一个 commit，commit message 用 conventional commits。整体 TDD：conversion 写测试 → 写实现 → 跑测试；store 写测试 → 写实现 → 跑测试；集成点改完编译过、手测端到端。
