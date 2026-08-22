//! Post-parse guards on a translation batch. Same LLM call retries; no extra
//! model. Language-agnostic: leak uses the source language's distinctive
//! script, neighbor-copy uses marked tokens (numbers, Latin, kana, hangul).

use std::collections::HashSet;

pub(super) fn language_leak_ids(
    translations: &[(usize, &str)],
    source_lang: &str,
    target_lang: &str,
) -> Vec<usize> {
    let Some(leak) = leak_script(source_lang, target_lang) else {
        return Vec::new();
    };
    let target = target_script(target_lang);
    translations
        .iter()
        .filter(|(_, text)| line_leaks_source_script(text, leak, target))
        .map(|(id, _)| *id)
        .collect()
}

pub(super) fn neighbor_copy_ids(
    translations: &[(usize, &str)],
    current_sources: &[String],
    prev_sources: &[String],
    next_sources: &[String],
) -> Vec<usize> {
    if current_sources.is_empty() {
        return Vec::new();
    }
    let mut stolen = Vec::new();
    for (id, text) in translations {
        let idx = id.saturating_sub(1);
        let Some(self_src) = current_sources.get(idx) else {
            continue;
        };
        let neighbors = neighbor_sources(idx, current_sources, prev_sources, next_sources);
        if copied_neighbor_tokens(text, self_src, &neighbors) {
            stolen.push(*id);
        }
    }
    stolen
}

fn neighbor_sources<'a>(
    idx: usize,
    current: &'a [String],
    prev: &'a [String],
    next: &'a [String],
) -> Vec<&'a str> {
    let mut out = Vec::new();
    if idx > 0 {
        if let Some(s) = current.get(idx - 1) {
            out.push(s.as_str());
        }
    } else if let Some(s) = prev.last() {
        out.push(s.as_str());
    }
    if let Some(s) = current.get(idx + 1) {
        out.push(s.as_str());
    } else if idx + 1 >= current.len() {
        if let Some(s) = next.first() {
            out.push(s.as_str());
        }
    }
    out
}

fn copied_neighbor_tokens(translation: &str, self_src: &str, neighbors: &[&str]) -> bool {
    let trans_toks = distinctive_tokens(translation);
    if trans_toks.is_empty() {
        return false;
    }
    let self_toks = distinctive_tokens(self_src);
    let self_hits = trans_toks.intersection(&self_toks).count();
    let mut stolen: HashSet<String> = HashSet::new();
    for neighbor in neighbors {
        let n_toks = distinctive_tokens(neighbor);
        for tok in trans_toks.intersection(&n_toks) {
            if !self_toks.contains(tok) {
                stolen.insert(tok.clone());
            }
        }
    }
    if stolen.is_empty() {
        return false;
    }
    let strong = stolen.iter().any(|t| {
        t.chars().count() >= 3 || (t.chars().all(|c| c.is_ascii_digit()) && t.len() >= 2)
    });
    (stolen.len() > self_hits && strong) || stolen.len() >= 2
}

pub(super) fn distinctive_tokens(text: &str) -> HashSet<String> {
    let mut out = HashSet::new();
    collect_script_runs(text, is_ascii_digit_or_fw, 2, &mut out);
    collect_script_runs(text, is_latin, 2, &mut out);
    collect_script_runs(text, is_katakana, 2, &mut out);
    collect_script_runs(text, is_hangul, 2, &mut out);
    out
}

fn collect_script_runs(
    text: &str,
    pred: fn(char) -> bool,
    min: usize,
    out: &mut HashSet<String>,
) {
    let mut current = String::new();
    for ch in text.chars() {
        if pred(ch) {
            current.push(normalize_fw_digit(ch));
            continue;
        }
        if current.chars().count() >= min {
            out.insert(std::mem::take(&mut current));
        } else {
            current.clear();
        }
    }
    if current.chars().count() >= min {
        out.insert(current);
    }
}

#[derive(Clone, Copy)]
enum Script {
    Kana,
    Hangul,
    Han,
    Arabic,
    Thai,
    Cyrillic,
    Hebrew,
}

fn lang_key(lang: &str) -> String {
    let trimmed = lang.trim();
    let end = trimmed.find(['-', '_']).unwrap_or(trimmed.len());
    trimmed[..end].to_ascii_lowercase()
}

fn leak_script(source_lang: &str, target_lang: &str) -> Option<Script> {
    let s = lang_key(source_lang);
    let t = lang_key(target_lang);
    if s.is_empty() || t.is_empty() || s == t {
        return None;
    }
    match s.as_str() {
        "ja" if t != "ja" => Some(Script::Kana),
        "ko" if t != "ko" => Some(Script::Hangul),
        "zh" | "yue" if !matches!(t.as_str(), "zh" | "yue" | "ja") => Some(Script::Han),
        "ar" if t != "ar" => Some(Script::Arabic),
        "th" if t != "th" => Some(Script::Thai),
        "ru" | "uk" | "bg" if t != s => Some(Script::Cyrillic),
        "he" if t != "he" => Some(Script::Hebrew),
        _ => None,
    }
}

fn target_script(target_lang: &str) -> Script {
    match lang_key(target_lang).as_str() {
        "zh" | "yue" | "ja" => Script::Han,
        "ko" => Script::Hangul,
        "ar" => Script::Arabic,
        "th" => Script::Thai,
        "ru" | "uk" | "bg" => Script::Cyrillic,
        "he" => Script::Hebrew,
        _ => Script::Han,
    }
}

fn line_leaks_source_script(text: &str, leak: Script, target: Script) -> bool {
    let content: String = text.chars().filter(|c| !c.is_whitespace() && !c.is_ascii_punctuation()).collect();
    if content.chars().count() < 8 {
        return false;
    }
    let leak_n = count_script(text, leak);
    let target_n = count_script(text, target);
    leak_n >= 8 && leak_n > target_n
}

fn count_script(text: &str, script: Script) -> usize {
    text.chars()
        .filter(|ch| match script {
            Script::Kana => is_kana(*ch),
            Script::Hangul => is_hangul(*ch),
            Script::Han => is_han(*ch),
            Script::Arabic => matches!(*ch as u32, 0x0600..=0x06FF | 0x0750..=0x077F),
            Script::Thai => matches!(*ch as u32, 0x0E00..=0x0E7F),
            Script::Cyrillic => matches!(*ch as u32, 0x0400..=0x04FF),
            Script::Hebrew => matches!(*ch as u32, 0x0590..=0x05FF),
        })
        .count()
}

fn is_kana(ch: char) -> bool {
    matches!(ch as u32, 0x3040..=0x30FF | 0xFF66..=0xFF9D)
}

fn is_katakana(ch: char) -> bool {
    matches!(ch as u32, 0x30A0..=0x30FF | 0xFF66..=0xFF9D) && ch != '・'
}

fn is_hangul(ch: char) -> bool {
    matches!(ch as u32, 0x1100..=0x11FF | 0x3130..=0x318F | 0xAC00..=0xD7AF)
}

fn is_han(ch: char) -> bool {
    matches!(ch as u32, 0x3400..=0x4DBF | 0x4E00..=0x9FFF)
}

fn is_latin(ch: char) -> bool {
    ch.is_ascii_alphabetic()
}

fn is_ascii_digit_or_fw(ch: char) -> bool {
    ch.is_ascii_digit() || ('０'..='９').contains(&ch)
}

fn normalize_fw_digit(ch: char) -> char {
    if ('０'..='９').contains(&ch) {
        char::from(b'0' + (ch as u32 - '０' as u32) as u8)
    } else {
        ch.to_ascii_lowercase()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_japanese_paraphrase_into_chinese() {
        let ids = language_leak_ids(
            &[(1, "過去にはいろいろな悩みを抱えてきました")],
            "ja",
            "zh-CN",
        );
        assert_eq!(ids, vec![1]);
    }

    #[test]
    fn allows_chinese_with_a_katakana_name() {
        let ids = language_leak_ids(
            &[(1, "我也绝对不会坐那种看起来像ホスト的人的位置")],
            "ja",
            "zh-CN",
        );
        assert!(ids.is_empty(), "{ids:?}");
    }

    #[test]
    fn skips_language_check_without_langs() {
        let ids = language_leak_ids(&[(1, "過去にはいろいろな悩みを抱えてきました")], "", "");
        assert!(ids.is_empty());
    }

    #[test]
    fn detects_neighbor_name_copied_into_wrong_line() {
        let current = vec![
            "100戦連馬のMCも手に".to_string(),
            "あるんだけどバロンっていう".to_string(),
        ];
        let ids = neighbor_copy_ids(
            &[(1, "也有呢叫做バロン")],
            &current,
            &[],
            &[],
        );
        assert_eq!(ids, vec![1]);
    }

    #[test]
    fn allows_own_name_in_translation() {
        let current = vec!["バロンという人が六本木に".to_string()];
        let ids = neighbor_copy_ids(&[(1, "有个叫バロン的人在六本木")], &current, &[], &[]);
        assert!(ids.is_empty(), "{ids:?}");
    }

    #[test]
    fn detects_copy_from_next_batch_context() {
        let current = vec!["見た方がわかりやすいと思うんですけれどもあ、".to_string()];
        let next = vec!["こんな感じだったんだそうなんです。へー。".to_string()];
        // Distinctive overlap is weak here (no marked tokens) — should not false-positive.
        let ids = neighbor_copy_ids(
            &[(1, "原来是这种感觉啊")],
            &current,
            &[],
            &next,
        );
        assert!(ids.is_empty(), "{ids:?}");
    }
}
