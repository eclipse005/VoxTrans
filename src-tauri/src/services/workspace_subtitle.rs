use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSubtitleWord {
    pub start_ms: u64,
    pub end_ms: u64,
    pub word: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSubtitleSegment {
    pub start_ms: u64,
    pub end_ms: u64,
    pub source_text: String,
    pub translated_text: String,
    pub source_words: Vec<WorkspaceSubtitleWord>,
}

pub fn serialize_segments(segments: &[WorkspaceSubtitleSegment]) -> String {
    serde_json::to_string(segments).unwrap_or_else(|_| "[]".to_string())
}
