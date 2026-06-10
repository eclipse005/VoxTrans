use crate::services::subtitle_srt::SubtitleSrtSegment;
use crate::services::workspace_subtitle::{
    WorkspaceSubtitleSegment, WorkspaceSubtitleWord,
};

pub fn beautify_subtitle_srt_segments(
    segments: &mut Vec<SubtitleSrtSegment>,
    subtitle_length_preset: &str,
    target_lang: &str,
) {
    for segment in &mut *segments {
        segment.translated_text = beautify_subtitle_text(&segment.translated_text);
    }
    crate::services::subtitle_step5::merge_watchability_subtitle_srt_segments(
        segments,
        subtitle_length_preset,
        target_lang,
    );
}

/// Beautify `WorkspaceSubtitleSegment`s in place so the SRT writer, DB,
/// and the in-app subtitle editor all see the same (beautified) text.
///
/// Internally runs `beautify_subtitle_srt_segments` (which only knows
/// about text), then re-attaches `source_words` to the (possibly
/// merged) output segments by timing containment — each word from the
/// originals is appended to whichever output segment covers its
/// [start_ms, end_ms].
pub fn beautify_workspace_segments(
    segments: &mut Vec<WorkspaceSubtitleSegment>,
    subtitle_length_preset: &str,
    target_lang: &str,
) {
    // Snapshot original words with their timings, source_text BEFORE
    // beautify (used to fix the source-text side which the SRT helper
    // does not touch).
    let original_words: Vec<(u64, u64, WorkspaceSubtitleWord)> = segments
        .iter()
        .flat_map(|seg| {
            seg.source_words
                .iter()
                .map(|w| (seg.start_ms, seg.end_ms, w.clone()))
                .collect::<Vec<_>>()
        })
        .collect();

    // Run the SRT-level beautify (text + watchability merge).
    let mut srt_segments: Vec<SubtitleSrtSegment> = segments
        .iter()
        .map(|seg| SubtitleSrtSegment {
            start_ms: seg.start_ms,
            end_ms: seg.end_ms,
            source_text: seg.source_text.clone(),
            translated_text: seg.translated_text.clone(),
        })
        .collect();
    beautify_subtitle_srt_segments(&mut srt_segments, subtitle_length_preset, target_lang);

    // Map back to WorkspaceSubtitleSegment.
    // - text fields: translated_text comes from srt_segments (already beautified);
    //   source_text is preserved as-is (beautify should only affect translation).
    // - source_words: re-attach by timing containment so a merged segment
    //   inherits the union of words from the originals it absorbed.
    *segments = srt_segments
        .into_iter()
        .map(|s| {
            let mut words: Vec<WorkspaceSubtitleWord> = original_words
                .iter()
                .filter(|(_orig_start, _orig_end, w)| {
                    // Word falls within this segment's window.
                    w.start_ms >= s.start_ms && w.end_ms <= s.end_ms
                })
                .map(|(_, _, w)| w.clone())
                .collect();
            words.sort_by_key(|w| w.start_ms);
            WorkspaceSubtitleSegment {
                start_ms: s.start_ms,
                end_ms: s.end_ms,
                source_text: s.source_text,
                translated_text: s.translated_text,
                source_words: words,
            }
        })
        .collect();
}

fn beautify_subtitle_text(raw: &str) -> String {
    let normalized = raw.replace('\r', "\n").replace('\n', " ");
    let normalized = normalized.trim();
    if normalized.is_empty() {
        return String::new();
    }

    let without_edges = trim_bounding_punctuation(normalized);
    if without_edges.is_empty() {
        return String::new();
    }
    let without_commas = remove_internal_commas_for_subtitle(&without_edges);
    let with_spacing = normalize_cjk_ascii_spacing(&without_commas);
    collapse_multiple_spaces(&with_spacing).trim().to_string()
}

fn trim_bounding_punctuation(text: &str) -> String {
    let mut chars = text.chars().collect::<Vec<char>>();
    while matches!(chars.first(), Some(ch) if is_subtitle_boundary_punctuation(*ch)) {
        let _ = chars.remove(0);
    }
    while matches!(chars.last(), Some(ch) if is_subtitle_boundary_punctuation(*ch)) {
        let _ = chars.pop();
    }
    chars.into_iter().collect()
}

fn is_subtitle_boundary_punctuation(ch: char) -> bool {
    ch.is_ascii_punctuation()
        || matches!(
            ch,
            '，' | '。'
                | '、'
                | '；'
                | '：'
                | '！'
                | '？'
                | '…'
                | '「'
                | '」'
                | '『'
                | '』'
                | '《'
                | '》'
                | '“'
                | '”'
                | '‘'
                | '’'
                | '（'
                | '）'
                | '［'
                | '］'
                | '【'
                | '】'
        )
}

fn remove_internal_commas_for_subtitle(text: &str) -> String {
    let chars = text.chars().collect::<Vec<_>>();
    let mut out = String::new();
    for idx in 0..chars.len() {
        let ch = chars[idx];
        if ch == ',' {
            let prev = chars.get(idx.wrapping_sub(1)).copied();
            let next = chars.get(idx + 1).copied();
            if prev.is_some_and(|value| value.is_ascii_digit())
                && next.is_some_and(|value| value.is_ascii_digit())
            {
                out.push(ch);
            } else {
                out.push(' ');
            }
            continue;
        }
        if ch == '，' {
            // Mirror the ASCII branch: replace with a space, not silently
            // drop. Otherwise two CJK words get glued together
            // ("...很棒，但..." → "...很棒但..." instead of "...很棒 但...")
            // since CJK↔CJK never triggers normalize_cjk_ascii_spacing.
            // collapse_multiple_spaces collapses any runs we introduce.
            out.push(' ');
            continue;
        }
        out.push(ch);
    }
    out
}

fn normalize_cjk_ascii_spacing(text: &str) -> String {
    let mut output = String::new();
    let mut previous = None;
    for ch in text.chars() {
        if let Some(prev) = previous
            && need_cjk_ascii_space(prev, ch)
            && !output.ends_with(' ')
        {
            output.push(' ');
        }
        output.push(ch);
        previous = Some(ch);
    }
    output
}

fn need_cjk_ascii_space(left: char, right: char) -> bool {
    (is_cjk_char(left) && is_ascii_word_char(right))
        || (is_ascii_word_char(left) && is_cjk_char(right))
}

fn is_ascii_word_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric()
}

fn is_cjk_char(ch: char) -> bool {
    let value = ch as u32;
    (0x3400..=0x4dbf).contains(&value)
        || (0x4e00..=0x9fff).contains(&value)
        || (0x20000..=0x2a6df).contains(&value)
        || (0xf900..=0xfaff).contains(&value)
        || (0x3040..=0x31ff).contains(&value)
        || (0xaf00..=0xafff).contains(&value)
}

fn collapse_multiple_spaces(text: &str) -> String {
    let mut out = String::new();
    let mut saw_space = false;
    for ch in text.chars() {
        if ch.is_whitespace() {
            if !saw_space {
                out.push(' ');
                saw_space = true;
            }
            continue;
        }
        out.push(ch);
        saw_space = false;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{
        beautify_subtitle_srt_segments, beautify_subtitle_text, collapse_multiple_spaces,
        is_ascii_word_char, is_cjk_char, need_cjk_ascii_space, trim_bounding_punctuation,
    };
    use crate::services::subtitle_srt::SubtitleSrtSegment;

    #[test]
    fn subtitle_beautify_text_handles_empty() {
        assert_eq!(beautify_subtitle_text(""), "");
        assert_eq!(beautify_subtitle_text("   "), "");
    }

    #[test]
    fn subtitle_beautify_text_removes_boundary_punctuation_and_commas() {
        assert_eq!(beautify_subtitle_text(" (Hello, world), "), "Hello world");
        assert_eq!(
            beautify_subtitle_text("代码,IPC,sockets"),
            "代码 IPC sockets"
        );
        // Full-width comma between CJK words must produce a space too,
        // otherwise the two words glue together (regression: 棒，但 → 棒但).
        assert_eq!(
            beautify_subtitle_text("盘整结构也很棒，但我们稍后会讨论"),
            "盘整结构也很棒 但我们稍后会讨论"
        );
    }

    #[test]
    fn subtitle_beautify_srt_segments_only_changes_translation() {
        let mut segments = vec![SubtitleSrtSegment {
            start_ms: 0,
            end_ms: 1000,
            source_text: " (Hello, world), ".to_string(),
            translated_text: " (你好，世界), ".to_string(),
        }];

        beautify_subtitle_srt_segments(&mut segments, "standard", "zh-CN");

        assert_eq!(segments[0].source_text, " (Hello, world), ");
        assert_eq!(segments[0].translated_text, "你好 世界");
    }

    #[test]
    fn subtitle_beautify_srt_segments_merges_with_original_watchability_logic() {
        let mut segments = vec![
            SubtitleSrtSegment {
                start_ms: 51_120,
                end_ms: 52_760,
                source_text: "And it's also just a good".to_string(),
                translated_text: "如果你某周表现不佳，可能会怀疑这".to_string(),
            },
            SubtitleSrtSegment {
                start_ms: 52_760,
                end_ms: 54_400,
                source_text: "exercise to rebuild belief in the system.".to_string(),
                translated_text: "个系统是否还有效，重建系统信心".to_string(),
            },
        ];

        beautify_subtitle_srt_segments(&mut segments, "standard", "zh-CN");

        assert_eq!(segments.len(), 1);
        assert_eq!(
            segments[0].source_text,
            "And it's also just a good exercise to rebuild belief in the system."
        );
        assert_eq!(
            segments[0].translated_text,
            "如果你某周表现不佳 可能会怀疑这个系统是否还有效 重建系统信心"
        );
    }

    #[test]
    fn subtitle_beautify_srt_segments_keeps_gap_over_half_second() {
        let mut segments = vec![
            SubtitleSrtSegment {
                start_ms: 0,
                end_ms: 1000,
                source_text: "And it's also just a good".to_string(),
                translated_text: "如果你某周表现不佳，可能会怀疑这".to_string(),
            },
            SubtitleSrtSegment {
                start_ms: 1501,
                end_ms: 2500,
                source_text: "exercise to rebuild belief in the system.".to_string(),
                translated_text: "个系统是否还有效，重建系统信心".to_string(),
            },
        ];

        beautify_subtitle_srt_segments(&mut segments, "standard", "zh-CN");

        assert_eq!(segments.len(), 2);
    }

    #[test]
    fn subtitle_beautify_srt_segments_respects_short_word_target_limit_when_merging() {
        let mut segments = vec![
            SubtitleSrtSegment {
                start_ms: 0,
                end_ms: 2000,
                source_text: "This source line is long enough to count as a real fragment"
                    .to_string(),
                translated_text:
                    "this local subtitle line is still clearly incomplete near the edge and"
                        .to_string(),
            },
            SubtitleSrtSegment {
                start_ms: 2000,
                end_ms: 3500,
                source_text: "the continuation should not make the short preset too wide"
                    .to_string(),
                translated_text:
                    "the continuation adds several more words for viewing comfort today again now"
                        .to_string(),
            },
        ];

        beautify_subtitle_srt_segments(&mut segments, "short", "en");

        assert_eq!(segments.len(), 2);
    }

    #[test]
    fn cjk_ascii_space_helpers() {
        assert!(is_cjk_char('中'));
        assert!(is_ascii_word_char('A'));
        assert!(need_cjk_ascii_space('码', 'v'));
        assert!(!need_cjk_ascii_space('码', ','));
        assert_eq!(collapse_multiple_spaces("a   b"), "a b");
        assert_eq!(trim_bounding_punctuation("「Hello，"), "Hello");
    }
}
