use serde_json::{Value, json};
use voxtrans_core::subtitle::alignment::align_text_to_timestamps;
use voxtrans_core::subtitle::segmenter::WordToken;
use voxtrans_core::subtitle::srt::{SrtCue, to_srt_from_cues};

use crate::services::translate::pipeline::beautify_translated_text;
use crate::services::translate::types::TranslateSegment;

#[derive(Debug, Clone)]
pub struct WordTimingAnchor {
    pub start: f64,
    pub end: f64,
    pub word: String,
}

pub fn realign_segments_with_words(
    segments: &mut [TranslateSegment],
    word_timestamps: &[WordTimingAnchor],
) -> Value {
    if segments.is_empty() {
        return json!({ "applied": false, "reason": "empty_segments" });
    }
    let words = word_timestamps
        .iter()
        .filter_map(|w| {
            let word = w.word.trim().to_string();
            if word.is_empty() {
                return None;
            }
            Some(WordToken {
                start: w.start,
                end: w.end.max(w.start),
                word,
            })
        })
        .collect::<Vec<_>>();
    if words.is_empty() {
        return json!({ "applied": false, "reason": "empty_words" });
    }
    let groups = build_alignment_groups(segments);
    if groups.is_empty() {
        return json!({ "applied": false, "reason": "empty_source_text" });
    }

    if words.len() < groups.len() {
        return json!({
            "applied": false,
            "reason": "insufficient_words",
            "wordTotal": words.len(),
            "segmentTotal": segments.len(),
            "groupTotal": groups.len()
        });
    }

    let source_full_text = groups
        .iter()
        .map(|g| g.source_text.as_str())
        .filter(|v| !v.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if source_full_text.is_empty() {
        return json!({ "applied": false, "reason": "empty_source_text" });
    }

    let aligned_words = align_text_to_timestamps(&source_full_text, &words);
    if aligned_words.is_empty() {
        return json!({ "applied": false, "reason": "alignment_empty" });
    }
    if aligned_words.len() < groups.len() {
        return json!({
            "applied": false,
            "reason": "alignment_insufficient_words",
            "alignedWordTotal": aligned_words.len(),
            "segmentTotal": segments.len(),
            "groupTotal": groups.len()
        });
    }

    let mut segment_token_owners: Vec<usize> = Vec::new();
    let mut segment_token_stream: Vec<String> = Vec::new();
    for (group_idx, group) in groups.iter().enumerate() {
        for token in group.source_text.split_whitespace() {
            let norm = normalize_alignment_token(token);
            if norm.is_empty() {
                continue;
            }
            segment_token_owners.push(group_idx);
            segment_token_stream.push(norm);
        }
    }
    let aligned_word_stream = aligned_words
        .iter()
        .map(|w| normalize_alignment_token(&w.word))
        .filter(|t| !t.is_empty())
        .collect::<Vec<_>>();
    if segment_token_stream.is_empty() || aligned_word_stream.is_empty() {
        return json!({
            "applied": false,
            "reason": "alignment_token_stream_empty",
            "segmentTokenTotal": segment_token_stream.len(),
            "alignedWordTokenTotal": aligned_word_stream.len()
        });
    }

    let matched_pairs = lcs_match_pairs(&segment_token_stream, &aligned_word_stream);
    let mut segment_boundaries: Vec<Option<(usize, usize)>> = vec![None; groups.len()];
    for (segment_token_idx, aligned_word_idx) in matched_pairs {
        let seg_idx = segment_token_owners[segment_token_idx];
        match &mut segment_boundaries[seg_idx] {
            Some((start, end)) => {
                if aligned_word_idx < *start {
                    *start = aligned_word_idx;
                }
                if aligned_word_idx > *end {
                    *end = aligned_word_idx;
                }
            }
            None => {
                segment_boundaries[seg_idx] = Some((aligned_word_idx, aligned_word_idx));
            }
        }
    }

    let total_segments = groups.len();
    let mut cursor_word = 0usize;
    let mut changed = 0usize;
    let mut fallback_segments = 0usize;
    for (idx, group) in groups.iter().enumerate() {
        if cursor_word >= aligned_words.len() {
            break;
        }
        let (mut start_idx, mut end_idx) = match segment_boundaries[idx] {
            Some(boundary) => boundary,
            None => {
                fallback_segments += 1;
                let remaining = total_segments.saturating_sub(idx).max(1);
                let remaining_words = aligned_words.len().saturating_sub(cursor_word).max(1);
                let allocation = if idx + 1 == total_segments {
                    remaining_words
                } else {
                    (remaining_words / remaining).max(1)
                };
                let start = cursor_word.min(aligned_words.len().saturating_sub(1));
                let end = (start + allocation.saturating_sub(1))
                    .min(aligned_words.len().saturating_sub(1));
                (start, end)
            }
        };
        if start_idx < cursor_word {
            start_idx = cursor_word;
        }
        if end_idx < start_idx {
            end_idx = start_idx;
        }
        end_idx = end_idx.min(aligned_words.len().saturating_sub(1));

        let start_word = &aligned_words[start_idx];
        let end_word = &aligned_words[end_idx];
        let new_start_ms = (start_word.start.max(0.0) * 1000.0).round() as u64;
        let new_end_ms = (end_word.end.max(start_word.start) * 1000.0).round() as u64;
        changed += apply_group_timing(segments, group, new_start_ms, new_end_ms.max(new_start_ms));
        cursor_word = end_idx.saturating_add(1);
    }

    json!({
        "applied": true,
        "segmentTotal": segments.len(),
        "groupTotal": total_segments,
        "wordTotal": words.len(),
        "alignedWordTotal": aligned_words.len(),
        "changedSegmentTotal": changed,
        "fallbackSegmentTotal": fallback_segments
    })
}

pub fn build_srt_from_translate_segments(
    segments: &[TranslateSegment],
    translated: bool,
) -> String {
    let cues = segments
        .iter()
        .enumerate()
        .map(|(idx, segment)| SrtCue {
            index: idx + 1,
            start_ms: segment.start_ms,
            end_ms: segment.end_ms.max(segment.start_ms),
            text: if translated {
                segment.translated_text.trim().to_string()
            } else {
                segment.source_text.trim().to_string()
            },
        })
        .collect::<Vec<_>>();
    to_srt_from_cues(&cues)
}

pub fn build_bilingual_srt_from_translate_segments(
    segments: &[TranslateSegment],
    source_first: bool,
) -> String {
    let cues = segments
        .iter()
        .enumerate()
        .map(|(idx, segment)| {
            let source = segment.source_text.trim();
            let translated = segment.translated_text.trim();
            let text = if source_first {
                format!("{source}\n{translated}")
            } else {
                format!("{translated}\n{source}")
            };
            SrtCue {
                index: idx + 1,
                start_ms: segment.start_ms,
                end_ms: segment.end_ms.max(segment.start_ms),
                text,
            }
        })
        .collect::<Vec<_>>();
    to_srt_from_cues(&cues)
}

pub fn apply_subtitle_beautify_to_segments(
    segments: &[TranslateSegment],
    enabled: bool,
) -> Vec<TranslateSegment> {
    if !enabled {
        return segments.to_vec();
    }
    let mut result: Vec<TranslateSegment> = segments
        .iter()
        .cloned()
        .map(|mut seg| {
            seg.translated_text = beautify_translated_text(&seg.translated_text);
            seg
        })
        .collect();
    fill_subtitle_gaps(&mut result, 200);
    result
}

fn fill_subtitle_gaps(segments: &mut [TranslateSegment], max_gap_ms: u64) {
    if segments.len() < 2 {
        return;
    }
    for i in 0..segments.len().saturating_sub(1) {
        let current_end = segments[i].end_ms;
        let next_start = segments[i + 1].start_ms;
        if next_start <= current_end {
            continue;
        }
        let gap = next_start.saturating_sub(current_end);
        if gap <= max_gap_ms {
            let current_duration = current_end.saturating_sub(segments[i].start_ms);
            let next_duration = segments[i + 1].end_ms.saturating_sub(next_start);
            if current_duration <= next_duration {
                segments[i].end_ms = next_start;
            } else {
                segments[i + 1].start_ms = current_end;
            }
        }
    }
}

#[derive(Debug, Clone)]
struct AlignmentGroup {
    source_text: String,
    segment_indexes: Vec<usize>,
}

fn build_alignment_groups(segments: &[TranslateSegment]) -> Vec<AlignmentGroup> {
    let mut groups: Vec<AlignmentGroup> = Vec::new();
    for (idx, segment) in segments.iter().enumerate() {
        let source_text = segment.source_text.trim().to_string();
        if source_text.is_empty() {
            continue;
        }
        if let Some(last_group) = groups.last_mut() {
            if last_group.source_text == source_text {
                last_group.segment_indexes.push(idx);
                continue;
            }
        }
        groups.push(AlignmentGroup {
            source_text,
            segment_indexes: vec![idx],
        });
    }
    groups
}

fn apply_group_timing(
    segments: &mut [TranslateSegment],
    group: &AlignmentGroup,
    group_start_ms: u64,
    group_end_ms: u64,
) -> usize {
    if group.segment_indexes.is_empty() {
        return 0;
    }
    if group.segment_indexes.len() == 1 {
        let idx = group.segment_indexes[0];
        let segment = &mut segments[idx];
        let changed =
            usize::from(segment.start_ms != group_start_ms || segment.end_ms != group_end_ms);
        segment.start_ms = group_start_ms;
        segment.end_ms = group_end_ms.max(group_start_ms);
        return changed;
    }

    let total_duration = group_end_ms.saturating_sub(group_start_ms);
    let weights = group
        .segment_indexes
        .iter()
        .map(|idx| {
            let segment = &segments[*idx];
            segment.end_ms.saturating_sub(segment.start_ms).max(1)
        })
        .collect::<Vec<_>>();
    let total_weight = weights.iter().copied().sum::<u64>().max(1);

    let mut changed = 0usize;
    let mut cursor = group_start_ms;
    for (position, seg_idx) in group.segment_indexes.iter().enumerate() {
        let segment = &mut segments[*seg_idx];
        let next_end = if position + 1 == group.segment_indexes.len() {
            group_end_ms
        } else {
            let weight = weights[position];
            let slice =
                ((total_duration as f64) * (weight as f64 / total_weight as f64)).round() as u64;
            (cursor + slice.max(1)).min(group_end_ms.saturating_sub(1).max(cursor + 1))
        };
        let final_end = next_end.max(cursor);
        changed += usize::from(segment.start_ms != cursor || segment.end_ms != final_end);
        segment.start_ms = cursor;
        segment.end_ms = final_end;
        cursor = final_end;
    }
    changed
}

fn normalize_alignment_token(token: &str) -> String {
    token
        .chars()
        .filter(|ch| ch.is_alphanumeric())
        .flat_map(|ch| ch.to_lowercase())
        .collect::<String>()
}

fn lcs_match_pairs(left: &[String], right: &[String]) -> Vec<(usize, usize)> {
    if left.is_empty() || right.is_empty() {
        return Vec::new();
    }
    let n = left.len();
    let m = right.len();
    let mut dp = vec![vec![0usize; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[i][j] = if left[i] == right[j] {
                dp[i + 1][j + 1] + 1
            } else {
                dp[i + 1][j].max(dp[i][j + 1])
            };
        }
    }
    let mut pairs = Vec::new();
    let (mut i, mut j) = (0usize, 0usize);
    while i < n && j < m {
        if left[i] == right[j] {
            pairs.push((i, j));
            i += 1;
            j += 1;
        } else if dp[i + 1][j] >= dp[i][j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    pairs
}
