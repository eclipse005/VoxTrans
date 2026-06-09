use crate::commands::translate_types::{
    BuildTranslationSegmentCommand, SegmentTokenForTerminologyCommand,
    SourceSegmentForTerminologyCommand, Step5AlignedParentCommand,
};
use crate::services::workspace_subtitle::{WorkspaceSubtitleSegment, WorkspaceSubtitleWord};

pub fn workspace_subtitle_segments_from_step2_segments(
    segments: &[crate::commands::transcription::GroupedSentenceSegmentCommandDto],
) -> Vec<WorkspaceSubtitleSegment> {
    segments
        .iter()
        .map(|segment| WorkspaceSubtitleSegment {
            start_ms: seconds_to_millis(segment.start),
            end_ms: seconds_to_millis(segment.end),
            source_text: segment.segment.clone(),
            translated_text: String::new(),
            source_words: segment
                .tokens
                .iter()
                .map(|token| WorkspaceSubtitleWord {
                    start_ms: seconds_to_millis(token.start),
                    end_ms: seconds_to_millis(token.end),
                    word: token.text.clone(),
                })
                .collect(),
        })
        .collect()
}

pub fn workspace_subtitle_segments_from_translation_segments(
    segments: &[BuildTranslationSegmentCommand],
) -> Vec<WorkspaceSubtitleSegment> {
    segments
        .iter()
        .map(|segment| WorkspaceSubtitleSegment {
            start_ms: seconds_to_millis(segment.start),
            end_ms: seconds_to_millis(segment.end),
            source_text: segment.source.clone(),
            translated_text: segment.translation.clone(),
            source_words: segment
                .tokens
                .iter()
                .map(|token| WorkspaceSubtitleWord {
                    start_ms: seconds_to_millis(token.start),
                    end_ms: seconds_to_millis(token.end),
                    word: token.text.clone(),
                })
                .collect(),
        })
        .collect()
}

pub fn workspace_subtitle_segments_from_step52_parents(
    parents: &[Step5AlignedParentCommand],
) -> Vec<WorkspaceSubtitleSegment> {
    let mut segments = Vec::new();
    for parent in parents {
        for part in &parent.parts {
            segments.push(WorkspaceSubtitleSegment {
                start_ms: seconds_to_millis(part.start),
                end_ms: seconds_to_millis(part.end),
                source_text: part.source.clone(),
                translated_text: part.translation.clone(),
                source_words: part
                    .tokens
                    .iter()
                    .map(|token| WorkspaceSubtitleWord {
                        start_ms: seconds_to_millis(token.start),
                        end_ms: seconds_to_millis(token.end),
                        word: token.text.clone(),
                    })
                    .collect(),
            });
        }
    }
    segments
}

pub fn translation_segments_from_step52_parents(
    parents: &[Step5AlignedParentCommand],
) -> Vec<BuildTranslationSegmentCommand> {
    let mut segments = Vec::new();
    for parent in parents {
        for part in &parent.parts {
            segments.push(BuildTranslationSegmentCommand {
                segment_id: segments.len() + 1,
                start: part.start,
                end: part.end,
                source: part.source.clone(),
                translation: part.translation.clone(),
                tokens: part.tokens.clone(),
            });
        }
    }
    segments
}

pub fn source_text_from_step2_segments(
    segments: &[crate::commands::transcription::GroupedSentenceSegmentCommandDto],
) -> String {
    segments
        .iter()
        .map(|segment| segment.segment.trim())
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn step2_segments_to_srt(
    segments: &[crate::commands::transcription::GroupedSentenceSegmentCommandDto],
) -> String {
    let mut out = String::new();
    for (index, segment) in segments.iter().enumerate() {
        let start_ms = seconds_to_millis(segment.start);
        let end_ms = seconds_to_millis(segment.end.max(segment.start));
        out.push_str(&(index + 1).to_string());
        out.push('\n');
        out.push_str(&format!(
            "{} --> {}\n",
            format_srt_ms(start_ms),
            format_srt_ms(end_ms)
        ));
        out.push_str(segment.segment.trim());
        out.push_str("\n\n");
    }
    out.trim_end().to_string()
}

pub fn map_step2_segments_for_translate(
    segments: &[crate::commands::transcription::GroupedSentenceSegmentCommandDto],
) -> Vec<SourceSegmentForTerminologyCommand> {
    segments
        .iter()
        .map(|segment| SourceSegmentForTerminologyCommand {
            segment: segment.segment.clone(),
            start: segment.start,
            end: segment.end,
            tokens: segment
                .tokens
                .iter()
                .map(|token| SegmentTokenForTerminologyCommand {
                    text: token.text.clone(),
                    start: token.start,
                    end: token.end,
                })
                .collect(),
        })
        .collect()
}

fn format_srt_ms(total_ms: u64) -> String {
    let hours = total_ms / 3_600_000;
    let minutes = (total_ms % 3_600_000) / 60_000;
    let seconds = (total_ms % 60_000) / 1000;
    let millis = total_ms % 1000;
    format!("{hours:02}:{minutes:02}:{seconds:02},{millis:03}")
}

fn seconds_to_millis(value: f64) -> u64 {
    if !value.is_finite() || value <= 0.0 {
        return 0;
    }
    (value * 1000.0).round() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seconds_to_millis_zero() {
        assert_eq!(seconds_to_millis(0.0), 0);
    }

    #[test]
    fn seconds_to_millis_negative() {
        assert_eq!(seconds_to_millis(-1.5), 0);
    }

    #[test]
    fn seconds_to_millis_nan() {
        assert_eq!(seconds_to_millis(f64::NAN), 0);
    }

    #[test]
    fn seconds_to_millis_rounding() {
        assert_eq!(seconds_to_millis(1.0), 1000);
        assert_eq!(seconds_to_millis(1.2345), 1235);
        assert_eq!(seconds_to_millis(0.5), 500);
    }

    #[test]
    fn format_srt_ms_basic() {
        assert_eq!(format_srt_ms(0), "00:00:00,000");
        assert_eq!(format_srt_ms(1000), "00:00:01,000");
        assert_eq!(format_srt_ms(61_234), "00:01:01,234");
        assert_eq!(format_srt_ms(3_661_001), "01:01:01,001");
    }

    #[test]
    fn source_text_from_step2_segments_joins_non_empty() {
        let segments = vec![
            crate::commands::transcription::GroupedSentenceSegmentCommandDto {
                segment: "Hello".to_string(),
                start: 0.0,
                end: 1.0,
                tokens: vec![],
            },
            crate::commands::transcription::GroupedSentenceSegmentCommandDto {
                segment: "World".to_string(),
                start: 1.0,
                end: 2.0,
                tokens: vec![],
            },
        ];
        assert_eq!(source_text_from_step2_segments(&segments), "Hello\nWorld");
    }

    #[test]
    fn source_text_from_step2_segments_skips_empty() {
        let segments = vec![
            crate::commands::transcription::GroupedSentenceSegmentCommandDto {
                segment: "Hello".to_string(),
                start: 0.0,
                end: 1.0,
                tokens: vec![],
            },
            crate::commands::transcription::GroupedSentenceSegmentCommandDto {
                segment: "  ".to_string(),
                start: 1.0,
                end: 2.0,
                tokens: vec![],
            },
            crate::commands::transcription::GroupedSentenceSegmentCommandDto {
                segment: "World".to_string(),
                start: 2.0,
                end: 3.0,
                tokens: vec![],
            },
        ];
        assert_eq!(source_text_from_step2_segments(&segments), "Hello\nWorld");
    }

    #[test]
    fn step2_segments_to_srt_basic() {
        let segments = vec![
            crate::commands::transcription::GroupedSentenceSegmentCommandDto {
                segment: "Hello".to_string(),
                start: 0.0,
                end: 1.5,
                tokens: vec![],
            },
        ];
        let srt = step2_segments_to_srt(&segments);
        assert!(srt.contains("1\n"));
        assert!(srt.contains("00:00:00,000 --> 00:00:01,500\n"));
        assert!(srt.contains("Hello"));
    }

    #[test]
    fn workspace_subtitle_segments_from_step2_segments_maps_fields() {
        let segments = vec![
            crate::commands::transcription::GroupedSentenceSegmentCommandDto {
                segment: "Test".to_string(),
                start: 1.0,
                end: 2.5,
                tokens: vec![
                    crate::commands::transcription::GroupedSentenceTokenCommandDto {
                        text: "token1".to_string(),
                        start: 1.0,
                        end: 1.5,
                    },
                ],
            },
        ];
        let result = workspace_subtitle_segments_from_step2_segments(&segments);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].start_ms, 1000);
        assert_eq!(result[0].end_ms, 2500);
        assert_eq!(result[0].source_text, "Test");
        assert_eq!(result[0].source_words.len(), 1);
        assert_eq!(result[0].source_words[0].word, "token1");
    }
}
