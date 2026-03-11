ALTER TABLE tasks ADD COLUMN source_lang TEXT NOT NULL DEFAULT 'auto';
ALTER TABLE tasks ADD COLUMN target_lang TEXT NOT NULL DEFAULT '';

ALTER TABLE tasks ADD COLUMN transcribe_status TEXT NOT NULL DEFAULT 'pending';
ALTER TABLE tasks ADD COLUMN transcribe_error TEXT NOT NULL DEFAULT '';
ALTER TABLE tasks ADD COLUMN transcript_text TEXT NOT NULL DEFAULT '';
ALTER TABLE tasks ADD COLUMN transcript_srt TEXT NOT NULL DEFAULT '';
ALTER TABLE tasks ADD COLUMN transcribed_at INTEGER;

ALTER TABLE tasks ADD COLUMN translate_status TEXT NOT NULL DEFAULT 'idle';
ALTER TABLE tasks ADD COLUMN translate_error TEXT NOT NULL DEFAULT '';
ALTER TABLE tasks ADD COLUMN translated_text TEXT NOT NULL DEFAULT '';
ALTER TABLE tasks ADD COLUMN translated_srt TEXT NOT NULL DEFAULT '';
ALTER TABLE tasks ADD COLUMN translated_srt_path TEXT NOT NULL DEFAULT '';
ALTER TABLE tasks ADD COLUMN translated_segments_json TEXT NOT NULL DEFAULT '';
ALTER TABLE tasks ADD COLUMN translate_model TEXT NOT NULL DEFAULT '';
ALTER TABLE tasks ADD COLUMN translated_at INTEGER;

CREATE INDEX IF NOT EXISTS idx_tasks_updated_at ON tasks(updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_tasks_transcribe_status ON tasks(transcribe_status, updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_tasks_translate_status ON tasks(translate_status, updated_at DESC);
