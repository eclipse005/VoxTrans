-- Per-task frozen settings: fields that should NOT change mid-run for a
-- task even if the user edits saved settings after enqueuing. Real-time
-- fields (LLM endpoint/model/concurrency, ASR/align model, chunk length)
-- are intentionally NOT frozen -- they continue to read live from the
-- settings row so the user can swap them and have running tasks pick up
-- the change on the next call/chunk.
ALTER TABLE tasks ADD COLUMN subtitle_length_preset TEXT NOT NULL DEFAULT '';
ALTER TABLE tasks ADD COLUMN enable_terminology INTEGER NOT NULL DEFAULT 1;
ALTER TABLE tasks ADD COLUMN enable_subtitle_beautify INTEGER NOT NULL DEFAULT 1;
ALTER TABLE tasks ADD COLUMN terminology_groups_json TEXT NOT NULL DEFAULT '[]';
