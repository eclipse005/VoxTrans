-- step5_split_align_results: Step 5 拆分+对齐 段级幂等结果
CREATE TABLE step5_split_align_results (
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    segment_index INTEGER NOT NULL,
    parent_json TEXT NOT NULL,
    PRIMARY KEY (task_id, segment_index)
);
