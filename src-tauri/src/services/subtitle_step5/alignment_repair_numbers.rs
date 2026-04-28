use std::collections::HashSet;

use super::numbers::extract_numbers;
use super::translation_candidate::{
    is_unusable_translation, prepend_missing_number_token, sanitize_translation_candidate,
};

pub(super) fn shared_number_count(left: &HashSet<String>, right: &HashSet<String>) -> usize {
    if left.is_empty() || right.is_empty() {
        return 0;
    }
    left.iter().filter(|value| right.contains(*value)).count()
}

pub(super) fn neighbor_source_numbers(
    source_numbers_by_part: &[HashSet<String>],
    index: usize,
) -> HashSet<String> {
    let mut out = HashSet::<String>::new();
    if index > 0 {
        for value in &source_numbers_by_part[index - 1] {
            out.insert(value.clone());
        }
    }
    if index + 1 < source_numbers_by_part.len() {
        for value in &source_numbers_by_part[index + 1] {
            out.insert(value.clone());
        }
    }
    out
}

pub(super) fn repair_missing_numbers_across_parts(
    out: &mut [String],
    source_numbers_by_part: &[HashSet<String>],
    aligned_numbers_by_part: &[HashSet<String>],
    fallback: &[String],
) {
    let mut source_number_universe = HashSet::<String>::new();
    for source_numbers in source_numbers_by_part {
        for value in source_numbers {
            source_number_universe.insert(value.clone());
        }
    }
    if source_number_universe.is_empty() {
        return;
    }

    let mut number_keys = source_number_universe.into_iter().collect::<Vec<_>>();
    number_keys.sort();
    for number in number_keys {
        let mut missing_indexes = Vec::<usize>::new();
        let mut leaked_indexes = Vec::<usize>::new();
        for index in 0..out.len() {
            let source_has = source_numbers_by_part
                .get(index)
                .map(|values| values.contains(&number))
                .unwrap_or(false);
            let line_numbers = out
                .get(index)
                .map(|line| extract_numbers(line))
                .unwrap_or_default();
            let line_has = line_numbers.contains(&number);
            if source_has && !line_has {
                missing_indexes.push(index);
            }
            if !source_has && line_has {
                leaked_indexes.push(index);
            }
        }
        if missing_indexes.is_empty() {
            continue;
        }

        for index in &missing_indexes {
            let fallback_text = fallback
                .get(*index)
                .map(|value| sanitize_translation_candidate(value))
                .unwrap_or_default();
            if is_unusable_translation(&fallback_text) {
                continue;
            }
            if extract_numbers(&fallback_text).contains(&number) {
                out[*index] = fallback_text;
            }
        }

        let unresolved_missing = missing_indexes
            .into_iter()
            .filter(|index| {
                out.get(*index)
                    .map(|line| !extract_numbers(line).contains(&number))
                    .unwrap_or(true)
            })
            .collect::<Vec<_>>();
        let has_leak_evidence = !leaked_indexes.is_empty()
            || unresolved_missing.iter().any(|index| {
                aligned_numbers_by_part
                    .get(*index)
                    .map(|values| values.contains(&number))
                    .unwrap_or(false)
            });
        if !has_leak_evidence {
            continue;
        }
        for index in unresolved_missing {
            let current = out.get(index).cloned().unwrap_or_default();
            if current.is_empty() {
                continue;
            }
            let updated = prepend_missing_number_token(&current, &number);
            if !updated.is_empty() {
                out[index] = updated;
            }
        }
    }
}
