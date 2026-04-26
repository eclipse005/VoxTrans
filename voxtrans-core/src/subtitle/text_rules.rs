pub fn should_split_after_terminal_token(current_token: &str, next_token: Option<&str>) -> bool {
    let normalized = strip_trailing_closers(current_token.trim());
    if normalized.is_empty() || !ends_with_terminal_punctuation(normalized) {
        return false;
    }
    if is_non_break_terminal_case(normalized) {
        return false;
    }
    if let Some(next) = next_token {
        let next_token = strip_leading_openers(next.trim());
        if starts_with_lowercase(next_token) {
            return false;
        }
    }
    true
}

pub fn has_break_terminal_punctuation(token: &str) -> bool {
    let normalized = strip_trailing_closers(token.trim());
    !normalized.is_empty()
        && ends_with_terminal_punctuation(normalized)
        && !is_non_break_terminal_case(normalized)
}

pub fn ends_with_terminal_punctuation(word: &str) -> bool {
    word.chars()
        .last()
        .map(is_terminal_punctuation)
        .unwrap_or(false)
}

pub fn strip_trailing_closers(token: &str) -> &str {
    token.trim_end_matches(|c: char| {
        matches!(
            c,
            '"' | '\'' | ')' | ']' | '}' | '”' | '’' | '》' | '」' | '』'
        )
    })
}

pub fn strip_leading_openers(token: &str) -> &str {
    token.trim_start_matches(|c: char| {
        matches!(
            c,
            '"' | '\'' | '(' | '[' | '{' | '“' | '‘' | '《' | '「' | '『'
        )
    })
}

pub fn is_non_break_terminal_case(token: &str) -> bool {
    is_common_abbreviation(token)
        || is_single_letter_initial(token)
        || looks_like_decimal_number(token)
}

pub fn starts_with_lowercase(token: &str) -> bool {
    token
        .chars()
        .find(|c| c.is_alphabetic())
        .map(|c| c.is_lowercase())
        .unwrap_or(false)
}

fn is_terminal_punctuation(c: char) -> bool {
    matches!(
        c,
        '.' | '!' | '?' | '。' | '！' | '？' | '｡' | '؟' | '۔' | '።' | '။' | '…'
    )
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
            | "a.m."
            | "p.m."
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

#[cfg(test)]
mod tests {
    use super::{
        has_break_terminal_punctuation, is_non_break_terminal_case,
        should_split_after_terminal_token,
    };

    #[test]
    fn am_pm_is_non_break() {
        assert!(is_non_break_terminal_case("a.m."));
        assert!(is_non_break_terminal_case("p.m."));
        assert!(!has_break_terminal_punctuation("a.m."));
        assert!(!has_break_terminal_punctuation("p.m."));
    }

    #[test]
    fn decimal_is_non_break() {
        assert!(is_non_break_terminal_case("3.14"));
        assert!(!has_break_terminal_punctuation("3.14"));
    }

    #[test]
    fn regular_sentence_terminal_breaks() {
        assert!(has_break_terminal_punctuation("world."));
        assert!(should_split_after_terminal_token("world.", Some("Next")));
    }

    #[test]
    fn lowercase_next_word_suppresses_break() {
        assert!(!should_split_after_terminal_token("hello.", Some("world")));
    }
}
