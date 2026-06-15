-- Voxtrans 数据库 schema —— 幂等初始化
--
-- 设计原则（见 ROADMAP / 用户决策）：桌面本地工具，不需要"老用户平滑迁移"
-- 这套复杂度。启动时执行本文件，所有对象都用 IF NOT EXISTS 保护：
--   - 新库：建出全部表/索引
--   - 老库：所有对象已存在 → 全部跳过，什么都不改
--
-- 不再使用 sqlx::migrate! 的 checksum 校验机制（它会因 migration 文件任意
-- 改动而 panic）。schema 演进时：要么 ALTER TABLE（手写一次性补丁），要么
-- 让用户删库重建 —— Voxtrans 的数据全是可再生的转录产物，不值得为它维护
-- 迁移框架。

-- settings: 单行配置（1:1 全平铺 SavedSettings）
CREATE TABLE IF NOT EXISTS settings (
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
    active_terminology_group_id TEXT NOT NULL DEFAULT '',
    enable_subtitle_beautify INTEGER NOT NULL,
    enable_click_sound INTEGER NOT NULL,
    auto_burn_hard_subtitle INTEGER NOT NULL,
    subtitle_burn_mode TEXT NOT NULL,
    subtitle_render_style_json TEXT NOT NULL,
    flat_srt_output INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

-- flat_srt_items: settings 的 1:N 子表
CREATE TABLE IF NOT EXISTS flat_srt_items (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    value TEXT NOT NULL UNIQUE,
    updated_at INTEGER NOT NULL
);

-- tasks: 1:1 全平铺 WorkspaceQueueItem + wrapper
CREATE TABLE IF NOT EXISTS tasks (
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
    -- frozen per-task settings（见 migration 20260610000001）
    subtitle_length_preset TEXT NOT NULL DEFAULT '',
    enable_subtitle_beautify INTEGER NOT NULL DEFAULT 1,
    terminology_groups_json TEXT NOT NULL DEFAULT '[]',
    terminology_group_id TEXT NOT NULL DEFAULT '',
    updated_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_tasks_status ON tasks(transcribe_status);
CREATE INDEX IF NOT EXISTS idx_tasks_updated_at ON tasks(updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_tasks_langs ON tasks(source_lang, target_lang);
CREATE INDEX IF NOT EXISTS idx_tasks_media_kind ON tasks(media_kind);

-- task_artifacts: pipeline step checkpoint 缓存（tasks 的 1:N，CASCADE 删除）
CREATE TABLE IF NOT EXISTS task_artifacts (
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    step_name TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (task_id, step_name)
);

-- subtitle_segments: tasks 的 1:N
CREATE TABLE IF NOT EXISTS subtitle_segments (
    id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    idx INTEGER NOT NULL,
    start_ms INTEGER NOT NULL,
    end_ms INTEGER NOT NULL,
    source_text TEXT NOT NULL DEFAULT '',
    translated_text TEXT NOT NULL DEFAULT '',
    updated_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_segments_task_idx ON subtitle_segments(task_id, idx);

-- subtitle_words: segments 的 1:N（无豁免）
CREATE TABLE IF NOT EXISTS subtitle_words (
    id TEXT PRIMARY KEY,
    segment_id TEXT NOT NULL REFERENCES subtitle_segments(id) ON DELETE CASCADE,
    idx INTEGER NOT NULL,
    start_ms INTEGER NOT NULL,
    end_ms INTEGER NOT NULL,
    word TEXT NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_words_segment_idx ON subtitle_words(segment_id, idx);

-- terminology_groups
CREATE TABLE IF NOT EXISTS terminology_groups (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    updated_at INTEGER NOT NULL
);

-- terminology_terms
CREATE TABLE IF NOT EXISTS terminology_terms (
    id TEXT PRIMARY KEY,
    group_id TEXT NOT NULL REFERENCES terminology_groups(id) ON DELETE CASCADE,
    origin TEXT NOT NULL,
    target TEXT NOT NULL,
    note TEXT NOT NULL DEFAULT '',
    updated_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_terms_group_id ON terminology_terms(group_id);

-- asr_transcripts: Step 1 ASR 段级转录结果
CREATE TABLE IF NOT EXISTS asr_transcripts (
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    segment_index INTEGER NOT NULL,
    text TEXT NOT NULL,
    PRIMARY KEY (task_id, segment_index)
);

-- translation_batch_results: Step 4 翻译批次结果
CREATE TABLE IF NOT EXISTS translation_batch_results (
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    batch_index INTEGER NOT NULL,
    segment_translations TEXT NOT NULL,
    PRIMARY KEY (task_id, batch_index)
);

-- alignment_results: Step 1 强制对齐段级结果
CREATE TABLE IF NOT EXISTS alignment_results (
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    segment_index INTEGER NOT NULL,
    result_json TEXT NOT NULL,
    PRIMARY KEY (task_id, segment_index)
);

-- step5_split_align_results: Step 5 拆分+对齐 段级幂等结果
CREATE TABLE IF NOT EXISTS step5_split_align_results (
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    segment_index INTEGER NOT NULL,
    parent_json TEXT NOT NULL,
    PRIMARY KEY (task_id, segment_index)
);
