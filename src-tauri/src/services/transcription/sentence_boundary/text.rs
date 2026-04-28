pub(super) fn normalize_ascii_word(raw: &str) -> String {
    raw.chars()
        .filter(|ch| ch.is_ascii_alphabetic() || *ch == '\'')
        .flat_map(|ch| ch.to_lowercase())
        .collect::<String>()
}

pub(super) fn is_semantic_clause_start(word: &str) -> bool {
    matches!(
        word,
        "although"
            | "and"
            | "as"
            | "because"
            | "before"
            | "but"
            | "due"
            | "except"
            | "if"
            | "just"
            | "maybe"
            | "or"
            | "otherwise"
            | "since"
            | "so"
            | "then"
            | "though"
            | "unless"
            | "until"
            | "when"
            | "where"
            | "while"
            | "which"
            | "yet"
    )
}

pub(super) fn is_pronoun_or_auxiliary_start(word: &str) -> bool {
    matches!(
        word,
        "i" | "you"
            | "he"
            | "she"
            | "it"
            | "we"
            | "they"
            | "is"
            | "are"
            | "was"
            | "were"
            | "am"
            | "do"
            | "does"
            | "did"
            | "can"
            | "could"
            | "will"
            | "would"
            | "should"
            | "might"
            | "may"
    )
}

pub(super) fn is_bad_segment_start_word(word: &str) -> bool {
    matches!(
        word,
        "a" | "an"
            | "at"
            | "by"
            | "for"
            | "from"
            | "in"
            | "into"
            | "of"
            | "on"
            | "the"
            | "to"
            | "with"
    )
}

pub(super) fn is_dangling_tail_word(word: &str) -> bool {
    matches!(
        word,
        "a" | "an"
            | "and"
            | "as"
            | "at"
            | "because"
            | "before"
            | "but"
            | "by"
            | "for"
            | "from"
            | "if"
            | "in"
            | "into"
            | "of"
            | "on"
            | "or"
            | "so"
            | "that"
            | "the"
            | "then"
            | "to"
            | "when"
            | "where"
            | "which"
            | "while"
            | "with"
    )
}

pub(super) fn ends_with_terminal_punctuation(word: &str) -> bool {
    word.trim_end()
        .chars()
        .last()
        .map(|ch| matches!(ch, '.' | '!' | '?' | '。' | '！' | '？'))
        .unwrap_or(false)
}

pub(super) fn ends_with_soft_punctuation(word: &str) -> bool {
    word.trim_end()
        .chars()
        .last()
        .map(|ch| matches!(ch, ',' | ';' | ':' | '，' | '；' | '：' | '、'))
        .unwrap_or(false)
}

pub(super) fn join_words<'a>(parts: impl Iterator<Item = &'a str>) -> String {
    let mut out = String::new();
    let mut prev_has_spacing_word = false;
    let mut prev_allows_space_after = false;

    for raw in parts {
        let token = raw.trim();
        if token.is_empty() {
            continue;
        }
        let next_has_spacing_word = token_has_spacing_word(token);
        if !out.is_empty()
            && next_has_spacing_word
            && (prev_has_spacing_word || prev_allows_space_after)
        {
            out.push(' ');
        }
        out.push_str(token);
        prev_has_spacing_word = next_has_spacing_word;
        prev_allows_space_after = token_allows_space_after(token);
    }

    out.replace(" ,", ",")
        .replace(" .", ".")
        .replace(" !", "!")
        .replace(" ?", "?")
        .replace(" :", ":")
        .replace(" ;", ";")
}

fn token_allows_space_after(token: &str) -> bool {
    token
        .chars()
        .last()
        .map(|ch| {
            matches!(
                ch,
                ',' | ';' | ':' | '?' | '!' | '.' | '，' | '；' | '：' | '？' | '！' | '。'
            )
        })
        .unwrap_or(false)
}

fn token_has_spacing_word(token: &str) -> bool {
    token
        .chars()
        .any(|ch| ch.is_ascii_alphanumeric() || is_hangul(ch))
}

fn is_hangul(ch: char) -> bool {
    matches!(
        ch as u32,
        0x1100..=0x11FF
            | 0x3130..=0x318F
            | 0xA960..=0xA97F
            | 0xAC00..=0xD7AF
            | 0xD7B0..=0xD7FF
    )
}
