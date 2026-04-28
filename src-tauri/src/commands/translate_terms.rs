use std::collections::HashSet;

use super::translate::{SourceSegmentForTerminologyCommand, TranslateTerminologyEntryCommand};

pub fn normalize_command_terminology_entries(
    entries: Vec<TranslateTerminologyEntryCommand>,
) -> Vec<TranslateTerminologyEntryCommand> {
    let mut out = Vec::new();
    let mut seen = HashSet::<(String, String)>::new();
    for entry in entries {
        let source = entry.source.trim().to_string();
        let target = entry.target.trim().to_string();
        if source.is_empty() || target.is_empty() {
            continue;
        }
        let key = (source.to_ascii_lowercase(), target.to_ascii_lowercase());
        if !seen.insert(key) {
            continue;
        }
        out.push(TranslateTerminologyEntryCommand {
            source,
            target,
            note: entry.note.trim().to_string(),
        });
    }
    out
}

pub fn load_terminology_entries_from_saved_settings()
-> Result<Vec<TranslateTerminologyEntryCommand>, String> {
    let settings = crate::services::preferences::load_saved_settings_from_default_path()?;
    if !settings.enable_terminology {
        return Ok(Vec::new());
    }

    let terms = settings
        .terminology_groups
        .into_iter()
        .flat_map(|group| group.terms.into_iter())
        .map(|term| TranslateTerminologyEntryCommand {
            source: term.origin,
            target: term.target,
            note: term.note,
        })
        .collect::<Vec<_>>();
    Ok(normalize_command_terminology_entries(terms))
}

pub fn count_source_tokens(segments: &[SourceSegmentForTerminologyCommand]) -> usize {
    let mut total = 0usize;
    for segment in segments {
        if !segment.tokens.is_empty() {
            total += segment
                .tokens
                .iter()
                .filter(|token| !token.text.trim().is_empty())
                .count();
            continue;
        }
        if !segment.segment.trim().is_empty() {
            total += 1;
        }
    }
    total
}
