DROP TABLE IF EXISTS task_events;
DROP TABLE IF EXISTS tasks;

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

CREATE INDEX IF NOT EXISTS idx_tasks_updated_at ON tasks(updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_tasks_transcribe_status ON tasks(transcribe_status, updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_tasks_translate_status ON tasks(translate_status, updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_task_events_task_time ON task_events(task_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_task_events_type_time ON task_events(event_type, created_at DESC);
