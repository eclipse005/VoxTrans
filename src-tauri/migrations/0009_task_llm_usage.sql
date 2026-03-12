CREATE TABLE IF NOT EXISTS task_llm_usage (
  task_id TEXT NOT NULL,
  stage TEXT NOT NULL,
  prompt_tokens INTEGER NOT NULL DEFAULT 0,
  completion_tokens INTEGER NOT NULL DEFAULT 0,
  total_tokens INTEGER NOT NULL DEFAULT 0,
  updated_at INTEGER NOT NULL,
  PRIMARY KEY (task_id, stage),
  FOREIGN KEY (task_id) REFERENCES tasks(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_task_llm_usage_task ON task_llm_usage(task_id, updated_at DESC);
