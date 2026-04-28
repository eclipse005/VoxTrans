use std::collections::HashSet;

use super::alignment_repair_numbers::shared_number_count;
use super::numbers::{extract_numbers, numeric_alignment_penalty};
use super::text_utils::normalize_inline_text;
use super::translation_candidate::{
    is_unusable_translation, sanitize_translation_candidate, trim_before_leaked_number_anchor,
};

pub(super) fn repair_neighbor_number_leaks(
    out: &mut [String],
    source_numbers_by_part: &[HashSet<String>],
    aligned_numbers_by_part: &[HashSet<String>],
    fallback: &[String],
) {
    if out.len() < 2 {
        return;
    }

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
        let mut right_text =
            sanitize_translation_candidate(out.get(index + 1).map(String::as_str).unwrap_or(""));

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
                    if let Some(trimmed) = trim_before_leaked_number_anchor(&left_text, &remaining)
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
                    if let Some(trimmed) = trim_before_leaked_number_anchor(&right_text, &remaining)
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
            let leaked_to_right = shared_number_count(&right_numbers, &left_source_numbers).max(
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
