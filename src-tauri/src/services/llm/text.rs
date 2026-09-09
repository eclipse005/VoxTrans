//! Small string utilities shared across the `llm` module.

/// Char-bounded truncation with an ASCII `...` ellipsis. Shared by the wire
/// layer (error bodies, SSE decode previews) and retry hint shaping, where a
/// bounded, greppable suffix beats an unbounded blob.
pub(super) fn truncate_chars(input: &str, max_chars: usize) -> String {
    if input.chars().count() <= max_chars {
        return input.to_string();
    }
    let out: String = input.chars().take(max_chars).collect();
    format!("{out}...")
}

#[cfg(test)]
mod tests {
    use super::truncate_chars;

    #[test]
    fn truncates_on_char_boundaries_with_ascii_ellipsis() {
        assert_eq!(truncate_chars("short", 10), "short");
        assert_eq!(truncate_chars("", 5), "");
        assert_eq!(truncate_chars("日本語のテキスト", 3), "日本語...");
    }
}
