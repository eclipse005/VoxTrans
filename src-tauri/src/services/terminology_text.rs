use super::terminology::TerminologySegment;

const MAX_CONTEXT_CHARS: usize = 8_000;

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

    truncate_chars(&lines.join("\n"), MAX_CONTEXT_CHARS)
}

pub(super) fn normalize_theme(raw: &str) -> String {
    normalize_inline_text(raw)
}

pub(super) fn normalize_inline_text(raw: &str) -> String {
    raw.replace(['\r', '\n'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string()
}

fn truncate_chars(input: &str, max_chars: usize) -> String {
    if input.chars().count() <= max_chars {
        return input.to_string();
    }
    input.chars().take(max_chars).collect::<String>()
}
