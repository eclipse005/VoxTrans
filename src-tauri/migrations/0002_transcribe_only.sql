PRAGMA foreign_keys = OFF;

DROP INDEX IF EXISTS idx_tasks_translate_status;
DROP INDEX IF EXISTS idx_task_llm_usage_task;

CREATE TABLE IF NOT EXISTS queue_items_new (
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
  result_text TEXT NOT NULL,
  result_srt TEXT NOT NULL,
  subtitle_segments_json TEXT NOT NULL,
  sort_order INTEGER NOT NULL
);

INSERT INTO queue_items_new (
  id, path, name, media_kind, size_bytes,
  transcribe_status, transcribe_progress, transcribe_segment_current, transcribe_segment_total,
  transcribe_error, result_text, result_srt, subtitle_segments_json, sort_order
)
SELECT
  id, path, name, media_kind, size_bytes,
  transcribe_status, transcribe_progress, transcribe_segment_current, transcribe_segment_total,
  transcribe_error, result_text, result_srt, subtitle_segments_json, sort_order
FROM queue_items;

DROP TABLE IF EXISTS queue_items;
ALTER TABLE queue_items_new RENAME TO queue_items;

CREATE TABLE IF NOT EXISTS tasks_new (
  id TEXT PRIMARY KEY NOT NULL,
  media_path TEXT NOT NULL,
  name TEXT NOT NULL,
  media_kind TEXT NOT NULL,
  size_bytes INTEGER NOT NULL,
  last_status TEXT NOT NULL,
  last_error TEXT NOT NULL,
  output_srt_path TEXT NOT NULL,
  output_words_json TEXT NOT NULL DEFAULT '',
  transcribe_status TEXT NOT NULL DEFAULT 'pending',
  transcribe_error TEXT NOT NULL DEFAULT '',
  transcript_srt TEXT NOT NULL DEFAULT '',
  subtitle_segments_json TEXT NOT NULL DEFAULT '[]',
  transcribed_at INTEGER,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

INSERT INTO tasks_new (
  id, media_path, name, media_kind, size_bytes, last_status, last_error,
  output_srt_path, output_words_json,
  transcribe_status, transcribe_error, transcript_srt, subtitle_segments_json, transcribed_at,
  created_at, updated_at
)
SELECT
  id, media_path, name, media_kind, size_bytes, last_status, last_error,
  output_srt_path, output_words_json,
  transcribe_status, transcribe_error, transcript_srt, subtitle_segments_json, transcribed_at,
  created_at, updated_at
FROM tasks;

DROP TABLE IF EXISTS tasks;
ALTER TABLE tasks_new RENAME TO tasks;

DROP TABLE IF EXISTS terms;
DROP TABLE IF EXISTS hotword_terms;
DROP TABLE IF EXISTS hotword_groups;
DROP TABLE IF EXISTS hotword_meta;
DROP TABLE IF EXISTS task_llm_usage;

CREATE INDEX IF NOT EXISTS idx_tasks_updated_at
  ON tasks(updated_at DESC);

CREATE INDEX IF NOT EXISTS idx_tasks_transcribe_status
  ON tasks(transcribe_status, updated_at DESC);

CREATE INDEX IF NOT EXISTS idx_task_events_task_time
  ON task_events(task_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_task_events_type_time
  ON task_events(event_type, created_at DESC);

PRAGMA foreign_keys = ON;
