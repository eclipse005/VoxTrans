PRAGMA foreign_keys = ON;

ALTER TABLE task_runs
  ADD COLUMN llm_prompt_tokens_total INTEGER NOT NULL DEFAULT 0;

ALTER TABLE task_runs
  ADD COLUMN llm_completion_tokens_total INTEGER NOT NULL DEFAULT 0;

ALTER TABLE task_runs
  ADD COLUMN llm_total_tokens INTEGER NOT NULL DEFAULT 0;

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

CREATE INDEX IF NOT EXISTS idx_task_llm_usage_phase_task
  ON task_llm_usage_phase(task_id);
