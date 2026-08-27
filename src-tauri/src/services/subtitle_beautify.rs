use crate::services::subtitle_srt::SubtitleSrtSegment;
use crate::services::workspace_subtitle::WorkspaceSubtitleSegment;

pub fn beautify_subtitle_srt_segments(
    segments: &mut Vec<SubtitleSrtSegment>,
    _subtitle_length_preset: &str,
    target_lang: &str,
) {
    beautify_subtitle_srt_text(segments, target_lang);
    pad_cue_hold_and_gaps(segments);
}

/// CJK comma/period polish only. Does not change timestamps.
/// Export uses this so hold/gap padding is not applied twice on already
/// materialized cues (min-hold then bridging the leftover gap).
pub fn beautify_subtitle_srt_text(segments: &mut [SubtitleSrtSegment], target_lang: &str) {
    if !is_cjk_target(target_lang) {
        return;
    }
    for segment in segments {
        segment.translated_text = beautify_subtitle_text(&segment.translated_text);
    }
}

/// Minimum on-screen duration. A short cue may grow up to this when there
/// is room — never in the same step as snapping to the next cue.
const MIN_HOLD_MS: u64 = 1000;
/// If the original gap to the next cue is below this, snap end to next
/// start so the viewer sees a cut, not a blank flash.
const MAX_GAP_FILL_MS: u64 = 1000;

/// Two mutually exclusive timing tweaks, decided from the original gap:
/// 1. Gap to next is (0, 1s) → end = next.start. Done.
/// 2. Else if duration < 1s → grow up to 1s, never past next.start.
///    The leftover gap is left as-is; it must not then trigger (1).
fn pad_cue_hold_and_gaps(segments: &mut [SubtitleSrtSegment]) {
    let len = segments.len();
    for i in 0..len {
        let start = segments[i].start_ms;
        let mut end = segments[i].end_ms.max(start);
        let next_start = if i + 1 < len {
            Some(segments[i + 1].start_ms)
        } else {
            None
        };

        match next_start {
            Some(next) if next <= start => {}
            Some(next) if next > end && next - end < MAX_GAP_FILL_MS => {
                end = next;
            }
            Some(next) if next > end && end - start < MIN_HOLD_MS => {
                end = start.saturating_add(MIN_HOLD_MS).min(next);
            }
            None if end - start < MIN_HOLD_MS => {
                end = start.saturating_add(MIN_HOLD_MS);
            }
            _ => {}
        }

        segments[i].end_ms = end;
    }
}

fn is_cjk_target(target_lang: &str) -> bool {
    let lower = target_lang.to_ascii_lowercase();
    lower == "zh-cn"
        || lower == "zh-tw"
        || lower == "zh"
        || lower.starts_with("zh-")
        || lower == "yue"
        || lower.starts_with("yue-")
}

/// Text polish + one-shot hold/gap pad. Call once on source SoT.
pub fn beautify_workspace_segments(
    segments: &mut Vec<WorkspaceSubtitleSegment>,
    subtitle_length_preset: &str,
    target_lang: &str,
) {
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
    for (seg, srt) in segments.iter_mut().zip(srt_segments) {
        seg.end_ms = srt.end_ms;
        seg.translated_text = srt.translated_text;
    }
}

/// CJK text polish only. Use on already-padded cues (target SoT / export).
pub fn beautify_workspace_text(segments: &mut [WorkspaceSubtitleSegment], target_lang: &str) {
    if !is_cjk_target(target_lang) {
        return;
    }
    for segment in segments {
        segment.translated_text = beautify_subtitle_text(&segment.translated_text);
    }
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
    matches!(ch, '.' | '。' | ',' | '，')
}

fn remove_internal_commas_for_subtitle(text: &str) -> String {
    let chars = text.chars().collect::<Vec<_>>();
    let mut out = String::new();
    for idx in 0..chars.len() {
        let ch = chars[idx];
        match ch {
            ',' => {
                // Keep digit-separator commas like "3,000", replace all others with space.
                let prev = chars.get(idx.wrapping_sub(1)).copied();
                let next = chars.get(idx + 1).copied();
                if prev.is_some_and(|value| value.is_ascii_digit())
                    && next.is_some_and(|value| value.is_ascii_digit())
                {
                    out.push(ch);
                } else {
                    out.push(' ');
                }
            }
            '，' | '.' | '。' => {
                // ponytail: Chinese commas/periods and ASCII periods become spaces.
                // Other punctuation (？！、！ etc.) is left untouched.
                out.push(' ');
            }
            _ => out.push(ch),
        }
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
        beautify_subtitle_srt_segments, beautify_subtitle_srt_text, beautify_subtitle_text,
        collapse_multiple_spaces, is_ascii_word_char, is_cjk_char, need_cjk_ascii_space,
        pad_cue_hold_and_gaps, trim_bounding_punctuation,
    };
    use crate::services::subtitle_srt::SubtitleSrtSegment;

    #[test]
    fn subtitle_beautify_text_handles_empty() {
        assert_eq!(beautify_subtitle_text(""), "");
        assert_eq!(beautify_subtitle_text("   "), "");
    }

    #[test]
    fn subtitle_beautify_text_removes_boundary_punctuation_and_commas() {
        // Only periods (。 .) are trimmed from edges; commas are replaced
        // with spaces internally.
        assert_eq!(beautify_subtitle_text(" (Hello, world), "), "(Hello world)");
        assert_eq!(
            beautify_subtitle_text("代码,IPC,sockets"),
            "代码 IPC sockets"
        );
        assert_eq!(
            beautify_subtitle_text("盘整结构也很棒，但我们稍后会讨论"),
            "盘整结构也很棒 但我们稍后会讨论"
        );
        // Trailing period removed, closing paren left intact.
        assert_eq!(
            beautify_subtitle_text("中间/过渡状态）。"),
            "中间/过渡状态）"
        );
    }

    #[test]
    fn subtitle_beautify_text_handles_chinese_commas_and_periods() {
        // Leading/trailing Chinese comma and period are stripped.
        assert_eq!(beautify_subtitle_text("，你好，世界。"), "你好 世界");
        // Internal commas and periods become spaces.
        assert_eq!(
            beautify_subtitle_text("你好，世界。你好。世界，"),
            "你好 世界 你好 世界"
        );
        // Mixed: leading period, internal comma+period.
        assert_eq!(
            beautify_subtitle_text("。你好，世界。再见，"),
            "你好 世界 再见"
        );
        // Other punctuation (？！、！) is left untouched.
        assert_eq!(beautify_subtitle_text("你好！真的吗？"), "你好！真的吗？");
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

        // source_text is untouched.
        assert_eq!(segments[0].source_text, " (Hello, world), ");
        // translated_text: commas → spaces; parens kept; periods trimmed.
        assert_eq!(segments[0].translated_text, "(你好 世界)");
    }

    #[test]
    fn subtitle_beautify_srt_segments_keeps_latin_text_untouched() {
        let mut segments = vec![SubtitleSrtSegment {
            start_ms: 0,
            end_ms: 1000,
            source_text: "Hello, world.".to_string(),
            translated_text: "Hola, mundo.".to_string(),
        }];

        beautify_subtitle_srt_segments(&mut segments, "standard", "en");

        // Non-CJK: the text beautify pass (comma/period trimming, CJK-ASCII
        // spacing) is skipped, so translation text stays byte-identical.
        assert_eq!(segments[0].translated_text, "Hola, mundo.");
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].end_ms, 1000);
    }

    #[test]
    fn subtitle_beautify_does_not_merge_adjacent_cues() {
        let mut segments = vec![
            SubtitleSrtSegment {
                start_ms: 0,
                end_ms: 2000,
                source_text: "And it's also just a good".to_string(),
                translated_text: "如果你某周表现不佳，可能会怀疑这".to_string(),
            },
            SubtitleSrtSegment {
                start_ms: 2000,
                end_ms: 3500,
                source_text: "exercise to rebuild belief in the system.".to_string(),
                translated_text: "个系统是否还有效，重建系统信心".to_string(),
            },
        ];

        beautify_subtitle_srt_segments(&mut segments, "standard", "zh-CN");

        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].end_ms, 2000);
        assert_eq!(segments[1].start_ms, 2000);
    }

    #[test]
    fn cjk_ascii_space_helpers() {
        assert!(is_cjk_char('中'));
        assert!(is_ascii_word_char('A'));
        assert!(need_cjk_ascii_space('码', 'v'));
        assert!(!need_cjk_ascii_space('码', ','));
        assert_eq!(collapse_multiple_spaces("a   b"), "a b");
        // Commas are now also boundary punctuation.
        assert_eq!(trim_bounding_punctuation("，Hello，"), "Hello");
    }

    fn timed(start_ms: u64, end_ms: u64) -> SubtitleSrtSegment {
        SubtitleSrtSegment {
            start_ms,
            end_ms,
            source_text: "Right?".into(),
            translated_text: String::new(),
        }
    }

    #[test]
    fn pads_sub_1000ms_hold_when_following_gap_is_wide() {
        let mut segments = vec![timed(0, 240), timed(4000, 5000)];
        pad_cue_hold_and_gaps(&mut segments);
        assert_eq!(segments[0].end_ms, 1000);
        assert_eq!(segments[1].start_ms, 4000);
    }

    #[test]
    fn does_not_grow_past_the_next_cue() {
        let mut segments = vec![timed(0, 240), timed(400, 900)];
        pad_cue_hold_and_gaps(&mut segments);
        assert_eq!(segments[0].end_ms, 400);
    }

    #[test]
    fn snaps_end_to_next_when_original_gap_is_under_1000ms() {
        let mut segments = vec![timed(0, 600), timed(1100, 2000)];
        pad_cue_hold_and_gaps(&mut segments);
        assert_eq!(segments[0].end_ms, 1100);
    }

    #[test]
    fn does_not_fill_gap_of_1000ms_or_more() {
        let mut segments = vec![timed(0, 1200), timed(2200, 3000)];
        pad_cue_hold_and_gaps(&mut segments);
        assert_eq!(segments[0].end_ms, 1200);
    }

    #[test]
    fn short_cue_with_small_gap_snaps_instead_of_min_hold() {
        // Original gap 400ms < 1s → snap to next. Do not min-hold to 1000
        // (which would overlap the next cue anyway).
        let mut segments = vec![timed(0, 200), timed(600, 1200)];
        pad_cue_hold_and_gaps(&mut segments);
        assert_eq!(segments[0].end_ms, 600);
    }

    #[test]
    fn min_hold_does_not_then_bridge_the_leftover_gap() {
        // Duration 200ms, original gap 1300ms ≥ 1s → grow to 1000ms only.
        // Leftover 500ms stays; do not also snap to the next cue.
        let mut segments = vec![timed(0, 200), timed(1500, 2200)];
        pad_cue_hold_and_gaps(&mut segments);
        assert_eq!(segments[0].end_ms, 1000);
        assert_eq!(segments[1].start_ms, 1500);
    }

    #[test]
    fn last_cue_pads_to_min_hold() {
        let mut segments = vec![timed(0, 240)];
        pad_cue_hold_and_gaps(&mut segments);
        assert_eq!(segments[0].end_ms, 1000);
    }

    #[test]
    fn text_polish_after_pad_does_not_snap_leftover_gap() {
        let mut segments = vec![timed(0, 200), timed(1500, 2200)];
        beautify_subtitle_srt_segments(&mut segments, "standard", "en");
        assert_eq!(segments[0].end_ms, 1000);
        beautify_subtitle_srt_text(&mut segments, "en");
        assert_eq!(segments[0].end_ms, 1000);
        assert_eq!(segments[1].start_ms, 1500);
    }
}
