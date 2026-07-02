use super::terminology::TerminologySegment;

/// Per-window limits for the Step3 briefing pass. A window closes when
/// either threshold is hit, whichever comes first:
///   - `STEP3_WINDOW_MAX_CHARS`: bounds prompt size so a single LLM call
///     never overloads the model (especially local models on modest hardware).
///   - `STEP3_WINDOW_MAX_SEGMENTS`: keeps each window focused on a coherent
///     slice of the transcript and avoids dumping hundreds of lines at once.
const STEP3_WINDOW_MAX_CHARS: usize = 5_000;
const STEP3_WINDOW_MAX_SEGMENTS: usize = 100;

/// Build the per-segment transcript context (one normalized line per
/// non-empty segment). The Step3 briefing pass groups these lines into
/// windows so the whole transcript is covered, including terms that only
/// appear late.
pub(super) fn build_context_lines(segments: &[TerminologySegment]) -> Vec<String> {
    segments
        .iter()
        .filter_map(|segment| {
            if !segment.segment.trim().is_empty() {
                let normalized = normalize_inline_text(&segment.segment);
                if normalized.is_empty() {
                    None
                } else {
                    Some(normalized)
                }
            } else {
                let text = segment
                    .tokens
                    .iter()
                    .map(|token| token.text.trim())
                    .filter(|token| !token.is_empty())
                    .collect::<Vec<_>>()
                    .join(" ");
                let normalized = normalize_inline_text(&text);
                if normalized.is_empty() {
                    None
                } else {
                    Some(normalized)
                }
            }
        })
        .collect()
}

/// Group segment lines into non-overlapping windows. A window closes when
/// adding the next line would exceed `STEP3_WINDOW_MAX_CHARS` OR when the
/// window already contains `STEP3_WINDOW_MAX_SEGMENTS` lines — whichever
/// triggers first. Segment boundaries are always respected (a line is never
/// split across windows). Empty/whitespace-only windows are dropped.
pub(super) fn chunk_lines(lines: &[String]) -> Vec<String> {
    if lines.is_empty() {
        return Vec::new();
    }
    let non_empty: Vec<&String> = lines.iter().filter(|l| !l.trim().is_empty()).collect();
    if non_empty.is_empty() {
        return Vec::new();
    }
    let mut windows = Vec::new();
    let mut current: Vec<String> = Vec::new();
    let mut current_chars: usize = 0;
    for line in non_empty {
        let line_chars = line.chars().count();
        let added_chars = if current.is_empty() {
            line_chars
        } else {
            // +1 for the '\n' separator that join will insert between lines
            line_chars + 1
        };
        let would_exceed_chars = current_chars + added_chars > STEP3_WINDOW_MAX_CHARS;
        let would_exceed_segments = current.len() >= STEP3_WINDOW_MAX_SEGMENTS;
        if !current.is_empty() && (would_exceed_chars || would_exceed_segments) {
            windows.push(current.join("\n"));
            current.clear();
            current_chars = 0;
        }
        current.push(line.clone());
        current_chars += line_chars;
    }
    if !current.is_empty() {
        windows.push(current.join("\n"));
    }
    windows
}

pub(super) fn normalize_inline_text(raw: &str) -> String {
    raw.replace(['\r', '\n'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::terminology::{TerminologySegment, TerminologyToken};

    fn seg(text: &str) -> TerminologySegment {
        TerminologySegment {
            segment: text.to_string(),
            tokens: Vec::<TerminologyToken>::new(),
        }
    }

    fn lines_from(texts: &[&str]) -> Vec<String> {
        texts.iter().map(|t| t.to_string()).collect()
    }

    #[test]
    fn empty_input_yields_no_windows() {
        assert!(chunk_lines(&[]).is_empty());
    }

    #[test]
    fn small_input_fits_in_one_window() {
        // 43 segments of ~170 chars each = ~7500 chars total.
        // With 5000-char / 100-segment limits this could be 1 or 2 windows
        // depending on exact size; verify it does NOT dump everything when
        // over the char limit.
        let short: Vec<String> = (0..10).map(|i| format!("段{i}：短句")).collect();
        let windows = chunk_lines(&short);
        assert_eq!(windows.len(), 1, "10 short lines should fit in 1 window");
    }

    #[test]
    fn splits_on_segment_count_limit() {
        // 150 segments, each tiny — should split by segment count (100).
        let many: Vec<String> = (0..150).map(|i| format!("s{i}")).collect();
        let windows = chunk_lines(&many);
        assert_eq!(
            windows.len(),
            2,
            "150 segments with 100-segment cap should yield 2 windows"
        );
        // First window has 100 lines, second has 50.
        assert_eq!(windows[0].lines().count(), 100);
        assert_eq!(windows[1].lines().count(), 50);
    }

    #[test]
    fn splits_on_char_limit_respecting_segment_boundaries() {
        // Each line ~200 chars; 30 lines = ~6000 chars > 5000 limit.
        // Should split into 2 windows, no line cut in half.
        let line = "字".repeat(200);
        let many: Vec<String> = (0..30).map(|_| line.clone()).collect();
        let windows = chunk_lines(&many);
        assert_eq!(windows.len(), 2, "30x200-char lines should split at 5000 chars");
        // No window should exceed the char limit (accounting for \n separators).
        for w in &windows {
            assert!(
                w.chars().count() <= STEP3_WINDOW_MAX_CHARS,
                "window over limit: {} chars",
                w.chars().count()
            );
        }
        // All 30 lines should be present across windows (no data loss).
        let total_lines: usize = windows.iter().map(|w| w.lines().count()).sum();
        assert_eq!(total_lines, 30);
    }

    #[test]
    fn single_line_over_char_limit_still_emitted() {
        // One giant line — must still emit (don't drop data even if over limit).
        let huge = vec!["字".repeat(6000)];
        let windows = chunk_lines(&huge);
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].chars().count(), 6000);
    }

    #[test]
    fn empty_lines_skipped() {
        let mixed = vec!["".to_string(), "  ".to_string(), "ok".to_string()];
        let windows = chunk_lines(&mixed);
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0], "ok");
    }

    #[test]
    fn build_context_lines_drops_empty_segments() {
        let segments = vec![
            seg("你好"),
            seg(""),
            seg("  "),
            seg("世界"),
        ];
        let lines = build_context_lines(&segments);
        assert_eq!(lines, lines_from(&["你好", "世界"]));
    }

    #[test]
    fn build_context_lines_falls_back_to_tokens() {
        let mut s = seg("");
        s.tokens = vec![
            TerminologyToken { text: "token1".to_string() },
            TerminologyToken { text: "token2".to_string() },
        ];
        let lines = build_context_lines(&[s]);
        assert_eq!(lines, lines_from(&["token1 token2"]));
    }
}
