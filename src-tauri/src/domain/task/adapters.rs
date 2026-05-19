use crate::commands::translate_types::{
    BuildTranslationSegmentCommand, SegmentTokenForTerminologyCommand,
    SourceSegmentForTerminologyCommand, Step5AlignedParentCommand, Step5SplitParentCommand,
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

pub fn workspace_subtitle_segments_from_step51_parents(
    parents: &[Step5SplitParentCommand],
) -> Vec<WorkspaceSubtitleSegment> {
    let mut segments = Vec::new();
    for parent in parents {
        for part in &parent.parts {
            segments.push(WorkspaceSubtitleSegment {
                start_ms: seconds_to_millis(part.start),
                end_ms: seconds_to_millis(part.end),
                source_text: part.source.clone(),
                translated_text: String::new(),
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
