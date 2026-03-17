use super::WordToken;

pub(super) fn split_by_semantic_boundary(words: &[WordToken], max_pause_ms: f64) -> Vec<Vec<WordToken>> {
    let mut segments: Vec<Vec<WordToken>> = Vec::new();
    let mut current: Vec<WordToken> = Vec::new();

    for (idx, word) in words.iter().enumerate() {
        current.push(word.clone());

        let punctuation_break = should_split_after_word(words, idx);
        let pause_break = if idx + 1 < words.len() {
            let pause_ms = (words[idx + 1].start - word.end).max(0.0) * 1000.0;
            pause_ms >= max_pause_ms
        } else {
            false
        };

        if punctuation_break || pause_break {
            segments.push(current);
            current = Vec::new();
        }
    }

    if !current.is_empty() {
        segments.push(current);
    }

    segments
}

fn should_split_after_word(words: &[WordToken], idx: usize) -> bool {
    let current_word = match words.get(idx) {
        Some(w) => w.word.trim(),
        None => return false,
    };
    let normalized = strip_trailing_closers(current_word);
    if normalized.is_empty() || !ends_with_terminal_punctuation(normalized) {
        return false;
    }
    if is_non_break_terminal_case(normalized) {
        return false;
    }

    if let Some(next) = words.get(idx + 1) {
        let next_token = strip_leading_openers(next.word.trim());
        if starts_with_lowercase(next_token) {
            return false;
        }
    }

    true
}

fn ends_with_terminal_punctuation(word: &str) -> bool {
    word.chars()
        .last()
        .map(is_terminal_punctuation)
        .unwrap_or(false)
}

fn strip_trailing_closers(token: &str) -> &str {
    token.trim_end_matches(|c: char| {
        matches!(
            c,
            '"' | '\'' | ')' | ']' | '}' | '”' | '’' | '》' | '」' | '』'
        )
    })
}

fn strip_leading_openers(token: &str) -> &str {
    token.trim_start_matches(|c: char| {
        matches!(
            c,
            '"' | '\'' | '(' | '[' | '{' | '“' | '‘' | '《' | '「' | '『'
        )
    })
}

fn is_terminal_punctuation(c: char) -> bool {
    matches!(
        c,
        '.' | '!' | '?' | '。' | '！' | '？' | '｡' | '؟' | '۔' | '።' | '။' | '…'
    )
}

fn is_non_break_terminal_case(token: &str) -> bool {
    is_common_abbreviation(token)
        || is_single_letter_initial(token)
        || looks_like_decimal_number(token)
}

fn is_common_abbreviation(token: &str) -> bool {
    let lower = token.to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "mr."
            | "mrs."
            | "ms."
            | "dr."
            | "prof."
            | "sr."
            | "jr."
            | "st."
            | "no."
            | "vs."
            | "etc."
            | "e.g."
            | "i.e."
            | "u.s."
            | "u.k."
            | "jan."
            | "feb."
            | "mar."
            | "apr."
            | "jun."
            | "jul."
            | "aug."
            | "sep."
            | "sept."
            | "oct."
            | "nov."
            | "dec."
    ) || is_single_letter_initial(&lower)
}

fn is_single_letter_initial(token: &str) -> bool {
    let chars: Vec<char> = token.chars().collect();
    chars.len() == 2 && chars[0].is_ascii_alphabetic() && chars[1] == '.'
}

fn looks_like_decimal_number(token: &str) -> bool {
    let t = strip_trailing_closers(token);
    let mut parts = t.split('.');
    let left = match parts.next() {
        Some(v) => v,
        None => return false,
    };
    let right = match parts.next() {
        Some(v) => v,
        None => return false,
    };
    if parts.next().is_some() {
        return false;
    }
    !left.is_empty()
        && !right.is_empty()
        && left.chars().all(|c| c.is_ascii_digit())
        && right.chars().all(|c| c.is_ascii_digit())
}

fn starts_with_lowercase(token: &str) -> bool {
    token
        .chars()
        .find(|c| c.is_alphabetic())
        .map(|c| c.is_lowercase())
        .unwrap_or(false)
}
