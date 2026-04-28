use std::collections::HashSet;

use super::alignment_repair_numbers::{
    neighbor_source_numbers, repair_missing_numbers_across_parts, shared_number_count,
};
use super::numbers::{extract_numbers, numeric_alignment_penalty};
use super::quality::{line_signatures, signature_counts, split_line_quality_score};
use super::source_residue::looks_like_source_residue;
use super::text_utils::normalize_inline_text;
use super::translation_candidate::{
    has_tail_ellipsis, is_unusable_translation, leading_number_anchor,
    sanitize_translation_candidate, strip_leading_number_token, strip_tail_ellipsis,
    trim_before_leaked_number_anchor,
};
use super::types::Step5SplitParent;
use super::watchability::repair_watchability_lines;

fn looks_like_full_parent_copy(text: &str, parent_draft: &str) -> bool {
    let normalized = normalize_inline_text(text);
    let draft = normalize_inline_text(parent_draft);
    if normalized.is_empty() || draft.is_empty() {
        return false;
    }
    let normalized_len = normalized.chars().count();
    let draft_len = draft.chars().count();
    if normalized_len < 12 || draft_len < 12 {
        return false;
    }
    let shorter = normalized_len.min(draft_len) as f64;
    let longer = normalized_len.max(draft_len) as f64;
    if shorter / longer < 0.82 {
        return false;
    }
    normalized.contains(&draft) || draft.contains(&normalized)
}

pub(super) fn repair_aligned_lines(
    parent: &Step5SplitParent,
    aligned: &[String],
    fallback: &[String],
    target_lang: &str,
) -> Vec<String> {
    let mut out = Vec::<String>::with_capacity(parent.parts.len());
    let source_numbers_by_part = parent
        .parts
        .iter()
        .map(|part| extract_numbers(&part.source))
        .collect::<Vec<_>>();
    let aligned_numbers_by_part = aligned
        .iter()
        .map(|line| extract_numbers(line))
        .collect::<Vec<_>>();
    let aligned_signatures = line_signatures(aligned);
    let fallback_signatures = line_signatures(fallback);
    let aligned_signature_counts = signature_counts(&aligned_signatures);
    let parent_draft = normalize_inline_text(&parent.draft_translation);

    for (index, part) in parent.parts.iter().enumerate() {
        let source_numbers = source_numbers_by_part
            .get(index)
            .cloned()
            .unwrap_or_default();
        let fallback_text = fallback
            .get(index)
            .map(|value| sanitize_translation_candidate(value))
            .unwrap_or_default();
        let mut text = aligned
            .get(index)
            .map(|value| sanitize_translation_candidate(value))
            .unwrap_or_default();
        let signature = aligned_signatures.get(index).cloned().unwrap_or_default();
        let is_duplicate_line = signature.len() >= 6
            && aligned_signature_counts
                .get(&signature)
                .copied()
                .unwrap_or(0)
                >= 2
            && signature != fallback_signatures.get(index).cloned().unwrap_or_default();
        if is_duplicate_line
            && !is_unusable_translation(&fallback_text)
            && !has_tail_ellipsis(&fallback_text)
        {
            text = fallback_text.clone();
        }
        if parent.parts.len() > 1
            && looks_like_full_parent_copy(&text, &parent_draft)
            && !is_unusable_translation(&fallback_text)
            && !looks_like_full_parent_copy(&fallback_text, &parent_draft)
        {
            text = fallback_text.clone();
        }
        if !source_numbers.is_empty() && !is_unusable_translation(&fallback_text) {
            let current_penalty = numeric_alignment_penalty(&source_numbers, &text);
            let fallback_penalty = numeric_alignment_penalty(&source_numbers, &fallback_text);
            if fallback_penalty < current_penalty {
                text = fallback_text.clone();
            }
        }
        if let Some(leading_anchor) = leading_number_anchor(&part.source) {
            let text_numbers = extract_numbers(&text);
            if !text_numbers.contains(&leading_anchor) {
                let fallback_numbers = extract_numbers(&fallback_text);
                if fallback_numbers.contains(&leading_anchor)
                    && !is_unusable_translation(&fallback_text)
                {
                    text = fallback_text.clone();
                }
            }
            let text_numbers_after = extract_numbers(&text);
            if !text_numbers_after.contains(&leading_anchor) && !text.is_empty() {
                text = sanitize_translation_candidate(&format!("{leading_anchor} {text}"));
            }
        }
        if let Some(text_leading_number) = leading_number_anchor(&text) {
            let source_leading_number = leading_number_anchor(&part.source);
            let source_matches_leading = source_leading_number
                .as_ref()
                .map(|value| value == &text_leading_number)
                .unwrap_or(false);
            if !source_matches_leading {
                let fallback_leading_number = leading_number_anchor(&fallback_text);
                let fallback_matches_source = source_leading_number
                    .as_ref()
                    .map(|value| fallback_leading_number.as_ref() == Some(value))
                    .unwrap_or(fallback_leading_number.is_none());
                if fallback_matches_source && !is_unusable_translation(&fallback_text) {
                    text = fallback_text.clone();
                } else {
                    let stripped = strip_leading_number_token(&text);
                    if !stripped.is_empty() {
                        text = stripped;
                    }
                }
            }
        }
        let text_numbers = extract_numbers(&text);
        if source_numbers.is_empty() && !text_numbers.is_empty() {
            let neighbor_numbers = neighbor_source_numbers(&source_numbers_by_part, index);
            let text_neighbor_hits = shared_number_count(&text_numbers, &neighbor_numbers);
            if text_neighbor_hits > 0 {
                let fallback_numbers = extract_numbers(&fallback_text);
                let fallback_hits = shared_number_count(&fallback_numbers, &neighbor_numbers);
                if fallback_hits < text_neighbor_hits && !is_unusable_translation(&fallback_text) {
                    text = fallback_text.clone();
                }
                let text_numbers_after = extract_numbers(&text);
                let leaked_numbers = text_numbers_after
                    .iter()
                    .filter(|value| neighbor_numbers.contains(*value))
                    .cloned()
                    .collect::<HashSet<_>>();
                if !leaked_numbers.is_empty() {
                    if let Some(trimmed) = trim_before_leaked_number_anchor(&text, &leaked_numbers)
                    {
                        text = trimmed;
                    } else if let Some(leading) = leading_number_anchor(&text) {
                        if leaked_numbers.contains(&leading) {
                            let stripped = strip_leading_number_token(&text);
                            if !stripped.is_empty() {
                                text = stripped;
                            }
                        }
                    }
                }
            }
        }
        if looks_like_source_residue(&part.source, &text, target_lang)
            && !looks_like_source_residue(&part.source, &fallback_text, target_lang)
            && !is_unusable_translation(&fallback_text)
        {
            text = fallback_text.clone();
        }
        if is_unusable_translation(&text) {
            if !is_unusable_translation(&fallback_text) {
                text = fallback_text;
            }
        }
        if is_unusable_translation(&text) {
            text = normalize_inline_text(&part.source);
        }
        if has_tail_ellipsis(&text) {
            let trimmed = strip_tail_ellipsis(&text);
            if !trimmed.is_empty() {
                text = trimmed;
            }
        }
        if is_unusable_translation(&text) {
            text = "[缺失译文]".to_string();
        }
        out.push(text);
    }

    let out_signatures = line_signatures(&out);
    let out_signature_counts = signature_counts(&out_signatures);
    let fallback_score = split_line_quality_score(fallback);
    let out_score = split_line_quality_score(&out);
    if fallback_score > out_score {
        for (index, text) in out.iter_mut().enumerate() {
            let signature = out_signatures.get(index).cloned().unwrap_or_default();
            let is_duplicate_line = signature.len() >= 6
                && out_signature_counts.get(&signature).copied().unwrap_or(0) >= 2;
            if !is_duplicate_line {
                continue;
            }
            let fallback_text = fallback
                .get(index)
                .map(|value| normalize_inline_text(value))
                .unwrap_or_default();
            if !is_unusable_translation(&fallback_text) {
                *text = fallback_text;
            }
        }
    }

    if out.len() >= 2 {
        for index in 0..(out.len() - 1) {
            let left_source_numbers = source_numbers_by_part
                .get(index)
                .cloned()
                .unwrap_or_default();
            let right_source_numbers = source_numbers_by_part
                .get(index + 1)
                .cloned()
                .unwrap_or_default();

            let mut left_text =
                sanitize_translation_candidate(out.get(index).map(String::as_str).unwrap_or(""));
            let mut right_text = sanitize_translation_candidate(
                out.get(index + 1).map(String::as_str).unwrap_or(""),
            );

            let left_numbers = extract_numbers(&left_text);
            let right_numbers = extract_numbers(&right_text);
            let left_aligned_numbers = aligned_numbers_by_part
                .get(index)
                .cloned()
                .unwrap_or_default();
            let right_aligned_numbers = aligned_numbers_by_part
                .get(index + 1)
                .cloned()
                .unwrap_or_default();

            if !left_source_numbers.is_empty() && !right_source_numbers.is_empty() {
                let left_numbers_now = extract_numbers(&left_text);
                let right_numbers_now = extract_numbers(&right_text);
                let leaked_from_left = left_numbers_now
                    .iter()
                    .filter(|value| right_source_numbers.contains(*value))
                    .filter(|value| !left_source_numbers.contains(*value))
                    .cloned()
                    .collect::<HashSet<_>>();
                let leaked_from_right = right_numbers_now
                    .iter()
                    .filter(|value| left_source_numbers.contains(*value))
                    .filter(|value| !right_source_numbers.contains(*value))
                    .cloned()
                    .collect::<HashSet<_>>();

                let missing_on_right = right_source_numbers
                    .iter()
                    .filter(|value| !right_numbers_now.contains(*value))
                    .count();
                if !leaked_from_left.is_empty() && missing_on_right == 0 {
                    let left_fallback = fallback
                        .get(index)
                        .map(|value| sanitize_translation_candidate(value))
                        .unwrap_or_default();
                    if !is_unusable_translation(&left_fallback) {
                        let fallback_numbers = extract_numbers(&left_fallback);
                        let fallback_leak = leaked_from_left
                            .iter()
                            .filter(|value| fallback_numbers.contains(*value))
                            .count();
                        let current_leak = leaked_from_left
                            .iter()
                            .filter(|value| left_numbers_now.contains(*value))
                            .count();
                        if fallback_leak < current_leak {
                            left_text = left_fallback;
                        }
                    }
                    let left_numbers_after = extract_numbers(&left_text);
                    let remaining = leaked_from_left
                        .iter()
                        .filter(|value| left_numbers_after.contains(*value))
                        .cloned()
                        .collect::<HashSet<_>>();
                    if !remaining.is_empty() {
                        if let Some(trimmed) =
                            trim_before_leaked_number_anchor(&left_text, &remaining)
                        {
                            left_text = trimmed;
                        }
                    }
                }

                let missing_on_left = left_source_numbers
                    .iter()
                    .filter(|value| !left_numbers_now.contains(*value))
                    .count();
                if !leaked_from_right.is_empty() && missing_on_left == 0 {
                    let right_fallback = fallback
                        .get(index + 1)
                        .map(|value| sanitize_translation_candidate(value))
                        .unwrap_or_default();
                    if !is_unusable_translation(&right_fallback) {
                        let fallback_numbers = extract_numbers(&right_fallback);
                        let fallback_leak = leaked_from_right
                            .iter()
                            .filter(|value| fallback_numbers.contains(*value))
                            .count();
                        let current_leak = leaked_from_right
                            .iter()
                            .filter(|value| right_numbers_now.contains(*value))
                            .count();
                        if fallback_leak < current_leak {
                            right_text = right_fallback;
                        }
                    }
                    let right_numbers_after = extract_numbers(&right_text);
                    let remaining = leaked_from_right
                        .iter()
                        .filter(|value| right_numbers_after.contains(*value))
                        .cloned()
                        .collect::<HashSet<_>>();
                    if !remaining.is_empty() {
                        if let Some(trimmed) =
                            trim_before_leaked_number_anchor(&right_text, &remaining)
                        {
                            right_text = trimmed;
                        }
                    }
                }
            }

            if left_source_numbers.is_empty() && !right_source_numbers.is_empty() {
                let leaked_to_left = shared_number_count(&left_numbers, &right_source_numbers).max(
                    shared_number_count(&left_aligned_numbers, &right_source_numbers),
                );
                let missing_on_right = right_source_numbers
                    .iter()
                    .filter(|value| !right_numbers.contains(*value))
                    .count();
                if leaked_to_left > 0 && missing_on_right > 0 {
                    let left_fallback = fallback
                        .get(index)
                        .map(|value| normalize_inline_text(value))
                        .unwrap_or_default();
                    let right_fallback = fallback
                        .get(index + 1)
                        .map(|value| normalize_inline_text(value))
                        .unwrap_or_default();
                    if !is_unusable_translation(&right_fallback)
                        && numeric_alignment_penalty(&right_source_numbers, &right_fallback)
                            < numeric_alignment_penalty(&right_source_numbers, &right_text)
                    {
                        right_text = right_fallback;
                    }
                    if !is_unusable_translation(&left_fallback) {
                        let left_fallback_numbers = extract_numbers(&left_fallback);
                        let fallback_leak =
                            shared_number_count(&left_fallback_numbers, &right_source_numbers);
                        if fallback_leak < leaked_to_left {
                            left_text = left_fallback;
                        }
                    }
                    let right_numbers_after = extract_numbers(&right_text);
                    let mut remaining_missing = right_source_numbers
                        .iter()
                        .filter(|value| !right_numbers_after.contains(*value))
                        .filter(|value| {
                            left_numbers.contains(*value) || left_aligned_numbers.contains(*value)
                        })
                        .cloned()
                        .collect::<Vec<_>>();
                    if !remaining_missing.is_empty() && !right_text.is_empty() {
                        remaining_missing.sort();
                        let prefix = remaining_missing.join("/");
                        right_text = normalize_inline_text(&format!("{prefix} {right_text}"));
                    }
                }
                if leaked_to_left > 0 && missing_on_right == 0 {
                    let left_fallback = fallback
                        .get(index)
                        .map(|value| sanitize_translation_candidate(value))
                        .unwrap_or_default();
                    if !is_unusable_translation(&left_fallback) {
                        let left_fallback_numbers = extract_numbers(&left_fallback);
                        let fallback_leak =
                            shared_number_count(&left_fallback_numbers, &right_source_numbers);
                        if fallback_leak < leaked_to_left {
                            left_text = left_fallback;
                        }
                    }
                    let left_numbers_after = extract_numbers(&left_text);
                    let remaining_leak =
                        shared_number_count(&left_numbers_after, &right_source_numbers);
                    if remaining_leak > 0 {
                        let trim_numbers = left_numbers_after
                            .iter()
                            .filter(|value| right_source_numbers.contains(*value))
                            .cloned()
                            .collect::<HashSet<_>>();
                        if let Some(trimmed) =
                            trim_before_leaked_number_anchor(&left_text, &trim_numbers)
                        {
                            left_text = trimmed;
                        }
                    }
                }
            }

            if right_source_numbers.is_empty() && !left_source_numbers.is_empty() {
                let leaked_to_right =
                    shared_number_count(&right_numbers, &left_source_numbers).max(
                        shared_number_count(&right_aligned_numbers, &left_source_numbers),
                    );
                let missing_on_left = left_source_numbers
                    .iter()
                    .filter(|value| !left_numbers.contains(*value))
                    .count();
                if leaked_to_right > 0 && missing_on_left > 0 {
                    let left_fallback = fallback
                        .get(index)
                        .map(|value| normalize_inline_text(value))
                        .unwrap_or_default();
                    let right_fallback = fallback
                        .get(index + 1)
                        .map(|value| normalize_inline_text(value))
                        .unwrap_or_default();
                    if !is_unusable_translation(&left_fallback)
                        && numeric_alignment_penalty(&left_source_numbers, &left_fallback)
                            < numeric_alignment_penalty(&left_source_numbers, &left_text)
                    {
                        left_text = left_fallback;
                    }
                    if !is_unusable_translation(&right_fallback) {
                        let right_fallback_numbers = extract_numbers(&right_fallback);
                        let fallback_leak =
                            shared_number_count(&right_fallback_numbers, &left_source_numbers);
                        if fallback_leak < leaked_to_right {
                            right_text = right_fallback;
                        }
                    }
                    let left_numbers_after = extract_numbers(&left_text);
                    let mut remaining_missing = left_source_numbers
                        .iter()
                        .filter(|value| !left_numbers_after.contains(*value))
                        .filter(|value| {
                            right_numbers.contains(*value) || right_aligned_numbers.contains(*value)
                        })
                        .cloned()
                        .collect::<Vec<_>>();
                    if !remaining_missing.is_empty() && !left_text.is_empty() {
                        remaining_missing.sort();
                        let prefix = remaining_missing.join("/");
                        left_text = normalize_inline_text(&format!("{prefix} {left_text}"));
                    }
                }
            }

            out[index] = left_text;
            out[index + 1] = right_text;
        }
    }

    repair_missing_numbers_across_parts(
        &mut out,
        &source_numbers_by_part,
        &aligned_numbers_by_part,
        fallback,
    );

    let source_lines = parent
        .parts
        .iter()
        .map(|part| part.source.clone())
        .collect::<Vec<_>>();
    repair_watchability_lines(&source_lines, &mut out, target_lang);
    out
}
