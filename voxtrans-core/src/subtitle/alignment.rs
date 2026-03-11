use super::segmenter::WordToken;

/// Align full text (possibly corrected by LLM) back onto ASR word timestamps.
///
/// Strategy:
/// - Character-level matching on alphanumeric-only cleaned strings.
/// - For each matched word, attach trailing punctuation from the original full text.
/// - Keep original word timestamp for the attached punctuation.
/// - If matching quality is too low (< 50%), fallback to original timestamps.
pub fn align_text_to_timestamps(full_text: &str, word_timestamps: &[WordToken]) -> Vec<WordToken> {
    if full_text.is_empty() || word_timestamps.is_empty() {
        return Vec::new();
    }

    let full_chars: Vec<char> = full_text.chars().collect();
    let mut clean_to_original: Vec<usize> = Vec::new();
    for (idx, ch) in full_chars.iter().enumerate() {
        if is_word_char(*ch) {
            clean_to_original.push(idx);
        }
    }

    if clean_to_original.is_empty() {
        return word_timestamps.to_vec();
    }

    let clean_chars: Vec<char> = clean_to_original.iter().map(|&i| full_chars[i]).collect();
    let cleaned_words: Vec<Vec<char>> = word_timestamps
        .iter()
        .map(|w| {
            w.word
                .chars()
                .filter(|c| is_word_char(*c))
                .collect::<Vec<char>>()
        })
        .collect();

    let mut result: Vec<WordToken> = Vec::new();
    let mut word_idx = 0usize;
    let mut clean_pos = 0usize;

    while word_idx < word_timestamps.len() && clean_pos < clean_chars.len() {
        let target = &cleaned_words[word_idx];
        if target.is_empty() {
            word_idx += 1;
            continue;
        }

        let target_len = target.len();
        if clean_pos + target_len > clean_chars.len() {
            break;
        }

        if eq_ignore_case_chars(&clean_chars[clean_pos..clean_pos + target_len], target) {
            let match_start_clean = clean_pos;
            let match_end_clean = clean_pos + target_len;

            let original_start = clean_to_original[match_start_clean];
            let original_end_exclusive = clean_to_original[match_end_clean - 1] + 1;

            let mut following_end = original_end_exclusive;
            while following_end < full_chars.len() && full_chars[following_end].is_whitespace() {
                following_end += 1;
            }
            while following_end < full_chars.len() {
                let ch = full_chars[following_end];
                if is_word_char(ch) {
                    break;
                }
                following_end += 1;
            }

            let text_segment: String = full_chars[original_start..following_end]
                .iter()
                .collect::<String>()
                .trim_end()
                .to_string();

            let original = &word_timestamps[word_idx];
            result.push(WordToken {
                word: if text_segment.is_empty() {
                    original.word.clone()
                } else {
                    text_segment
                },
                start: original.start,
                end: original.end,
            });

            clean_pos = match_end_clean;
            word_idx += 1;
        } else {
            clean_pos += 1;
            if clean_pos >= clean_chars.len() {
                word_idx += 1;
                clean_pos = 0;
            }
        }
    }

    let matched = result.len() as f64;
    let total = word_timestamps.len() as f64;
    if total > 0.0 && matched < total * 0.5 {
        return word_timestamps.to_vec();
    }

    if result.is_empty() {
        word_timestamps.to_vec()
    } else {
        result
    }
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric()
}

fn eq_ignore_case_chars(a: &[char], b: &[char]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter()
        .zip(b.iter())
        .all(|(x, y)| x.to_lowercase().to_string() == y.to_lowercase().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wt(word: &str, start: f64, end: f64) -> WordToken {
        WordToken {
            word: word.to_string(),
            start,
            end,
        }
    }

    #[test]
    fn attaches_following_punctuation() {
        let full = "Hello, world! This is test.";
        let words = vec![
            wt("Hello", 0.0, 0.3),
            wt("world", 0.3, 0.6),
            wt("This", 0.6, 0.8),
            wt("is", 0.8, 0.9),
            wt("test", 0.9, 1.2),
        ];
        let out = align_text_to_timestamps(full, &words);

        assert_eq!(out[0].word, "Hello,");
        assert_eq!(out[1].word, "world!");
        assert_eq!(out[4].word, "test.");
        assert_eq!(out[0].start, 0.0);
        assert_eq!(out[0].end, 0.3);
    }

    #[test]
    fn handles_hyphen_format_difference() {
        let full = "seventy-one people.";
        let words = vec![wt("seventyone", 0.0, 0.4), wt("people", 0.4, 0.8)];
        let out = align_text_to_timestamps(full, &words);

        assert_eq!(out.len(), 2);
        assert_eq!(out[0].word, "seventy-one");
        assert_eq!(out[1].word, "people.");
    }

    #[test]
    fn fallback_when_match_quality_too_low() {
        let full = "totally unrelated content";
        let words = vec![
            wt("hello", 0.0, 0.2),
            wt("world", 0.2, 0.5),
            wt("test", 0.5, 0.8),
        ];
        let out = align_text_to_timestamps(full, &words);

        assert_eq!(out.len(), words.len());
        assert_eq!(out[0].word, "hello");
        assert_eq!(out[1].word, "world");
        assert_eq!(out[2].word, "test");
    }
}
