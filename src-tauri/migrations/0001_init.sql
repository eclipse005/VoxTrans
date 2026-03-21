PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS app_settings (
  key TEXT PRIMARY KEY NOT NULL,
  value TEXT NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS task_runs (
  id TEXT PRIMARY KEY NOT NULL,
  media_path TEXT NOT NULL,
  name TEXT NOT NULL,
  media_kind TEXT NOT NULL,
  size_bytes INTEGER NOT NULL DEFAULT 0,
  intent TEXT NOT NULL,
  retry_count INTEGER NOT NULL DEFAULT 0,
  max_retries INTEGER NOT NULL DEFAULT 0,
  settings_policy_version TEXT NOT NULL DEFAULT 'v1',
  settings_snapshot_json TEXT NOT NULL DEFAULT '{}',
  source_lang TEXT NOT NULL DEFAULT 'auto',
  target_lang TEXT NOT NULL DEFAULT 'zh-CN',
  queued_at INTEGER NOT NULL DEFAULT (strftime('%s','now')),
  started_at INTEGER,
  finished_at INTEGER,
  created_at INTEGER NOT NULL DEFAULT (strftime('%s','now')),
  updated_at INTEGER NOT NULL DEFAULT (strftime('%s','now')),
  sort_order INTEGER NOT NULL DEFAULT 0,
  overall_status TEXT NOT NULL DEFAULT 'pending',
  current_stage TEXT NOT NULL DEFAULT '',
  progress_percent INTEGER NOT NULL DEFAULT 0,
  phase_detail TEXT NOT NULL DEFAULT '',
  segment_current INTEGER NOT NULL DEFAULT 0,
  segment_total INTEGER NOT NULL DEFAULT 0,
  error_message TEXT NOT NULL DEFAULT '',
  result_text TEXT NOT NULL DEFAULT '',
  result_srt TEXT NOT NULL DEFAULT '',
  subtitle_segments_json TEXT NOT NULL DEFAULT '[]',
  translated_srt TEXT NOT NULL DEFAULT ''
);

CREATE TABLE IF NOT EXISTS task_stage_runs (
  task_id TEXT NOT NULL,
  stage TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'pending',
  attempt INTEGER NOT NULL DEFAULT 0,
  input_hash TEXT NOT NULL DEFAULT '',
  output_json TEXT NOT NULL DEFAULT '{}',
  metrics_json TEXT NOT NULL DEFAULT '{}',
  error_code TEXT NOT NULL DEFAULT '',
  error_message TEXT NOT NULL DEFAULT '',
  started_at INTEGER,
  finished_at INTEGER,
  duration_ms INTEGER NOT NULL DEFAULT 0,
  updated_at INTEGER NOT NULL DEFAULT (strftime('%s','now')),
  PRIMARY KEY (task_id, stage),
  FOREIGN KEY(task_id) REFERENCES task_runs(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS task_artifacts (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  task_id TEXT NOT NULL,
  kind TEXT NOT NULL,
  path TEXT NOT NULL,
  checksum TEXT NOT NULL DEFAULT '',
  size_bytes INTEGER NOT NULL DEFAULT 0,
  produced_by_stage TEXT NOT NULL DEFAULT '',
  metadata_json TEXT NOT NULL DEFAULT '{}',
  created_at INTEGER NOT NULL DEFAULT (strftime('%s','now')),
  updated_at INTEGER NOT NULL DEFAULT (strftime('%s','now')),
  FOREIGN KEY(task_id) REFERENCES task_runs(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS task_llm_usage_phase (
  task_id TEXT NOT NULL,
  phase TEXT NOT NULL,
  prompt_tokens INTEGER NOT NULL DEFAULT 0,
  completion_tokens INTEGER NOT NULL DEFAULT 0,
  total_tokens INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL DEFAULT (strftime('%s','now')),
  updated_at INTEGER NOT NULL DEFAULT (strftime('%s','now')),
  PRIMARY KEY (task_id, phase),
  FOREIGN KEY(task_id) REFERENCES task_runs(id) ON DELETE CASCADE
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_task_artifacts_task_kind
  ON task_artifacts(task_id, kind);

CREATE INDEX IF NOT EXISTS idx_task_runs_intent_time
  ON task_runs(intent, updated_at DESC);

CREATE INDEX IF NOT EXISTS idx_task_runs_media_path
  ON task_runs(media_path);

CREATE INDEX IF NOT EXISTS idx_task_runs_sort_order
  ON task_runs(sort_order ASC, updated_at DESC);

CREATE INDEX IF NOT EXISTS idx_task_stage_runs_task_updated
  ON task_stage_runs(task_id, updated_at DESC);

CREATE INDEX IF NOT EXISTS idx_task_llm_usage_phase_task
  ON task_llm_usage_phase(task_id);
