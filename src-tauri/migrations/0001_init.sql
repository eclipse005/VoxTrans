CREATE TABLE IF NOT EXISTS app_settings (
  key TEXT PRIMARY KEY NOT NULL,
  value TEXT NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS terms (
  id TEXT PRIMARY KEY NOT NULL,
  source TEXT NOT NULL,
  target TEXT NOT NULL,
  note TEXT NOT NULL,
  sort_order INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS hotword_groups (
  id TEXT PRIMARY KEY NOT NULL,
  name TEXT NOT NULL,
  sort_order INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS hotword_terms (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  group_id TEXT NOT NULL,
  term TEXT NOT NULL,
  sort_order INTEGER NOT NULL,
  FOREIGN KEY(group_id) REFERENCES hotword_groups(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS hotword_meta (
  singleton_id INTEGER PRIMARY KEY CHECK (singleton_id = 1),
  enabled INTEGER NOT NULL,
  active_group_id TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS queue_items (
  id TEXT PRIMARY KEY NOT NULL,
  path TEXT NOT NULL,
  name TEXT NOT NULL,
  media_kind TEXT NOT NULL,
  size_bytes INTEGER NOT NULL,
  transcribe_status TEXT NOT NULL,
  transcribe_progress INTEGER NOT NULL,
  transcribe_segment_current INTEGER NOT NULL,
  transcribe_segment_total INTEGER NOT NULL,
  transcribe_error TEXT NOT NULL,
  translate_status TEXT NOT NULL,
  translate_progress INTEGER NOT NULL,
  translate_error TEXT NOT NULL,
  result_text TEXT NOT NULL,
  result_srt TEXT NOT NULL,
  subtitle_segments_json TEXT NOT NULL,
  hotword_hint_json TEXT NOT NULL DEFAULT '',
  sort_order INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS tasks (
  id TEXT PRIMARY KEY NOT NULL,
  media_path TEXT NOT NULL,
  name TEXT NOT NULL,
  media_kind TEXT NOT NULL,
  size_bytes INTEGER NOT NULL,
  last_status TEXT NOT NULL,
  last_error TEXT NOT NULL,
  output_srt_path TEXT NOT NULL,
  output_words_json TEXT NOT NULL DEFAULT '',
  source_lang TEXT NOT NULL DEFAULT 'auto',
  target_lang TEXT NOT NULL DEFAULT '',
  transcribe_status TEXT NOT NULL DEFAULT 'pending',
  transcribe_error TEXT NOT NULL DEFAULT '',
  transcript_srt TEXT NOT NULL DEFAULT '',
  transcribed_at INTEGER,
  translate_status TEXT NOT NULL DEFAULT 'idle',
  translate_error TEXT NOT NULL DEFAULT '',
  translated_srt TEXT NOT NULL DEFAULT '',
  translated_srt_path TEXT NOT NULL DEFAULT '',
  subtitle_segments_json TEXT NOT NULL DEFAULT '[]',
  translate_model TEXT NOT NULL DEFAULT '',
  hotword_status TEXT NOT NULL DEFAULT '',
  hotword_changed_count INTEGER NOT NULL DEFAULT 0,
  hotword_replacements_json TEXT NOT NULL DEFAULT '[]',
  translated_at INTEGER,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS task_events (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  task_id TEXT,
  event_type TEXT NOT NULL,
  payload_json TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  FOREIGN KEY(task_id) REFERENCES tasks(id) ON DELETE SET NULL
);

CREATE TABLE IF NOT EXISTS task_llm_usage (
  task_id TEXT NOT NULL,
  stage TEXT NOT NULL,
  prompt_tokens INTEGER NOT NULL DEFAULT 0,
  completion_tokens INTEGER NOT NULL DEFAULT 0,
  total_tokens INTEGER NOT NULL DEFAULT 0,
  updated_at INTEGER NOT NULL,
  PRIMARY KEY (task_id, stage)
);

CREATE INDEX IF NOT EXISTS idx_tasks_updated_at
  ON tasks(updated_at DESC);

CREATE INDEX IF NOT EXISTS idx_tasks_transcribe_status
  ON tasks(transcribe_status, updated_at DESC);

CREATE INDEX IF NOT EXISTS idx_tasks_translate_status
  ON tasks(translate_status, updated_at DESC);

CREATE INDEX IF NOT EXISTS idx_task_events_task_time
  ON task_events(task_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_task_events_type_time
  ON task_events(event_type, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_task_llm_usage_task
  ON task_llm_usage(task_id, updated_at DESC);
