#[cfg(test)]
use super::translate_terms::{count_source_tokens, normalize_command_terminology_entries};

pub use super::translate_types::*;

pub(crate) fn is_tail_ellipsis(text: &str) -> bool {
    let trimmed = text.trim_end();
    trimmed.ends_with("...") || trimmed.ends_with('…')
}

#[cfg(test)]
mod tests {
    use super::{
        TranslateTerminologyEntryCommand, count_source_tokens,
        normalize_command_terminology_entries,
    };

    #[test]
    fn normalize_terminology_deduplicates_and_trims() {
        let normalized = normalize_command_terminology_entries(vec![
            TranslateTerminologyEntryCommand {
                source: "  NATO ".to_string(),
                target: "北约".to_string(),
                note: "a".to_string(),
            },
            TranslateTerminologyEntryCommand {
                source: "nato".to_string(),
                target: " 北约 ".to_string(),
                note: "b".to_string(),
            },
            TranslateTerminologyEntryCommand {
                source: "EU".to_string(),
                target: "欧盟".to_string(),
                note: " ".to_string(),
            },
        ]);

        assert_eq!(normalized.len(), 2);
        assert_eq!(normalized[0].source, "NATO");
        assert_eq!(normalized[1].source, "EU");
    }

    #[test]
    fn source_token_count_uses_segment_tokens() {
        let segments = vec![
            super::SourceSegmentForTerminologyCommand {
                segment: "Hello world".to_string(),
                start: 0.0,
                end: 1.2,
                tokens: vec![
                    super::SegmentTokenForTerminologyCommand {
                        text: "Hello".to_string(),
                        start: 0.0,
                        end: 0.5,
                    },
                    super::SegmentTokenForTerminologyCommand {
                        text: "world".to_string(),
                        start: 0.5,
                        end: 1.2,
                    },
                ],
            },
            super::SourceSegmentForTerminologyCommand {
                segment: "你好世界".to_string(),
                start: 1.2,
                end: 2.0,
                tokens: Vec::new(),
            },
        ];
        assert_eq!(count_source_tokens(&segments), 3);
    }
}
