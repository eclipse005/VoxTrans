use crate::services::subtitle_srt::SubtitleSrtSegment;

pub fn beautify_subtitle_srt_segments(
    segments: &mut Vec<SubtitleSrtSegment>,
    subtitle_length_reference: u32,
    target_lang: &str,
) {
    for segment in &mut *segments {
        segment.translated_text = beautify_subtitle_text(&segment.translated_text);
    }
    crate::services::subtitle_step5::merge_watchability_subtitle_srt_segments(
        segments,
        subtitle_length_reference,
        target_lang,
    );
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
        if let Some(prev) = previous {
            if need_cjk_ascii_space(prev, ch) && !output.ends_with(' ') {
                output.push(' ');
            }
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
    }

    #[test]
    fn subtitle_beautify_srt_segments_only_changes_translation() {
        let mut segments = vec![SubtitleSrtSegment {
            start_ms: 0,
            end_ms: 1000,
            source_text: " (Hello, world), ".to_string(),
            translated_text: " (你好，世界), ".to_string(),
        }];

        beautify_subtitle_srt_segments(&mut segments, 28, "zh-CN");

        assert_eq!(segments[0].source_text, " (Hello, world), ");
        assert_eq!(segments[0].translated_text, "你好世界");
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

        beautify_subtitle_srt_segments(&mut segments, 28, "zh-CN");

        assert_eq!(segments.len(), 1);
        assert_eq!(
            segments[0].source_text,
            "And it's also just a good exercise to rebuild belief in the system."
        );
        assert_eq!(
            segments[0].translated_text,
            "如果你某周表现不佳可能会怀疑这个系统是否还有效重建系统信心"
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

        beautify_subtitle_srt_segments(&mut segments, 28, "zh-CN");

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
