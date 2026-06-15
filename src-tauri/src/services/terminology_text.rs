use super::terminology::TerminologySegment;

/// Per-window character budget for the Step3 briefing pass. Keeps each LLM
/// call focused (a small window is handled reliably and never overloads the
/// model) and bounds prompt size for long videos. Non-overlapping stride
/// equals the window size.
const STEP3_WINDOW_CHARS: usize = 8_000;

/// Build the full transcript context (one line per non-empty segment),
/// without truncation. The Step3 briefing pass iterates this text in windows
/// so the whole transcript is covered, including terms that only appear late.
pub(super) fn build_context_text(segments: &[TerminologySegment]) -> String {
    let lines = segments
        .iter()
        .filter_map(|segment| {
            if !segment.segment.trim().is_empty() {
                return Some(normalize_inline_text(&segment.segment));
            }
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
        })
        .collect::<Vec<_>>();

    lines.join("\n")
}

/// Split `full` into non-overlapping character windows. Each window is fed
/// to one briefing LLM call. Cut by char count (not bytes) so CJK text is
/// handled correctly. Empty/whitespace-only trailing windows are dropped.
pub(super) fn chunk_text(full: &str) -> Vec<String> {
    if full.is_empty() {
        return Vec::new();
    }
    let chars: Vec<char> = full.chars().collect();
    let mut windows = Vec::new();
    let mut start = 0;
    while start < chars.len() {
        let end = (start + STEP3_WINDOW_CHARS).min(chars.len());
        let window: String = chars[start..end].iter().collect();
        if !window.trim().is_empty() {
            windows.push(window);
        }
        if end == chars.len() {
            break;
        }
        start += STEP3_WINDOW_CHARS;
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
