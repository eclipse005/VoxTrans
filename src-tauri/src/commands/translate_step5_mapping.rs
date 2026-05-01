use super::translate_types::{
    BuildTranslationSegmentCommand, SegmentTokenForTerminologyCommand, Step5AlignedParentCommand,
    Step5AlignedPartCommand, Step5SplitParentCommand, Step5SplitPartCommand,
    TranslateTerminologyEntryCommand,
};

pub(super) fn command_tokens_to_step5(
    tokens: &[SegmentTokenForTerminologyCommand],
) -> Vec<crate::services::subtitle_step5::Step5Token> {
    tokens
        .iter()
        .map(|token| crate::services::subtitle_step5::Step5Token {
            text: token.text.clone(),
            start: token.start,
            end: token.end,
        })
        .collect()
}

pub(super) fn step5_tokens_to_command(
    tokens: Vec<crate::services::subtitle_step5::Step5Token>,
) -> Vec<SegmentTokenForTerminologyCommand> {
    tokens
        .into_iter()
        .map(|token| SegmentTokenForTerminologyCommand {
            text: token.text,
            start: token.start,
            end: token.end,
        })
        .collect()
}

pub(super) fn command_terminology_to_step5(
    entries: &[TranslateTerminologyEntryCommand],
) -> Vec<crate::services::subtitle_step5::Step5TerminologyEntry> {
    entries
        .iter()
        .map(
            |entry| crate::services::subtitle_step5::Step5TerminologyEntry {
                source: entry.source.clone(),
                target: entry.target.clone(),
                note: entry.note.clone(),
            },
        )
        .collect()
}

pub(super) fn command_segments_to_step5_draft(
    segments: &[BuildTranslationSegmentCommand],
) -> Vec<crate::services::subtitle_step5::Step5DraftSegment> {
    segments
        .iter()
        .map(
            |segment| crate::services::subtitle_step5::Step5DraftSegment {
                segment_id: segment.segment_id,
                start: segment.start,
                end: segment.end,
                source: segment.source.clone(),
                draft_translation: segment.translation.clone(),
                tokens: command_tokens_to_step5(&segment.tokens),
            },
        )
        .collect()
}

pub(super) fn step5_split_parents_to_command(
    parents: Vec<crate::services::subtitle_step5::Step5SplitParent>,
) -> Vec<Step5SplitParentCommand> {
    parents
        .into_iter()
        .map(|parent| Step5SplitParentCommand {
            parent_segment_id: parent.parent_segment_id,
            draft_translation: parent.draft_translation,
            parts: parent
                .parts
                .into_iter()
                .map(|part| Step5SplitPartCommand {
                    part_id: part.part_id,
                    start: part.start,
                    end: part.end,
                    source: part.source,
                    tokens: step5_tokens_to_command(part.tokens),
                })
                .collect(),
        })
        .collect()
}

pub(super) fn command_split_parents_to_step5(
    parents: &[Step5SplitParentCommand],
) -> Vec<crate::services::subtitle_step5::Step5SplitParent> {
    parents
        .iter()
        .map(|parent| crate::services::subtitle_step5::Step5SplitParent {
            parent_segment_id: parent.parent_segment_id,
            draft_translation: parent.draft_translation.clone(),
            parts: parent
                .parts
                .iter()
                .map(|part| crate::services::subtitle_step5::Step5SplitPart {
                    part_id: part.part_id,
                    start: part.start,
                    end: part.end,
                    source: part.source.clone(),
                    tokens: command_tokens_to_step5(&part.tokens),
                })
                .collect(),
        })
        .collect()
}

pub(super) fn step5_aligned_parents_to_command(
    parents: Vec<crate::services::subtitle_step5::Step5AlignedParent>,
) -> Vec<Step5AlignedParentCommand> {
    parents
        .into_iter()
        .map(|parent| Step5AlignedParentCommand {
            parent_segment_id: parent.parent_segment_id,
            parts: parent
                .parts
                .into_iter()
                .map(|part| Step5AlignedPartCommand {
                    part_id: part.part_id,
                    start: part.start,
                    end: part.end,
                    source: part.source,
                    translation: part.translation,
                    tokens: step5_tokens_to_command(part.tokens),
                })
                .collect(),
        })
        .collect()
}
