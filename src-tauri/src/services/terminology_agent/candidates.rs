//! Deterministic candidate term extraction. The agent gets this menu in
//! round 1 so it does not burn search rounds discovering terms; search is
//! only for disambiguation. English-oriented; CJK transcripts yield few or
//! no candidates and the agent falls back to discovery.

use std::collections::HashMap;

use super::types::TranscriptCue;

pub const PHRASE_SLOTS: usize = 90;
pub const SINGLE_SLOTS: usize = 20;
pub const CANDIDATE_CAP: usize = PHRASE_SLOTS + SINGLE_SLOTS;
const MIN_COUNT_ABBR: usize = 2;
const MIN_COUNT_NAME: usize = 2;
const MIN_COUNT_NGRAM: usize = 3;
const MIN_COUNT_WORD: usize = 8;
const MIN_WORD_CHARS: usize = 4;
const MIN_PAIR_COUNT: usize = 3;

/// Single words too generic to be useful glossary rows on their own. Only
/// blocks the single-word candidate slot; n-grams like "swing high" are
/// unaffected (this list is not part of STOPWORDS).
const SINGLE_BLOCKLIST: &[&str] = &[
    "high", "low", "above", "below", "back", "start", "started", "notice", "time", "times",
    "year", "years", "month", "months", "price", "prices", "market", "markets", "they're",
    "level", "levels", "range", "open", "close", "long", "short", "hard", "higher", "lower",
    "up", "down", "trading", "buying", "selling", "sell", "buy", "going", "looking",
    "before", "doing", "side", "swing", "thing", "things", "point", "area", "way",
    "what's", "getting", "saying", "says", "everything", "here's", "trade",
];

/// Function words and discourse fillers only. Content-ish words (high, low,
/// up, down, open, close...) stay — "swing high" / "up close candle" are
/// real terms in some domains.
const STOPWORDS: &[&str] = &[
    "a", "an", "the", "and", "or", "but", "so", "of", "to", "in", "on", "at", "for", "with",
    "by", "from", "is", "are", "was", "were", "be", "been", "being", "it", "its", "this",
    "that", "these", "those", "you", "your", "yours", "i", "me", "my", "we", "our", "they",
    "them", "their", "he", "she", "his", "her", "do", "does", "did", "have", "has", "had",
    "will", "would", "can", "could", "should", "shall", "may", "might", "must", "not", "no",
    "if", "then", "than", "when", "while", "what", "which", "who", "how", "why", "as",
    "because", "until", "here", "there", "where", "now", "just", "like", "right", "okay",
    "ok", "yeah", "yes", "well", "really", "very", "too", "also", "only", "even", "about",
    "into", "out", "off", "over", "under", "again", "all", "some", "any", "each", "other",
    "another", "more", "most", "much", "many", "own", "same", "such", "going", "gonna",
    "want", "wanna", "get", "gets", "got", "let", "see", "look", "looking", "say", "saying",
    "said", "know", "think", "actually", "basically", "literally", "um", "uh", "ah", "oh",
    "guys", "thing", "things", "lot", "lots", "kind", "sort", "stuff", "way", "make",
    "makes", "made", "take", "takes", "took", "go", "goes", "went", "come", "comes", "came",
    "put", "keep", "keeps", "give", "gives", "use", "using", "used", "one", "two", "three",
    "first", "second", "next", "last", "every", "per", "vs", "etc", "am", "aren't", "isn't",
    "don't", "doesn't", "didn't", "can't", "won't", "i'm", "we're", "you're", "it's",
    "that's", "there's", "i've", "we've", "i'll", "you'll", "we'll", "let's", "i'd",
    "they're", "what's", "here's", "he's", "she's", "who's", "how's", "where's",
    "when's", "why's", "that'd", "it'd", "we'd", "they'd", "ain't",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub phrase: String,
    pub count: usize,
    pub example_index: usize,
}

fn is_stopword(w: &str) -> bool {
    STOPWORDS.contains(&w)
}

fn words(text: &str) -> Vec<&str> {
    text.split(|c: char| !c.is_alphanumeric() && c != '\'')
        .filter(|w| !w.is_empty())
        .collect()
}

fn is_abbrev(w: &str) -> bool {
    let n = w.chars().count();
    (2..=8).contains(&n)
        && w.chars().any(|c| c.is_ascii_alphabetic())
        && w.chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
}

fn is_title_word(w: &str) -> bool {
    let mut chars = w.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_uppercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
}

/// Collect candidate phrases with transcript-wide counts. Deterministic and
/// cheap; the LLM ranks/judges, it does not have to find these itself.
pub fn extract_candidates(cues: &[TranscriptCue]) -> Vec<Candidate> {
    #[derive(Default)]
    struct Acc {
        count: usize,
        first_idx: usize,
    }
    let mut abbrs: HashMap<String, Acc> = HashMap::new();
    let mut names: HashMap<String, Acc> = HashMap::new();
    let mut ngrams: HashMap<String, Acc> = HashMap::new();
    let mut single_words: HashMap<String, Acc> = HashMap::new();
    let mut lowercase_forms: std::collections::HashSet<String> = std::collections::HashSet::new();

    // Lowercase word inventory first: a Title word whose lowercase form also
    // appears is a common word at sentence start, not a name.
    for cue in cues {
        for w in words(&cue.text) {
            if w.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()) {
                lowercase_forms.insert(w.to_string());
            }
        }
    }

    let bump = |map: &mut HashMap<String, Acc>, key: String, idx: usize| {
        let e = map.entry(key).or_insert_with(|| Acc { count: 0, first_idx: idx });
        e.count += 1;
    };

    for cue in cues {
        let ws = words(&cue.text);
        for (i, w) in ws.iter().enumerate() {
            if is_abbrev(w) {
                bump(&mut abbrs, w.to_string(), cue.index);
            }
            // Title-case runs (1..=3 words), only when name-like: at least one
            // token never appears lowercase. Single letters ("I") excluded.
            if is_title_word(w) && w.len() >= 2 {
                let mut run = vec![*w];
                for nxt in ws.iter().skip(i + 1) {
                    if run.len() < 3 && (is_title_word(nxt) || is_abbrev(nxt)) {
                        run.push(nxt);
                    } else {
                        break;
                    }
                }
                for len in 1..=run.len() {
                    let phrase = run[..len].join(" ");
                    let name_like = run[..len].iter().any(|t| {
                        is_abbrev(t) || !lowercase_forms.contains(&t.to_ascii_lowercase())
                    });
                    if name_like {
                        bump(&mut names, phrase, cue.index);
                    }
                }
            }
            // Single words: case-insensitive (a term is a term at sentence
            // start too — "Rally back up" counts for 'rally'). Abbreviations
            // stay in the abbr path; names get deduped against below.
            let lw = w.to_ascii_lowercase();
            let wordish = w.chars().next().is_some_and(|c| c.is_alphabetic())
                && w.chars().all(|c| c.is_alphanumeric() || c == '\'');
            if wordish
                && !is_stopword(&lw)
                && !is_abbrev(w)
                && lw.chars().count() >= MIN_WORD_CHARS
                && !SINGLE_BLOCKLIST.contains(&lw.as_str())
            {
                bump(&mut single_words, lw.clone(), cue.index);
            }
            // N-grams (2..=3 words), case-insensitive: a phrase is a phrase
            // at sentence start too ("Buy side liquidity" counts for
            // 'buy side liquidity'). All-Title-case runs are skipped — the
            // name path above already covers those.
            if !is_stopword(&lw) {
                for len in 2..=3usize {
                    if i + len > ws.len() {
                        break;
                    }
                    let last = ws[i + len - 1].to_ascii_lowercase();
                    if is_stopword(&last) {
                        continue;
                    }
                    let wordish = ws[i..i + len].iter().all(|t| {
                        t.is_ascii()
                            && !t.is_empty()
                            && t.chars().all(|c| c.is_alphanumeric() || c == '\'')
                    });
                    if !wordish {
                        break;
                    }
                    let all_title = ws[i..i + len]
                        .iter()
                        .all(|t| is_title_word(t) || is_abbrev(t));
                    if all_title {
                        continue;
                    }
                    let phrase = ws[i..i + len]
                        .iter()
                        .map(|t| t.to_ascii_lowercase())
                        .collect::<Vec<_>>()
                        .join(" ");
                    bump(&mut ngrams, phrase, cue.index);
                }
            }
        }
    }

    let mut phrases: Vec<Candidate> = Vec::new();
    let mut singles: Vec<Candidate> = Vec::new();
    let push = |map: HashMap<String, Acc>, min: usize, into: &mut Vec<Candidate>| {
        for (phrase, acc) in map {
            if acc.count >= min {
                into.push(Candidate {
                    phrase,
                    count: acc.count,
                    example_index: acc.first_idx,
                });
            }
        }
    };
    push(abbrs, MIN_COUNT_ABBR, &mut phrases);
    push(names, MIN_COUNT_NAME, &mut phrases);
    push(ngrams, MIN_COUNT_NGRAM, &mut phrases);
    push(single_words, MIN_COUNT_WORD, &mut singles);

    let by_rank = |a: &Candidate, b: &Candidate| {
        b.count
            .cmp(&a.count)
            .then_with(|| b.phrase.chars().count().cmp(&a.phrase.chars().count()))
            .then_with(|| a.phrase.cmp(&b.phrase))
    };
    phrases.sort_by(by_rank);
    phrases.truncate(PHRASE_SLOTS);
    // Drop bigrams fully explained by a trigram of similar frequency — but
    // only when that trigram actually survived the slot cut. Applying this
    // before truncation could lose both (trigram cut by slots, bigram already
    // suppressed), which silently drops real terms from the discovery list.
    let tri_counts: HashMap<String, usize> = phrases
        .iter()
        .filter(|c| c.phrase.split(' ').count() == 3)
        .map(|c| (c.phrase.clone(), c.count))
        .collect();
    phrases.retain(|c| {
        if c.phrase.split(' ').count() != 2 {
            return true;
        }
        !tri_counts
            .iter()
            .any(|(t, tc)| t.contains(c.phrase.as_str()) && *tc * 5 >= c.count * 4)
    });
    // Drop singles whose occurrences are mostly explained by a phrase
    // candidate ("frame" ⊂ "time frame"), or that duplicate a name/abbr
    // candidate ("October" name vs "october" single).
    singles.retain(|s| {
        !phrases.iter().any(|p| {
            p.phrase.split(' ').any(|w| w == s.phrase) && p.count * 2 >= s.count
        }) && !phrases.iter().any(|p| p.phrase.eq_ignore_ascii_case(&s.phrase))
    });
    singles.sort_by(by_rank);
    singles.truncate(SINGLE_SLOTS);
    phrases.extend(singles);
    phrases.truncate(CANDIDATE_CAP);
    phrases
}

/// One-word-substitution pairs among candidates where the differing words
/// are plausibly confusable by ASR: same initial letter (or both
/// vowel-initial) and similar length — e.g. a short word misheard as another
/// short word with the same onset. Clearly different words ("buy side" vs
/// "sell side", "range high" vs "swing high") are not ASR suspects, only
/// ordinary neighbours; flagging them would drown the real danger zones.
/// The harness only detects; the model adjudicates — and may additionally
/// register its own suspect pairs via the flag_pair tool, which go through
/// the same submit gate. Singles are never paired
/// (any two differ once).
pub fn find_confusable_pairs(cands: &[Candidate]) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    for i in 0..cands.len() {
        for j in (i + 1)..cands.len() {
            let (a, b) = (&cands[i], &cands[j]);
            if a.count < MIN_PAIR_COUNT || b.count < MIN_PAIR_COUNT {
                continue;
            }
            let wa: Vec<&str> = a.phrase.split(' ').collect();
            let wb: Vec<&str> = b.phrase.split(' ').collect();
            if wa.len() < 2 || wa.len() != wb.len() {
                continue;
            }
            let diffs: Vec<(&str, &str)> = wa
                .iter()
                .zip(&wb)
                .filter(|(x, y)| x != y)
                .map(|(x, y)| (*x, *y))
                .collect();
            if diffs.len() == 1
                && suspicious_substitution(diffs[0].0, diffs[0].1)
                && !is_inflection_pair(diffs[0].0, diffs[0].1)
            {
                pairs.push((a.phrase.clone(), b.phrase.clone()));
            }
        }
    }
    pairs
}

/// Could ASR plausibly confuse these two words? Same initial letter (or both
/// vowel-initial) and length within two characters. Purely orthographic —
/// good enough to separate mishearing suspects from ordinary antonyms.
fn suspicious_substitution(x: &str, y: &str) -> bool {
    let fx = x.chars().next();
    let fy = y.chars().next();
    let vowel_initial = |c: Option<char>| matches!(c, Some('a' | 'e' | 'i' | 'o' | 'u'));
    if fx != fy && !(vowel_initial(fx) && vowel_initial(fy)) {
        return false;
    }
    x.chars().count().abs_diff(y.chars().count()) <= 2
}

/// Morphological relatives are never an ASR suspect: plural/inflection
/// variants of the same lexeme ("shift"/"shifts", "tendency"/"tendencies")
/// and stem extensions where one word grows out of the other
/// ("Korea"/"Korean", "Cuba"/"Cuban") — a country/adjective-style pair is a
/// real distinction, not a mishearing. The glossary's same-concept rule
/// already covers them. Requires a 4+ char shared prefix so trivial overlaps
/// ("do"/"dog") don't exempt a real substitution.
fn is_inflection_pair(x: &str, y: &str) -> bool {
    let stems = |w: &str| -> Vec<String> {
        let mut v = vec![w.to_string()];
        if let Some(s) = w.strip_suffix('s') {
            v.push(s.to_string());
        }
        if let Some(s) = w.strip_suffix("ies") {
            v.push(format!("{s}y"));
        }
        v
    };
    let ys = stems(y);
    if stems(x).iter().any(|sx| ys.contains(sx)) {
        return true;
    }
    let (short, long) = if x.len() <= y.len() { (x, y) } else { (y, x) };
    short.chars().count() >= 4 && long.starts_with(short)
}

/// Up to `n` verbatim transcript lines containing `surface`, formatted as
/// `[#idx] text` (text capped at 140 chars). Shared by the candidates block
/// and the submit-gate rejection messages.
pub fn surface_example_lines(surface: &str, cues: &[TranscriptCue], n: usize) -> Vec<String> {
    let needle = surface.to_lowercase();
    let mut lines = Vec::new();
    for cue in cues {
        if cue.text.to_lowercase().contains(&needle) {
            let text: String = cue.text.chars().take(140).collect();
            lines.push(format!("  [#{}] {text}", cue.index));
            if lines.len() == n {
                break;
            }
        }
    }
    lines
}

/// Verbatim transcript lines for every surface of a confusable pair, so the
/// model adjudicates from grounded evidence without spending probes.
pub fn format_pair_evidence(pairs: &[(String, String)], cues: &[TranscriptCue]) -> String {
    if pairs.is_empty() {
        return String::new();
    }
    let mut out = String::from(
        "\n=== ⚠ PAIR EVIDENCE (verbatim transcript lines — adjudicate these pairs from HERE, no probes needed) ===\n",
    );
    let mut seen = std::collections::HashSet::new();
    for (a, b) in pairs {
        out.push_str(&format!("pair: '{a}' vs '{b}'\n"));
        for surface in [a, b] {
            if !seen.insert(surface.as_str()) {
                continue;
            }
            out.push_str(&format!(
                "'{surface}':\n{}\n",
                surface_example_lines(surface, cues, 2).join("\n")
            ));
        }
    }
    out
}

pub fn format_candidates_block(cands: &[Candidate], pairs: &[(String, String)]) -> String {
    if cands.is_empty() {
        return "(none extracted — discover terms by reading the transcript)".to_string();
    }
    let counts: HashMap<&str, usize> = cands
        .iter()
        .map(|c| (c.phrase.as_str(), c.count))
        .collect();
    cands
        .iter()
        .map(|c| {
            let warns: Vec<String> = pairs
                .iter()
                .filter_map(|(a, b)| {
                    if a == &c.phrase {
                        Some(format!("{b} ({}x)", counts.get(b.as_str()).unwrap_or(&0)))
                    } else if b == &c.phrase {
                        Some(format!("{a} ({}x)", counts.get(a.as_str()).unwrap_or(&0)))
                    } else {
                        None
                    }
                })
                .collect();
            let suffix = if warns.is_empty() {
                String::new()
            } else {
                format!("  ⚠ confusable with {}", warns.join("; "))
            };
            format!(
                "  - {}x '{}' (e.g. cue #{}){suffix}",
                c.count, c.phrase, c.example_index
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cue(i: usize, text: &str) -> TranscriptCue {
        TranscriptCue {
            index: i,
            start_ms: i as u64 * 1000,
            text: text.into(),
        }
    }

    #[test]
    fn finds_abbreviations_names_and_ngrams() {
        let cues = vec![
            cue(1, "We look at SMT divergence on the order block."),
            cue(2, "Another order block here, and SMT again."),
            cue(3, "The order block holds. Barchart com shows it."),
            cue(4, "Barchart com is where I check the FVG and the order block."),
            cue(5, "FVG on the chart, FVG again, and the FVG holds."),
        ];
        let cands = extract_candidates(&cues);
        let phrases: Vec<&str> = cands.iter().map(|c| c.phrase.as_str()).collect();
        assert!(phrases.contains(&"SMT"), "got {phrases:?}");
        assert!(phrases.contains(&"FVG"), "got {phrases:?}");
        assert!(phrases.contains(&"order block"), "got {phrases:?}");
        assert!(phrases.contains(&"Barchart"), "got {phrases:?}");
        // "the order block" / "on the" style junk filtered by stopword ends.
        assert!(!phrases.iter().any(|p| p.starts_with("the ")), "got {phrases:?}");
        assert!(!phrases.contains(&"and"), "got {phrases:?}");
    }

    #[test]
    fn bigram_dropped_when_trigram_covers_it() {
        let cues: Vec<TranscriptCue> = (1..=5)
            .map(|i| cue(i, "change in the state of delivery matters"))
            .collect();
        let cands = extract_candidates(&cues);
        let phrases: Vec<&str> = cands.iter().map(|c| c.phrase.as_str()).collect();
        assert!(phrases.contains(&"state of delivery"), "got {phrases:?}");
        assert!(!phrases.contains(&"of delivery"), "got {phrases:?}");
    }

    #[test]
    fn common_word_capitalized_at_sentence_start_is_not_a_name() {
        let cues = vec![
            cue(1, "Premium matters here. We sell at premium."),
            cue(2, "Premium again, but premium is a common word here."),
        ];
        let cands = extract_candidates(&cues);
        let phrases: Vec<&str> = cands.iter().map(|c| c.phrase.as_str()).collect();
        assert!(!phrases.contains(&"Premium"), "got {phrases:?}");
    }

    #[test]
    fn cjk_transcript_yields_no_candidates_but_does_not_panic() {
        let cues = vec![cue(1, "这里是峡谷深处"), cue(2, "我们继续前进")];
        let cands = extract_candidates(&cues);
        assert!(cands.is_empty());
        assert!(format_candidates_block(&cands, &[]).contains("none"));
    }

    #[test]
    fn confusable_pairs_flag_one_word_substitutions() {
        let mk = |phrase: &str, count: usize| Candidate {
            phrase: phrase.into(),
            count,
            example_index: 1,
        };
        let cands = vec![
            mk("hard time frame", 20),
            mk("high time frame", 16),
            mk("buy side liquidity", 9),
            mk("sell side liquidity", 8),
            mk("range high", 10),
            mk("swing high", 8),
            mk("premium", 12),
            mk("discount", 10),
        ];
        let pairs = find_confusable_pairs(&cands);
        // Same-initial, similar-length substitution: a mishearing suspect.
        assert!(pairs.contains(&("hard time frame".into(), "high time frame".into())));
        // Clearly different words are ordinary neighbours, not ASR suspects.
        assert!(!pairs.iter().any(|(a, b)| a.contains("buy side") || b.contains("buy side")));
        assert!(!pairs.iter().any(|(a, b)| a == "range high" || b == "range high"));
        // Plural inflections of the same lexeme are never flagged.
        let inflected = vec![mk("seasonal tendency", 18), mk("seasonal tendencies", 5)];
        assert!(find_confusable_pairs(&inflected).is_empty());
        // Stem extensions (country/adjective-style morphology) are real
        // distinctions, not mishearings — never flagged.
        let stemmed = vec![mk("north korea", 30), mk("north korean", 12)];
        assert!(find_confusable_pairs(&stemmed).is_empty());
        // Singles never pair with each other.
        assert!(!pairs.iter().any(|(a, b)| a == "premium" || b == "premium"));
        let block = format_candidates_block(&cands, &pairs);
        assert!(block.contains("⚠ confusable with high time frame (16x)"));
    }

    #[test]
    fn pair_evidence_quotes_verbatim_lines_for_each_surface() {
        let cues = vec![
            cue(1, "We traded into a hard time frame key level here."),
            cue(2, "A high probability system for high time frame bias."),
        ];
        let pairs = vec![("hard time frame".to_string(), "high time frame".to_string())];
        let ev = format_pair_evidence(&pairs, &cues);
        assert!(ev.contains("pair: 'hard time frame' vs 'high time frame'"));
        assert!(ev.contains("[#1] We traded into a hard time frame key level here."));
        assert!(ev.contains("[#2] A high probability system for high time frame bias."));
        assert!(format_pair_evidence(&[], &cues).is_empty());
    }

    #[test]
    fn single_words_get_reserved_slots_after_phrases() {
        // "premium" sits next to varying neighbours so no phrase explains it;
        // "frame" only ever occurs inside "time frame" and must be dropped.
        let mut cues = Vec::new();
        for i in 1..=12 {
            let text = match i % 3 {
                0 => "premium at order block, time frame",
                1 => "order block near premium, time frame",
                _ => "with premium, order block time frame",
            };
            cues.push(cue(i, text));
        }
        let cands = extract_candidates(&cues);
        assert!(cands.iter().any(|c| c.phrase == "premium"));
        assert!(cands.iter().any(|c| c.phrase == "order block"));
        assert!(cands.iter().any(|c| c.phrase == "time frame"));
        assert!(!cands.iter().any(|c| c.phrase == "frame"));
        // Blocklisted generic words stay out.
        assert!(!cands.iter().any(|c| c.phrase == "high"));
    }
}
