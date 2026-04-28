pub(super) fn split_clauses(text: &str) -> Vec<String> {
    let mut out = Vec::<String>::new();
    let mut current = String::new();
    let chars = text.chars().collect::<Vec<_>>();
    for (index, ch) in chars.iter().enumerate() {
        let ch = *ch;
        current.push(ch);
        if is_clause_boundary_char(&chars, index) {
            let chunk = super::normalize_inline_text(&current);
            if !chunk.is_empty() {
                out.push(chunk);
            }
            current.clear();
        }
    }
    let tail = super::normalize_inline_text(&current);
    if !tail.is_empty() {
        out.push(tail);
    }
    out
}

fn is_clause_boundary_char(chars: &[char], index: usize) -> bool {
    let ch = chars.get(index).copied().unwrap_or_default();
    if matches!(ch, '.' | '!' | '?' | ';' | '。' | '！' | '？' | '；') {
        return true;
    }
    if !matches!(ch, ',' | '，' | '、' | ':' | '：') {
        return false;
    }
    let prev = index.checked_sub(1).and_then(|idx| chars.get(idx)).copied();
    let next = chars.get(index + 1).copied();
    if matches!(ch, ',' | ':')
        && prev.map(|value| value.is_ascii_digit()).unwrap_or(false)
        && next.map(|value| value.is_ascii_digit()).unwrap_or(false)
    {
        return false;
    }
    true
}
