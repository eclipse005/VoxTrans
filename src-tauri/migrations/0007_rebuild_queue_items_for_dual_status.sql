DROP TABLE IF EXISTS queue_items;

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
  sort_order INTEGER NOT NULL
);
