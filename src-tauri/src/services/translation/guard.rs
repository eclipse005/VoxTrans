//! Post-parse guards on a translation batch. Language-agnostic: leak uses
//! the source language's distinctive script.

use super::batches::normalize_for_match;

pub(super) fn language_leak_ids(
    translations: &[(usize, &str)],
    source_lang: &str,
    target_lang: &str,
    enforced_targets: &[String],
) -> Vec<usize> {
    let Some(leak) = leak_script(source_lang, target_lang) else {
        return Vec::new();
    };
    let target = target_script(target_lang);
    // Normalize once and strip longest targets first: a shorter target that is
    // a prefix of a longer one (「ラストコール」 vs 「ラストコールショータイム」)
    // would otherwise consume the shared head and leave the tail counted as
    // leakage.
    let mut normalized_targets: Vec<String> = enforced_targets
        .iter()
        .map(|target| normalize_for_match(target))
        .filter(|normalized| !normalized.is_empty())
        .collect();
    normalized_targets.sort_by_key(|target| std::cmp::Reverse(target.chars().count()));
    translations
        .iter()
        .filter(|(_, text)| {
            let scrubbed = strip_enforced_targets(text, &normalized_targets);
            line_leaks_source_script(&scrubbed, leak, target)
        })
        .map(|(id, _)| *id)
        .collect()
}

/// Enforced terminology targets are mandated verbatim in the output; any
/// source-script characters they contain (e.g. a kana annotation inside a
/// zh-CN target) are not translator leakage. Strip their occurrences before
/// counting scripts, or a compliant translation gets rejected.
///
/// The strip happens entirely on a normalized copy (case/spacing/fullwidth
/// folded), so a configured "Last Call (ラストコール)" also covers the
/// model's "LastCall（ラストコール）". The raw translation is never modified;
/// script counting is unaffected by the folding.
fn strip_enforced_targets(text: &str, normalized_targets: &[String]) -> String {
    let mut out = normalize_for_match(text);
    for target in normalized_targets {
        out = out.replace(target.as_str(), "");
    }
    out
}

#[derive(Clone, Copy)]
pub(super) enum Script {
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

pub(super) fn leak_script(source_lang: &str, target_lang: &str) -> Option<Script> {
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

/// Does `text` contain any character of `script`? Shared with name memory:
/// a "rendering" that stays in the source's distinctive script is not a
/// translation and must not be propagated.
pub(super) fn contains_script(text: &str, script: Script) -> bool {
    count_script(text, script) > 0
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

fn is_hangul(ch: char) -> bool {
    matches!(ch as u32, 0x1100..=0x11FF | 0x3130..=0x318F | 0xAC00..=0xD7AF)
}

fn is_han(ch: char) -> bool {
    matches!(ch as u32, 0x3400..=0x4DBF | 0x4E00..=0x9FFF)
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
            &[],
        );
        assert_eq!(ids, vec![1]);
    }

    #[test]
    fn allows_chinese_with_a_katakana_name() {
        let ids = language_leak_ids(
            &[(1, "我也绝对不会坐那种看起来像ホスト的人的位置")],
            "ja",
            "zh-CN",
            &[],
        );
        assert!(ids.is_empty(), "{ids:?}");
    }

    #[test]
    fn exempts_enforced_target_with_source_script() {
        // The term target is mandated verbatim; its kana is not leakage even
        // when the line repeats the term and kana would otherwise dominate.
        let ids = language_leak_ids(
            &[(1, "只能去Last Call（ラストコール）了啊，去Last Call（ラストコール）吧")],
            "ja",
            "zh-CN",
            &["Last Call（ラストコール）".to_string()],
        );
        assert!(ids.is_empty(), "{ids:?}");
        // Same line WITHOUT the exemption is still caught.
        let ids = language_leak_ids(
            &[(1, "只能去Last Call（ラストコール）了啊，去Last Call（ラストコール）吧")],
            "ja",
            "zh-CN",
            &[],
        );
        assert_eq!(ids, vec![1]);
    }

    #[test]
    fn exemption_does_not_hide_real_leakage() {
        // Stripping the enforced target must not launder an actual untranslated
        // sentence that merely quotes the term once.
        let ids = language_leak_ids(
            &[(
                1,
                "Last Call（ラストコール）そして彼女は黙って立ち去ったのだった",
            )],
            "ja",
            "zh-CN",
            &["Last Call（ラストコール）".to_string()],
        );
        assert_eq!(ids, vec![1]);
    }

    #[test]
    fn skips_language_check_without_langs() {
        let ids =
            language_leak_ids(&[(1, "過去にはいろいろな悩みを抱えてきました")], "", "", &[]);
        assert!(ids.is_empty());
    }

    #[test]
    fn exempts_enforced_target_fullwidth_and_spacing_variants() {
        // The configured term uses halfwidth ASCII with a space while the
        // model rendered it fullwidth without a space. The exemption must
        // still apply — folding is part of the strip, not a literal match.
        let ids = language_leak_ids(
            &[(1, "只能去LastCall（ラストコール）了啊，去LastCall（ラストコール）吧")],
            "ja",
            "zh-CN",
            &["Last Call (ラストコール)".to_string()],
        );
        assert!(ids.is_empty(), "{ids:?}");
        // One extra kana line not covered by the term is still caught.
        let ids = language_leak_ids(
            &[
                (1, "只能去LastCall（ラストコール）了啊"),
                (2, "ラストコールショータイムが始まるよ"),
            ],
            "ja",
            "zh-CN",
            &["Last Call (ラストコール)".to_string()],
        );
        assert_eq!(ids, vec![2]);
    }

    #[test]
    fn overlapping_targets_strip_longest_first() {
        // 「ラストコールショータイム」 contains the shorter 「ラストコール」 as a
        // prefix. Stripping the short one first would leave ショータイム×2
        // counted as leakage; longest-first removes both terms entirely.
        let ids = language_leak_ids(
            &[(1, "ラストコールショータイムだよ、ラストコールショータイムだ")],
            "ja",
            "zh-CN",
            &[
                "ラストコール".to_string(),
                "ラストコールショータイム".to_string(),
            ],
        );
        assert!(ids.is_empty(), "{ids:?}");
        // Without the longer term, the remaining ショータイム runs leak.
        let ids = language_leak_ids(
            &[(1, "ラストコールショータイムだよ、ラストコールショータイムだ")],
            "ja",
            "zh-CN",
            &["ラストコール".to_string()],
        );
        assert_eq!(ids, vec![1]);
    }
}
