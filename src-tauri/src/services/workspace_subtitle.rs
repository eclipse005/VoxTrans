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
    /// Semantic-sentence id from the boundary stage (0 = none: imported cues,
    /// legacy artifacts). Travels with the SoT — the translation stage rebuilds
    /// sentence-level translation units from the SoT, not from raw step2
    /// segments — so the round-trip through the editor JSON must preserve it.
    #[serde(default)]
    pub semantic_sentence_id: usize,
}

pub fn serialize_segments(segments: &[WorkspaceSubtitleSegment]) -> String {
    serde_json::to_string(segments).unwrap_or_else(|_| "[]".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_sot_json_without_sid_loads_as_zero() {
        // SoT JSON written before semantic_sentence_id existed must still
        // deserialize — grouping degrades to per-cue units, never errors.
        let legacy = r#"[{"startMs":0,"endMs":1000,"sourceText":"hi","translatedText":"","sourceWords":[]}]"#;
        let segs: Vec<WorkspaceSubtitleSegment> = serde_json::from_str(legacy).unwrap();
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].semantic_sentence_id, 0);
    }

    #[test]
    fn sid_survives_sot_roundtrip() {
        let segs = vec![WorkspaceSubtitleSegment {
            start_ms: 0,
            end_ms: 1000,
            source_text: "a".into(),
            translated_text: String::new(),
            source_words: vec![],
            semantic_sentence_id: 7,
        }];
        let json = serialize_segments(&segs);
        assert!(json.contains("semanticSentenceId\":7"));
        let back: Vec<WorkspaceSubtitleSegment> = serde_json::from_str(&json).unwrap();
        assert_eq!(back[0].semantic_sentence_id, 7);
    }
}
