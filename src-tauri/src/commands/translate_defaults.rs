pub fn default_llm_concurrency() -> u32 {
    4
}

pub fn default_batch_size() -> usize {
    20
}

pub fn default_subtitle_max_words_per_segment() -> u32 {
    20
}

pub fn default_subtitle_length_reference() -> u32 {
    28
}

pub fn step5_schema_version() -> u32 {
    2
}

pub fn step5_pipeline_version() -> &'static str {
    "step5.v2"
}
