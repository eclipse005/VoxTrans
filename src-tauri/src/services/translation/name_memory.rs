//! Cross-batch name memory derived from already-committed translations.
//!
//! No extra LLM call: after each batch we already have `source → translation`
//! pairs. Recurring marked-script spans (katakana, or Latin inside CJK text)
//! are replayed into later prompts so the translator can keep using the same
//! rendering. User-supplied terminology still wins on conflict.

use std::collections::{HashMap, HashSet};

use crate::services::prompts::translation::{TranslationNameExample, TranslationPromptTerm};

use super::batches::normalize_for_match;
use super::types::NormalizedSegment;

const MAX_NAME_EXAMPLES: usize = 24;

#[derive(Debug, Clone)]
pub(super) struct NameMemory {
    pub terms: Vec<TranslationPromptTerm>,
    pub examples: Vec<TranslationNameExample>,
}

/// Build name memory from committed translations, keeping only names that
/// appear in `current_batch_text`. First-seen example wins (segment id order).
///
/// A "rendering" that keeps the source's distinctive script (e.g. a katakana
/// name left as katakana in a zh-CN translation) is not a translation at all
/// — it is skipped here, never becoming an example or a locked term. Without
/// this, one lazy passthrough gets locked by `bind_marked_target` and teaches
/// every later batch to leave the name untranslated.
pub(super) fn derive_name_memory(
    segments: &[NormalizedSegment],
    known_translations: &HashMap<usize, String>,
    current_batch_text: &str,
    source_lang: &str,
    target_lang: &str,
) -> NameMemory {
    let leak = super::guard::leak_script(source_lang, target_lang);
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
            // Passthrough of a source-script span (kana name left as kana in
            // a zh output, …) is an untranslated name: do not propagate it.
            // Rescue: when the source spelling itself carries the Latin
            // rendering right next to the span (「バロン（Baron）」 /
            // 「Baron（バロン）」) and the translation uses that Latin form,
            // the kana is an annotation, not a lazy passthrough.
            let companion_latin = adjacent_latin_in_source(&segment.source, &span);
            if let Some(script) = leak {
                if super::guard::contains_script(span.as_str(), script)
                    && translation.contains(span.as_str())
                    && !latin_rendered_in_translation(companion_latin.as_deref(), translation)
                {
                    continue;
                }
            }
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
                    if let Some(target) =
                        bind_marked_target(&form, translation, companion_latin.as_deref())
                            .or_else(|| {
                                bind_marked_target(&span, translation, companion_latin.as_deref())
                            })
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
                if let Some(target) =
                    bind_marked_target(&form, translation, companion_latin.as_deref())
                        .or_else(|| {
                            bind_marked_target(&span, translation, companion_latin.as_deref())
                        })
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

/// Latin rendering spelled out right next to `span` in `source`
/// (「バロン（Baron）」, 「Baron（バロン）」). Separators are whitespace and
/// bracket-ish glyphs; the Latin run must be ≥2 letters to count as a name.
fn adjacent_latin_in_source(source: &str, span: &str) -> Option<String> {
    let pos = source.find(span)?;
    let after: String = source[pos + span.len()..]
        .chars()
        .skip_while(|c| is_latin_annotation_sep(*c))
        .take_while(|c| is_latin_letter(*c))
        .collect();
    if after.chars().count() >= 2 {
        return Some(after);
    }
    let before: String = source[..pos]
        .chars()
        .rev()
        .skip_while(|c| is_latin_annotation_sep(*c))
        .take_while(|c| is_latin_letter(*c))
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    if before.chars().count() >= 2 {
        return Some(before);
    }
    None
}

fn is_latin_annotation_sep(c: char) -> bool {
    c.is_whitespace() || matches!(c, '(' | ')' | '（' | '）' | '[' | ']' | '【' | '】' | '・')
}

/// True when the source-declared companion Latin form (「バロン（Baron）」's
/// "Baron") appears in the translation, case-insensitively. Any substring
/// counts here — the rescue is deliberately lenient; the bind step below is
/// the strict gate that decides what actually gets locked.
fn latin_rendered_in_translation(companion_latin: Option<&str>, translation: &str) -> bool {
    companion_latin
        .map(|latin| translation.to_lowercase().contains(&latin.to_lowercase()))
        .unwrap_or(false)
}

/// Lock a target only when it stays in a marked script (passthrough or
/// Latin/kana transcription). Semantic common-noun renderings are not locked.
///
/// `companion_latin` is the Latin form the source declares right next to the
/// name (「バロン（Baron）」). When the translation uses that declared form
/// verbatim, it is the lock — however many *other* Latin runs the translation
/// carries, because the source's own declaration disambiguates them. A run
/// that merely *contains* it (Baroness vs Baron) is not the declared
/// rendering and must not be locked — that would teach every later batch a
/// name the source never spelled out.
fn bind_marked_target(
    name: &str,
    translation: &str,
    companion_latin: Option<&str>,
) -> Option<String> {
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
    // The declared companion wins first: in 「Baron（バロン）击败了 BOSS」 the
    // lone-Latin heuristics below cannot tell "Baron" from "BOSS", but the
    // source already did — without this branch the kana fallback would lock
    // the passthrough annotation instead of the declared rendering.
    if let Some(declared) = companion_latin
        && let Some(run) = trans_latin.iter().find(|t| t.eq_ignore_ascii_case(declared))
    {
        return Some(run.clone());
    }
    // Kana kept in the translation as an annotation next to the one Latin
    // rendering (「Baron（バロン）」): the Latin name is the marked target,
    // not the kana.
    if name_kana && trans_latin.len() == 1 && translation.contains(name) {
        return lock_declared_latin_run(&trans_latin[0], companion_latin);
    }
    if translation.contains(name) {
        return Some(name.to_string());
    }
    if name_kana && trans_latin.len() == 1 && trans_kana.is_empty() {
        return lock_declared_latin_run(&trans_latin[0], companion_latin);
    }
    if name_latin && trans_latin.iter().any(|t| t.eq_ignore_ascii_case(name)) {
        return Some(name.to_string());
    }
    None
}

/// A lone Latin run is lockable only when it equals the Latin form the
/// source declared next to the name (case-insensitively); without a declared
/// companion the run stands on its own.
fn lock_declared_latin_run(run: &str, companion_latin: Option<&str>) -> Option<String> {
    if let Some(declared) = companion_latin {
        return run.eq_ignore_ascii_case(declared).then(|| run.to_string());
    }
    Some(run.to_string())
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
            bind_marked_target("キャニオン", "我是Canyon", None),
            Some("Canyon".to_string())
        );
        assert_eq!(
            bind_marked_target("キャニオン", "我是キャニオン", None),
            Some("キャニオン".to_string())
        );
        assert_eq!(bind_marked_target("キャニオン", "我是世界第一可爱的公主峡谷", None), None);
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
        let memory = derive_name_memory(&segments, &known, "今日のキャニオン", "ja", "zh-CN");
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
        let memory = derive_name_memory(&segments, &known, "またキャニオン", "ja", "zh-CN");
        assert_eq!(memory.examples[0].example_translation, "我是峡谷");
        assert_eq!(memory.terms.len(), 1);
        assert_eq!(memory.terms[0].target, "Canyon");
    }

    #[test]
    fn source_script_passthrough_is_not_propagated() {
        let segments = vec![NormalizedSegment {
            segment_id: 1,
            start: 0.0,
            end: 1.0,
            source: "キャニオンです".to_string(),
            tokens: Vec::new(),
        }];
        let mut known = HashMap::new();
        // Lazy passthrough: the name stayed kana in a zh-CN translation.
        known.insert(1, "这就是キャニオン的视频".to_string());
        // ja→zh: untranslated kana must not become an example or a lock.
        let memory = derive_name_memory(&segments, &known, "またキャニオン", "ja", "zh-CN");
        assert!(memory.examples.is_empty(), "{:?}", memory.examples);
        assert!(memory.terms.is_empty(), "{:?}", memory.terms);
        // Same-script direction has no leak relation: passthrough still works.
        let memory = derive_name_memory(&segments, &known, "またキャニオン", "ja", "ja");
        assert_eq!(memory.examples.len(), 1);
        assert_eq!(memory.terms.len(), 1);
        assert_eq!(memory.terms[0].target, "キャニオン");
    }

    #[test]
    fn latin_passthrough_is_still_locked() {
        // Latin names legitimately stay Latin in a zh-CN output; only the
        // source's distinctive script is filtered.
        let segments = vec![NormalizedSegment {
            segment_id: 1,
            start: 0.0,
            end: 1.0,
            source: "ROLANDが登場".to_string(),
            tokens: Vec::new(),
        }];
        let mut known = HashMap::new();
        known.insert(1, "ROLAND登场了".to_string());
        let memory = derive_name_memory(&segments, &known, "またROLAND", "ja", "zh-CN");
        assert_eq!(memory.terms.len(), 1);
        assert_eq!(memory.terms[0].target, "ROLAND");
    }

    #[test]
    fn latin_annotation_in_translation_keeps_kana_name() {
        // 「バロン（Baron）」 in the translation: the Latin rendering is the
        // marked target; the kana is an annotation, not a lazy passthrough.
        // The kana span must still become an example, locked to "Baron".
        let segments = vec![NormalizedSegment {
            segment_id: 1,
            start: 0.0,
            end: 1.0,
            source: "バロン（Baron）です".to_string(),
            tokens: Vec::new(),
        }];
        let mut known = HashMap::new();
        known.insert(1, "Baron（バロン）的视频".to_string());
        let memory = derive_name_memory(&segments, &known, "またバロン", "ja", "zh-CN");
        assert_eq!(memory.examples.len(), 1, "{:?}", memory.examples);
        assert_eq!(memory.examples[0].source, "バロン");
        assert_eq!(memory.terms.len(), 1, "{:?}", memory.terms);
        assert_eq!(memory.terms[0].source, "バロン");
        assert_eq!(memory.terms[0].target, "Baron");
    }

    #[test]
    fn kana_kept_without_latin_companion_is_still_dropped() {
        // The translation drops the Latin rendering from the annotation: the
        // kana really is an untranslated passthrough and must not propagate.
        let segments = vec![NormalizedSegment {
            segment_id: 1,
            start: 0.0,
            end: 1.0,
            source: "バロン（Baron）です".to_string(),
            tokens: Vec::new(),
        }];
        let mut known = HashMap::new();
        known.insert(1, "バロンの视频".to_string());
        let memory = derive_name_memory(&segments, &known, "またバロン", "ja", "zh-CN");
        assert!(memory.examples.is_empty(), "{:?}", memory.examples);
        assert!(memory.terms.is_empty(), "{:?}", memory.terms);
    }

    #[test]
    fn latin_run_longer_than_declared_form_is_not_locked() {
        // 「バロン（Baron）」 declares "Baron": a Latin run in the translation
        // that merely *contains* it ("Baroness") passes the lenient rescue
        // but must not be locked. Locking it would propagate a rendering the
        // source never spelled out to every later batch.
        let cases: [(&str, &str); 2] = [
            ("Baroness（バロン）的视频", "Baroness"),
            ("Baroness 的视频", "Baroness"),
        ];
        for (translation, run) in cases {
            let segments = vec![NormalizedSegment {
                segment_id: 1,
                start: 0.0,
                end: 1.0,
                source: "バロン（Baron）です".to_string(),
                tokens: Vec::new(),
            }];
            let mut known = HashMap::new();
            known.insert(1, translation.to_string());
            let memory = derive_name_memory(&segments, &known, "またバロン", "ja", "zh-CN");
            assert!(
                !memory.terms.iter().any(|t| t.target == run),
                "{:?}: locked {}",
                memory.terms,
                run
            );
        }
        // The correct path still locks the declared form.
        let segments = vec![NormalizedSegment {
            segment_id: 1,
            start: 0.0,
            end: 1.0,
            source: "バロン（Baron）です".to_string(),
            tokens: Vec::new(),
        }];
        let mut known = HashMap::new();
        known.insert(1, "Baron（バロン）的视频".to_string());
        let memory = derive_name_memory(&segments, &known, "またバロン", "ja", "zh-CN");
        assert_eq!(memory.terms.len(), 1, "{:?}", memory.terms);
        assert_eq!(memory.terms[0].target, "Baron");
    }

    #[test]
    fn declared_latin_wins_over_unrelated_latin_runs() {
        // 「Baron（バロン）击败了 BOSS」: the kana annotation sits next to the
        // declared rendering, but a second unrelated Latin run breaks the
        // lone-Latin heuristic. The declared form must still be the lock —
        // never the kana passthrough.
        assert_eq!(
            bind_marked_target("バロン", "Baron（バロン）击败BOSS", Some("Baron")),
            Some("Baron".to_string())
        );
        let segments = vec![NormalizedSegment {
            segment_id: 1,
            start: 0.0,
            end: 1.0,
            source: "バロン（Baron）です".to_string(),
            tokens: Vec::new(),
        }];
        let mut known = HashMap::new();
        known.insert(1, "Baron（バロン）击败BOSS".to_string());
        let memory = derive_name_memory(&segments, &known, "またバロン", "ja", "zh-CN");
        assert_eq!(memory.terms.len(), 1, "{:?}", memory.terms);
        assert_eq!(memory.terms[0].target, "Baron");
    }
}
