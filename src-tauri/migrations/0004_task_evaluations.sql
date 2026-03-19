PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS task_evaluations (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  task_id TEXT NOT NULL,
  version TEXT NOT NULL DEFAULT 'v1',
  overall_score REAL NOT NULL DEFAULT 0,
  summary TEXT NOT NULL DEFAULT '',
  metrics_json TEXT NOT NULL DEFAULT '{}',
  output_path TEXT NOT NULL DEFAULT '',
  created_at INTEGER NOT NULL DEFAULT (strftime('%s','now')),
  updated_at INTEGER NOT NULL DEFAULT (strftime('%s','now')),
  FOREIGN KEY(task_id) REFERENCES task_runs(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_task_evaluations_task_time
  ON task_evaluations(task_id, created_at DESC);
