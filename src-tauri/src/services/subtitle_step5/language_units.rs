pub(super) fn text_length_units(text: &str, lang: &str) -> f64 {
    if text.trim().is_empty() {
        return 0.0;
    }
    if use_char_units(lang, text) {
        count_char_units(text) as f64
    } else {
        count_word_units(text) as f64
    }
}

pub(super) fn use_char_units(lang: &str, text: &str) -> bool {
    let lower = lang.trim().to_ascii_lowercase();
    if lower.starts_with("zh") || lower.starts_with("ja") || lower.starts_with("ko") {
        return true;
    }
    if lower.is_empty() || lower == "auto" {
        return contains_cjk(text);
    }
    false
}

pub(super) fn contains_cjk(text: &str) -> bool {
    text.chars().any(is_cjk_char)
}

pub(super) fn count_char_units(text: &str) -> usize {
    let mut total = 0usize;
    let mut in_ascii_group = false;
    for ch in text.chars() {
        if ch.is_whitespace() {
            if in_ascii_group {
                total += 1;
                in_ascii_group = false;
            }
            continue;
        }
        if ch.is_ascii_alphanumeric() {
            in_ascii_group = true;
            continue;
        }
        if in_ascii_group {
            total += 1;
            in_ascii_group = false;
        }
        if is_cjk_char(ch) || ch.is_alphanumeric() {
            total += 1;
        }
    }
    if in_ascii_group {
        total += 1;
    }
    total
}

pub(super) fn count_word_units(text: &str) -> usize {
    let mut total = 0usize;
    let mut in_word = false;
    for ch in text.chars() {
        if ch.is_alphanumeric() {
            if !in_word {
                total += 1;
                in_word = true;
            }
            continue;
        }
        if is_cjk_char(ch) {
            total += 1;
        }
        in_word = false;
    }
    total
}

pub(super) fn is_cjk_char(ch: char) -> bool {
    matches!(
        ch as u32,
        0x3040..=0x30FF
            | 0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xF900..=0xFAFF
            | 0xAC00..=0xD7AF
    )
}

pub(super) fn is_hangul_char(ch: char) -> bool {
    matches!(
        ch as u32,
        0x1100..=0x11FF
            | 0x3130..=0x318F
            | 0xA960..=0xA97F
            | 0xAC00..=0xD7AF
            | 0xD7B0..=0xD7FF
    )
}
