use super::domain::AlignedCue;

pub fn assert_source_immutable(before: &[AlignedCue], after: &[AlignedCue]) -> Result<(), String> {
    if before.len() != after.len() {
        return Err("source immutability check failed: cue length changed".to_string());
    }

    for (prev, next) in before.iter().zip(after.iter()) {
        if prev.cue_id != next.cue_id {
            return Err(format!(
                "source immutability check failed: cue id mismatch {} != {}",
                prev.cue_id, next.cue_id
            ));
        }
        if prev.source_text != next.source_text {
            return Err(format!(
                "source immutability check failed: source text changed at {}",
                prev.cue_id
            ));
        }
    }
    Ok(())
}
