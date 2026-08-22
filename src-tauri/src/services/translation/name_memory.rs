//! Cross-batch name memory derived from already-committed translations.
//!
//! No extra LLM call: after each batch we already have `source → translation`
//! pairs. Recurring marked-script spans (katakana, or Latin inside CJK text)
//! are replayed into later prompts so the translator can keep using the same
//! rendering. User-supplied terminology still wins on conflict.

use std::collections::{HashMap, HashSet};

use crate::services::prompts::translation::{TranslationNameExample, TranslationPromptTerm};

use super::types::NormalizedSegment;

const MAX_NAME_EXAMPLES: usize = 24;

#[derive(Debug, Clone)]
pub(super) struct NameMemory {
    pub terms: Vec<TranslationPromptTerm>,
    pub examples: Vec<TranslationNameExample>,
}

/// Build name memory from committed translations, keeping only names that
/// appear in `current_batch_text`. First-seen example wins (segment id order).
pub(super) fn derive_name_memory(
    segments: &[NormalizedSegment],
    known_translations: &HashMap<usize, String>,
    current_batch_text: &str,
) -> NameMemory {
    let current_norm = normalize_for_match(current_batch_text);
    if current_norm.is_empty() {
        return NameMemory {
            terms: Vec::new(),
            examples: Vec::new(),
        };
    }
    let current_spans = name_like_spans(current_batch_text);

    let mut seen = HashSet::<String>::new();
    let mut terms = Vec::new();
    let mut examples = Vec::new();

    let mut ordered: Vec<&NormalizedSegment> = segments.iter().collect();
    ordered.sort_by_key(|s| s.segment_id);

    for segment in ordered {
        let Some(translation) = known_translations.get(&segment.segment_id) else {
            continue;
        };
        if translation.trim().is_empty() {
            continue;
        }
        for span in name_like_spans(&segment.source) {
            for form in forms_present_in_current(&span, &current_spans, &current_norm) {
                let key = normalize_for_match(&form);
                if key.is_empty() {
                    continue;
                }
                if !seen.insert(key.clone()) {
                    // First example already stored. If a later line yields a
                    // marked-script lock (phonetic / passthrough), add it —
                    // terminology verbatim beats a common-noun first example.
                    if terms
                        .iter()
                        .any(|t: &TranslationPromptTerm| normalize_for_match(&t.source) == key)
                    {
                        continue;
                    }
                    if let Some(target) = bind_marked_target(&form, translation)
                        .or_else(|| bind_marked_target(&span, translation))
                    {
                        terms.push(TranslationPromptTerm {
                            source: form,
                            target,
                            note: String::new(),
                        });
                    }
                    continue;
                }
                examples.push(TranslationNameExample {
                    source: form.clone(),
                    example_source: segment.source.clone(),
                    example_translation: translation.clone(),
                });
                if let Some(target) = bind_marked_target(&form, translation)
                    .or_else(|| bind_marked_target(&span, translation))
                {
                    terms.push(TranslationPromptTerm {
                        source: form,
                        target,
                        note: String::new(),
                    });
                }
                if examples.len() >= MAX_NAME_EXAMPLES {
                    return NameMemory { terms, examples };
                }
            }
        }
    }

    NameMemory { terms, examples }
}

pub(super) fn normalize_for_match(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect()
}

/// Marked-script name spans: katakana runs, and Latin runs inside CJK-majority
/// text. Not a named-entity model — script contrast is the signal.
fn forms_present_in_current(
    span: &str,
    current_spans: &[String],
    current_norm: &str,
) -> Vec<String> {
    let span_norm = normalize_for_match(span);
    let mut forms = Vec::new();
    if current_norm.contains(&span_norm) {
        forms.push(span.to_string());
    }
    for current in current_spans {
        if current == span {
            continue;
        }
        if span.contains(current.as_str()) || current.contains(span) {
            if current.chars().filter(|c| !c.is_ascii_punctuation()).count() >= 3
                && !forms.iter().any(|f| f == current)
            {
                forms.push(current.clone());
            }
        }
    }
    forms
}

pub(super) fn name_like_spans(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    collect_runs(text, is_katakana, 2, &mut out);
    let has_cjk = text.chars().any(is_cjk_ideograph_or_kana);
    if has_cjk {
        collect_runs(text, is_latin_letter, 2, &mut out);
    }
    out
}

fn collect_runs(text: &str, pred: fn(char) -> bool, min: usize, out: &mut Vec<String>) {
    let mut current = String::new();
    for ch in text.chars() {
        if pred(ch) || (!current.is_empty() && is_name_run_glue(ch, current.chars().last())) {
            current.push(ch);
            continue;
        }
        push_run(&mut current, min, out);
    }
    push_run(&mut current, min, out);
}

fn is_name_run_glue(ch: char, last: Option<char>) -> bool {
    let Some(last) = last else {
        return false;
    };
    // Keep ー inside katakana; keep '.' inside Latin abbreviations (S.B.C).
    ((ch == 'ー' || ch == 'ｰ') && is_katakana(last)) || (ch == '.' && is_latin_letter(last))
}

fn push_run(current: &mut String, min: usize, out: &mut Vec<String>) {
    let letters = current.chars().filter(|c| c.is_alphanumeric() || is_katakana(*c)).count();
    if letters >= min {
        out.push(std::mem::take(current));
    } else {
        current.clear();
    }
}

/// Lock a target only when it stays in a marked script (passthrough or
/// Latin/kana transcription). Semantic common-noun renderings are not locked.
fn bind_marked_target(name: &str, translation: &str) -> Option<String> {
    if translation.contains(name) {
        return Some(name.to_string());
    }
    let name_kana = name.chars().any(is_katakana);
    let name_latin = name.chars().any(is_latin_letter);
    let trans_latin: Vec<String> = {
        let mut v = Vec::new();
        collect_runs(translation, is_latin_letter, 2, &mut v);
        v
    };
    let trans_kana: Vec<String> = {
        let mut v = Vec::new();
        collect_runs(translation, is_katakana, 2, &mut v);
        v
    };
    if name_kana && trans_latin.len() == 1 && trans_kana.is_empty() {
        return Some(trans_latin[0].clone());
    }
    if name_latin && trans_latin.iter().any(|t| t.eq_ignore_ascii_case(name)) {
        return Some(name.to_string());
    }
    None
}

fn is_katakana(ch: char) -> bool {
    matches!(ch as u32, 0x30A0..=0x30FF | 0xFF66..=0xFF9D)
        && ch != '・'
        && ch != '゠'
}

fn is_latin_letter(ch: char) -> bool {
    ch.is_ascii_alphabetic()
}

fn is_cjk_ideograph_or_kana(ch: char) -> bool {
    matches!(
        ch as u32,
        0x3040..=0x30FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xFF66..=0xFF9D
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_katakana_and_latin_inside_cjk() {
        let spans = name_like_spans("世界一可愛いプリンセスキャニオンです SNS");
        assert!(spans.iter().any(|s| s.contains("プリンセスキャニオン")), "{spans:?}");
        assert!(spans.iter().any(|s| s == "SNS"), "{spans:?}");
    }

    #[test]
    fn does_not_extract_latin_from_english_only_line() {
        let spans = name_like_spans("Hello world from the market");
        assert!(spans.is_empty(), "{spans:?}");
    }

    #[test]
    fn locks_passthrough_and_latin_transcription_not_common_nouns() {
        assert_eq!(
            bind_marked_target("キャニオン", "我是Canyon"),
            Some("Canyon".to_string())
        );
        assert_eq!(
            bind_marked_target("キャニオン", "我是キャニオン"),
            Some("キャニオン".to_string())
        );
        assert_eq!(bind_marked_target("キャニオン", "我是世界第一可爱的公主峡谷"), None);
    }

    #[test]
    fn first_example_wins_and_filters_to_current_batch() {
        let segments = vec![
            NormalizedSegment {
                segment_id: 1,
                start: 0.0,
                end: 1.0,
                source: "プリンセスキャニオンです".to_string(),
                tokens: Vec::new(),
            },
            NormalizedSegment {
                segment_id: 2,
                start: 1.0,
                end: 2.0,
                source: "またキャニオン".to_string(),
                tokens: Vec::new(),
            },
            NormalizedSegment {
                segment_id: 3,
                start: 2.0,
                end: 3.0,
                source: "別の名前シンデレラ".to_string(),
                tokens: Vec::new(),
            },
        ];
        let mut known = HashMap::new();
        known.insert(1, "我是Canyon".to_string());
        known.insert(2, "又是Canyon".to_string());
        known.insert(3, "灰姑娘".to_string());
        let memory = derive_name_memory(&segments, &known, "今日のキャニオン");
        assert_eq!(memory.examples.len(), 1, "{:?}", memory.examples);
        assert_eq!(memory.examples[0].source, "キャニオン");
        assert_eq!(memory.examples[0].example_translation, "我是Canyon");
        assert_eq!(memory.terms.len(), 1);
        assert_eq!(memory.terms[0].source, "キャニオン");
        assert_eq!(memory.terms[0].target, "Canyon");
        assert!(!memory
            .examples
            .iter()
            .any(|e| e.source.contains("シンデレラ")));
    }

    #[test]
    fn later_marked_target_upgrades_first_common_noun_example() {
        let segments = vec![
            NormalizedSegment {
                segment_id: 1,
                start: 0.0,
                end: 1.0,
                source: "キャニオンです".to_string(),
                tokens: Vec::new(),
            },
            NormalizedSegment {
                segment_id: 2,
                start: 1.0,
                end: 2.0,
                source: "キャニオンだよ".to_string(),
                tokens: Vec::new(),
            },
        ];
        let mut known = HashMap::new();
        known.insert(1, "我是峡谷".to_string());
        known.insert(2, "我是Canyon".to_string());
        let memory = derive_name_memory(&segments, &known, "またキャニオン");
        assert_eq!(memory.examples[0].example_translation, "我是峡谷");
        assert_eq!(memory.terms.len(), 1);
        assert_eq!(memory.terms[0].target, "Canyon");
    }
}
