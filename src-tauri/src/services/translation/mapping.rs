use std::collections::HashMap;

use super::domain::{AlignedCue, SentenceUnit, SourceCue, TranslatedUnit};

pub fn cues_to_sentence_units(cues: &[SourceCue]) -> Vec<SentenceUnit> {
    cues.iter()
        .enumerate()
        .map(|(idx, cue)| SentenceUnit {
            // Keep LLM-facing IDs simple and contiguous.
            sentence_id: (idx + 1).to_string(),
            source_text: cue.source_text.clone(),
            cue_ids: vec![cue.cue_id.clone()],
        })
        .collect()
}

pub fn align_translated_units_to_cues(
    cues: &[SourceCue],
    translated_units: &[TranslatedUnit],
) -> Vec<AlignedCue> {
    let translated_map: HashMap<&str, &str> = translated_units
        .iter()
        .map(|u| (u.sentence_id.as_str(), u.translated_text.as_str()))
        .collect();

    cues.iter()
        .enumerate()
        .map(|(idx, cue)| {
            let sentence_id = (idx + 1).to_string();
            AlignedCue {
                cue_id: cue.cue_id.clone(),
                source_text: cue.source_text.clone(),
                translated_text: translated_map
                    .get(sentence_id.as_str())
                    .copied()
                    .unwrap_or_default()
                    .to_string(),
            }
        })
        .collect()
}
