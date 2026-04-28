use super::constants::WATCHABILITY_SPLIT_TRIGGER;
use super::polish_repair::repair_polished_translation;
use super::source_residue::looks_like_non_cjk_translation_for_cjk_target;
use super::types::Step5FinalSegment;
use super::watchability::{apply_residual_watchability_overrides, repair_watchability_fragments};
use super::watchability_split::split_watchability_overlong_segments;

pub(super) fn postprocess_polished_segments(
    segments: &mut Vec<Step5FinalSegment>,
    baseline_translations: &[String],
    target_lang: &str,
) {
    for segment in segments.iter_mut() {
        repair_polished_translation(segment);
    }
    repair_watchability_fragments(segments, target_lang);
    for segment in segments.iter_mut() {
        repair_polished_translation(segment);
    }
    apply_residual_watchability_overrides(segments, target_lang);
    for (index, segment) in segments.iter_mut().enumerate() {
        if !looks_like_non_cjk_translation_for_cjk_target(&segment.translation, target_lang) {
            continue;
        }
        let fallback = baseline_translations
            .get(index)
            .cloned()
            .unwrap_or_default();
        if fallback.is_empty() {
            continue;
        }
        segment.translation = fallback;
        repair_polished_translation(segment);
    }
    split_watchability_overlong_segments(segments, WATCHABILITY_SPLIT_TRIGGER, target_lang);
    for segment in segments.iter_mut() {
        repair_polished_translation(segment);
    }
    repair_watchability_fragments(segments, target_lang);
    apply_residual_watchability_overrides(segments, target_lang);
}
