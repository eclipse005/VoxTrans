ALTER TABLE task_runs ADD COLUMN transcribe_status TEXT NOT NULL DEFAULT 'pending';
ALTER TABLE task_runs ADD COLUMN transcribe_progress INTEGER NOT NULL DEFAULT 0;
ALTER TABLE task_runs ADD COLUMN transcribe_segment_current INTEGER NOT NULL DEFAULT 0;
ALTER TABLE task_runs ADD COLUMN transcribe_segment_total INTEGER NOT NULL DEFAULT 0;
ALTER TABLE task_runs ADD COLUMN transcribe_phase TEXT NOT NULL DEFAULT '';
ALTER TABLE task_runs ADD COLUMN transcribe_error TEXT NOT NULL DEFAULT '';
ALTER TABLE task_runs ADD COLUMN result_text TEXT NOT NULL DEFAULT '';
ALTER TABLE task_runs ADD COLUMN result_srt TEXT NOT NULL DEFAULT '';
ALTER TABLE task_runs ADD COLUMN subtitle_segments_json TEXT NOT NULL DEFAULT '[]';
ALTER TABLE task_runs ADD COLUMN sort_order INTEGER NOT NULL DEFAULT 0;

CREATE INDEX IF NOT EXISTS idx_task_runs_sort_order
  ON task_runs(sort_order ASC, updated_at DESC);
