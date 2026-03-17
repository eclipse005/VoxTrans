use super::WordToken;

pub(super) fn recursive_balance_split(words: &[WordToken], max_words: usize) -> Vec<Vec<WordToken>> {
    if words.len() <= max_words {
        return vec![words.to_vec()];
    }

    let (split_idx, min_split_cost) = find_best_split_point(words);
    let over_limit = (words.len().saturating_sub(max_words)) as f64;
    let do_nothing_cost = over_limit * over_limit;

    if words.len() <= max_words * 2 && min_split_cost > do_nothing_cost {
        return vec![words.to_vec()];
    }

    let left = &words[..=split_idx];
    let right = &words[split_idx + 1..];
    if left.is_empty() || right.is_empty() {
        return vec![words.to_vec()];
    }

    let mut out = recursive_balance_split(left, max_words);
    out.extend(recursive_balance_split(right, max_words));
    out
}

fn find_best_split_point(words: &[WordToken]) -> (usize, f64) {
    let center = words.len() / 2;
    let mut best_idx = center.saturating_sub(1);
    let mut min_cost = f64::INFINITY;

    for i in 0..words.len().saturating_sub(1) {
        let balance_cost = ((i as isize - center as isize).abs() as f64) * 1.5;

        let mut semantic_bonus = 0.0_f64;
        if ends_with_comma(&words[i].word) {
            semantic_bonus += 15.0;
        }
        if is_connector_at(i + 1, words) {
            semantic_bonus += 12.0;
        }

        let structure_penalty = if i == 0 || i == words.len() - 2 {
            60.0
        } else {
            0.0
        };

        let total_cost = balance_cost - semantic_bonus + structure_penalty;
        if total_cost < min_cost {
            min_cost = total_cost;
            best_idx = i;
        }
    }

    (best_idx, min_cost)
}

fn ends_with_comma(word: &str) -> bool {
    strip_trailing_closers(word.trim())
        .chars()
        .last()
        .map(|c| matches!(c, ',' | '，' | '、'))
        .unwrap_or(false)
}

fn is_connector_at(idx: usize, words: &[WordToken]) -> bool {
    const CONNECTORS: &[&str] = &[
        "and",
        "or",
        "but",
        "yet",
        "however",
        "because",
        "if",
        "when",
        "where",
        "while",
        "although",
        "though",
        "therefore",
        "consequently",
        "so",
        "then",
        "also",
        "meanwhile",
    ];
    let token = match words.get(idx) {
        Some(w) => normalize_word_for_match(&w.word),
        None => return false,
    };
    CONNECTORS.contains(&token.as_str())
}

fn normalize_word_for_match(word: &str) -> String {
    strip_trailing_closers(word.trim())
        .trim_end_matches(|c: char| {
            matches!(
                c,
                '.' | ',' | '!' | '?' | ':' | ';' | '。' | '，' | '！' | '？' | '：' | '；' | '、'
            )
        })
        .to_ascii_lowercase()
}

fn strip_trailing_closers(token: &str) -> &str {
    token.trim_end_matches(|c: char| {
        matches!(
            c,
            '"' | '\'' | ')' | ']' | '}' | '”' | '’' | '》' | '」' | '』'
        )
    })
}
