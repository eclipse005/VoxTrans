PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS task_runs (
  id TEXT PRIMARY KEY NOT NULL,
  media_path TEXT NOT NULL,
  name TEXT NOT NULL,
  media_kind TEXT NOT NULL,
  size_bytes INTEGER NOT NULL DEFAULT 0,
  intent TEXT NOT NULL,
  state TEXT NOT NULL,
  current_step TEXT NOT NULL DEFAULT '',
  progress_percent INTEGER NOT NULL DEFAULT 0,
  progress_note TEXT NOT NULL DEFAULT '',
  error_code TEXT NOT NULL DEFAULT '',
  error_message TEXT NOT NULL DEFAULT '',
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
  updated_at INTEGER NOT NULL DEFAULT (strftime('%s','now'))
);

CREATE TABLE IF NOT EXISTS task_step_runs (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  task_id TEXT NOT NULL,
  step TEXT NOT NULL,
  attempt INTEGER NOT NULL DEFAULT 1,
  status TEXT NOT NULL,
  binding_mode TEXT NOT NULL,
  input_hash TEXT NOT NULL DEFAULT '',
  settings_snapshot_json TEXT NOT NULL DEFAULT '{}',
  diagnostics_json TEXT NOT NULL DEFAULT '{}',
  error_code TEXT NOT NULL DEFAULT '',
  error_message TEXT NOT NULL DEFAULT '',
  started_at INTEGER NOT NULL DEFAULT (strftime('%s','now')),
  finished_at INTEGER,
  created_at INTEGER NOT NULL DEFAULT (strftime('%s','now')),
  updated_at INTEGER NOT NULL DEFAULT (strftime('%s','now')),
  FOREIGN KEY(task_id) REFERENCES task_runs(id) ON DELETE CASCADE
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_task_step_runs_task_step_attempt
  ON task_step_runs(task_id, step, attempt);

CREATE INDEX IF NOT EXISTS idx_task_step_runs_task_time
  ON task_step_runs(task_id, created_at DESC);

CREATE TABLE IF NOT EXISTS task_artifacts (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  task_id TEXT NOT NULL,
  kind TEXT NOT NULL,
  path TEXT NOT NULL,
  checksum TEXT NOT NULL DEFAULT '',
  size_bytes INTEGER NOT NULL DEFAULT 0,
  mime_type TEXT NOT NULL DEFAULT '',
  produced_by_step TEXT NOT NULL DEFAULT '',
  metadata_json TEXT NOT NULL DEFAULT '{}',
  created_at INTEGER NOT NULL DEFAULT (strftime('%s','now')),
  updated_at INTEGER NOT NULL DEFAULT (strftime('%s','now')),
  FOREIGN KEY(task_id) REFERENCES task_runs(id) ON DELETE CASCADE
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_task_artifacts_task_kind
  ON task_artifacts(task_id, kind);

CREATE INDEX IF NOT EXISTS idx_task_runs_state_time
  ON task_runs(state, updated_at DESC);

CREATE INDEX IF NOT EXISTS idx_task_runs_intent_time
  ON task_runs(intent, updated_at DESC);

CREATE INDEX IF NOT EXISTS idx_task_runs_media_path
  ON task_runs(media_path);
