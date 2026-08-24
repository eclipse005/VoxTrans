use super::types::{normalize_term_key, GlossaryEntry, TranscriptCue};

const SINGLE_WINDOW_MAX_CHARS: usize = 100_000;
const WINDOW_MAX_CHARS: usize = 50_000;
const WINDOW_OVERLAP_CUES: usize = 2;
const STYLE_GUIDE_MAX_CHARS: usize = 1_200;

pub fn transcript_plain(cues: &[TranscriptCue]) -> String {
    cues.iter()
        .map(|c| format!("[{}] {}", c.index, c.text))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn transcript_haystack(cues: &[TranscriptCue]) -> String {
    cues.iter()
        .map(|c| c.text.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

fn is_cjk(c: char) -> bool {
    matches!(c,
        '\u{3040}'..='\u{30FF}'
        | '\u{3400}'..='\u{9FFF}'
        | '\u{F900}'..='\u{FAFF}'
        | '\u{FF66}'..='\u{FF9D}'
        | '\u{AC00}'..='\u{D7AF}'
    )
}

fn needs_ascii_word_boundary(source: &str) -> bool {
    let has_ascii_word = source.chars().any(|c| c.is_ascii_alphabetic());
    let has_cjk = source.chars().any(is_cjk);
    has_ascii_word && !has_cjk
}

fn is_ascii_word_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// True if `source` can fire on real subtitle text.
/// ASCII terms use word boundaries; CJK / mixed / unspaced text uses substring.
pub fn source_grounded_in_text(source: &str, text: &str) -> bool {
    let needle = source.trim();
    if needle.is_empty() || text.is_empty() {
        return false;
    }
    if !needs_ascii_word_boundary(needle) {
        return text.to_lowercase().contains(&needle.to_lowercase());
    }
    let hay = text.to_lowercase();
    let needle_l = needle.to_lowercase();
    let mut search_from = 0usize;
    while let Some(rel) = hay[search_from..].find(&needle_l) {
        let abs = search_from + rel;
        let before_ok = abs == 0
            || hay[..abs]
                .chars()
                .next_back()
                .map(|c| !is_ascii_word_char(c))
                .unwrap_or(true);
        let after = abs + needle_l.len();
        let after_ok = after >= hay.len()
            || hay[after..]
                .chars()
                .next()
                .map(|c| !is_ascii_word_char(c))
                .unwrap_or(true);
        if before_ok && after_ok {
            return true;
        }
        let next = abs.saturating_add(1);
        if next >= hay.len() {
            break;
        }
        search_from = hay.ceil_char_boundary(next);
        if search_from >= hay.len() {
            break;
        }
    }
    if needle.chars().any(char::is_whitespace) {
        let compact_hay: String = hay.chars().filter(|c| !c.is_whitespace()).collect();
        let compact_needle: String = needle_l.chars().filter(|c| !c.is_whitespace()).collect();
        if compact_needle.chars().count() >= 4 && compact_hay.contains(&compact_needle) {
            return true;
        }
    }
    false
}

fn expand_source_forms<'a>(source: &'a str, text: &str) -> Vec<&'a str> {
    let src = source.trim();
    if src.is_empty() {
        return Vec::new();
    }
    if source_grounded_in_text(src, text) {
        return vec![src];
    }
    // "Foo (Bar)" → try inner parts when the full string is not present.
    if let Some(open) = src.find('(') {
        if let Some(close) = src.rfind(')') {
            if close > open {
                let outer = src[..open].trim();
                let inner = src[open + 1..close].trim();
                let mut forms = Vec::new();
                if !outer.is_empty() && source_grounded_in_text(outer, text) {
                    forms.push(outer);
                }
                if !inner.is_empty() && source_grounded_in_text(inner, text) {
                    forms.push(inner);
                }
                return forms;
            }
        }
    }
    Vec::new()
}

pub fn ground_glossary(glossary: &[GlossaryEntry], transcript_text: &str) -> Vec<GlossaryEntry> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for g in glossary {
        let src = g.source.trim();
        let tgt = g.target.trim();
        if src.is_empty() || tgt.is_empty() {
            continue;
        }
        for form in expand_source_forms(src, transcript_text) {
            let key = normalize_term_key(form);
            if key.is_empty() || !seen.insert(key) {
                continue;
            }
            out.push(GlossaryEntry::new(form, tgt, g.note.trim()));
        }
    }
    out
}

/// User terms always kept (user-wins). Agent rows grounded; same source key
/// takes the user target.
pub fn merge_glossary_user_priority(
    user_terms: &[GlossaryEntry],
    agent_glossary: &[GlossaryEntry],
    transcript_text: &str,
) -> Vec<GlossaryEntry> {
    let grounded_agent = ground_glossary(agent_glossary, transcript_text);
    let mut user_by_key = std::collections::HashMap::new();
    for t in user_terms {
        let key = normalize_term_key(&t.source);
        if !key.is_empty() {
            user_by_key.entry(key).or_insert(t);
        }
    }

    let mut merged = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for t in user_terms {
        let src = t.source.trim();
        let tgt = t.target.trim();
        if src.is_empty() || tgt.is_empty() {
            continue;
        }
        let key = normalize_term_key(src);
        if key.is_empty() || !seen.insert(key) {
            continue;
        }
        merged.push(GlossaryEntry::new(src, tgt, t.note.trim()));
    }

    for g in grounded_agent {
        let key = normalize_term_key(&g.source);
        if key.is_empty() || seen.contains(&key) {
            continue;
        }
        seen.insert(key.clone());
        if let Some(ut) = user_by_key.get(&key) {
            let note = if ut.note.trim().is_empty() {
                "user-preferred target".to_string()
            } else {
                ut.note.trim().to_string()
            };
            merged.push(GlossaryEntry::new(
                ut.source.trim(),
                ut.target.trim(),
                note,
            ));
        } else {
            merged.push(g);
        }
    }

    merged
}

pub fn union_glossaries(parts: &[Vec<GlossaryEntry>]) -> Vec<GlossaryEntry> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for part in parts {
        for g in part {
            let src = g.source.trim();
            let tgt = g.target.trim();
            if src.is_empty() || tgt.is_empty() {
                continue;
            }
            let key = normalize_term_key(src);
            if key.is_empty() || !seen.insert(key) {
                continue;
            }
            out.push(GlossaryEntry::new(src, tgt, g.note.trim()));
        }
    }
    out
}

pub fn merge_style_guides(styles: &[String], glossary: &[GlossaryEntry], target_lang: &str) -> String {
    let cleaned: Vec<&str> = styles
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();
    if cleaned.is_empty() {
        return String::new();
    }
    let merged = if cleaned.len() == 1 {
        cleaned[0].to_string()
    } else {
        let primary = cleaned.iter().max_by_key(|s| s.len()).copied().unwrap_or("");
        let mut out = primary.to_string();
        for s in &cleaned {
            if *s != primary && !primary.contains(prefix_chars(s, 80)) {
                out.push(' ');
                out.push_str(s);
                break;
            }
        }
        out
    };
    let aligned = align_style_guide_to_glossary(&merged, glossary, target_lang);
    if aligned.chars().count() > STYLE_GUIDE_MAX_CHARS {
        aligned.chars().take(STYLE_GUIDE_MAX_CHARS).collect()
    } else {
        aligned
    }
}

fn align_style_guide_to_glossary(style: &str, glossary: &[GlossaryEntry], target_lang: &str) -> String {
    let mut out = style.to_string();
    let mut pairs: Vec<(&str, &str)> = glossary
        .iter()
        .map(|g| (g.source.as_str(), g.target.as_str()))
        .collect();
    pairs.sort_by_key(|(s, _)| std::cmp::Reverse(s.chars().count()));
    for (src, tgt) in pairs {
        if src.is_empty() || tgt.is_empty() {
            continue;
        }
        out = rewrite_quoted_claim(&out, src, tgt, target_lang);
    }
    out
}

/// Which "X is rendered as Y" verbs to recognize when rewriting quoted
/// claims, per target language. Only the target language's own convention
/// is matched; other languages skip the rewrite entirely rather than risk
/// mangling a legitimate quote with a mismatched verb table.
fn rewrite_verbs(target_lang: &str) -> &'static [&'static str] {
    let lang = target_lang.trim().to_ascii_lowercase();
    if lang.starts_with("zh") {
        &["译为", "译作", "翻译为"]
    } else if lang.starts_with("en") {
        &["translated as", "rendered as"]
    } else {
        &[]
    }
}

fn rewrite_quoted_claim(style: &str, source: &str, target: &str, target_lang: &str) -> String {
    let verbs = rewrite_verbs(target_lang);
    let mut best: Option<(usize, usize)> = None;
    for q in ['"', '\'', '“', '「'] {
        let open = format!("{q}{source}{q}");
        let Some(src_at) = find_ignore_ascii_case(style, &open) else {
            continue;
        };
        let after_src = src_at + open.len();
        let Some(rest) = style.get(after_src..) else {
            continue;
        };
        let trimmed = rest.trim_start();
        let ws = rest.len() - trimmed.len();
        for verb in verbs {
            if starts_with_ignore_ascii_case(trimmed, verb) {
                let after_verb = after_src + ws + verb.len();
                let Some(after_verb_str) = style.get(after_verb..) else {
                    continue;
                };
                let claim_area = after_verb_str.trim_start();
                let claim_ws = after_verb_str.len() - claim_area.len();
                let close_q = match q {
                    '“' => '”',
                    '「' => '」',
                    other => other,
                };
                if !claim_area.starts_with(close_q) && !claim_area.starts_with(q) {
                    continue;
                }
                let used_q = if claim_area.starts_with(close_q) {
                    close_q
                } else {
                    q
                };
                let inner = &claim_area[used_q.len_utf8()..];
                let Some(end_rel) = inner.find(used_q) else {
                    continue;
                };
                let claimed = &inner[..end_rel];
                if claimed == target {
                    continue;
                }
                let claim_start = after_verb + claim_ws + used_q.len_utf8();
                let claim_end = claim_start + end_rel;
                best = Some((claim_start, claim_end));
                break;
            }
        }
        if best.is_some() {
            break;
        }
    }
    let Some((start, end)) = best else {
        return style.to_string();
    };
    let (Some(head), Some(tail)) = (style.get(..start), style.get(end..)) else {
        return style.to_string();
    };
    let mut out = String::with_capacity(style.len() + target.len());
    out.push_str(head);
    out.push_str(target);
    out.push_str(tail);
    out
}

fn find_ignore_ascii_case(hay: &str, needle: &str) -> Option<usize> {
    hay.to_lowercase().find(&needle.to_lowercase()).and_then(|byte| {
        hay.get(byte..).map(|_| byte)
    })
}

fn starts_with_ignore_ascii_case(s: &str, prefix: &str) -> bool {
    s.get(..prefix.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
}

/// First `n` Unicode scalars; never panics on multibyte UTF-8.
fn prefix_chars(s: &str, n: usize) -> &str {
    match s.char_indices().nth(n) {
        Some((byte, _)) => &s[..byte],
        None => s,
    }
}

pub fn split_cues_windows(cues: &[TranscriptCue]) -> Vec<Vec<TranscriptCue>> {
    if cues.is_empty() {
        return vec![Vec::new()];
    }
    // One agent over the whole transcript beats splitting: no cross-window
    // term conflicts and the transcript is sent once. Only long videos chunk.
    let total: usize = cues.iter().map(|c| c.text.chars().count() + 24).sum();
    if total <= SINGLE_WINDOW_MAX_CHARS {
        return vec![cues.to_vec()];
    }
    let mut windows = Vec::new();
    let mut i = 0;
    let n = cues.len();
    while i < n {
        let mut chunk = Vec::new();
        let mut chars = 0usize;
        let mut j = i;
        while j < n {
            let add = cues[j].text.chars().count() + 24;
            if !chunk.is_empty() && chars + add > WINDOW_MAX_CHARS {
                break;
            }
            chunk.push(cues[j].clone());
            chars += add;
            j += 1;
        }
        if chunk.is_empty() {
            chunk.push(cues[i].clone());
            j = i + 1;
        }
        windows.push(chunk);
        if j >= n {
            break;
        }
        let nxt = j.saturating_sub(WINDOW_OVERLAP_CUES);
        i = (i + 1).max(nxt);
    }
    windows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_word_boundary_rejects_substring() {
        assert!(source_grounded_in_text("cat", "the cat sat"));
        assert!(!source_grounded_in_text("cat", "category theory"));
        assert!(source_grounded_in_text("NATO", "NATO summit"));
        assert!(!source_grounded_in_text("NATO", "THANATOS"));
    }

    #[test]
    fn cjk_uses_substring() {
        assert!(source_grounded_in_text("キャニオン", "ラストコール キャニオンへ"));
        assert!(source_grounded_in_text("峡谷", "走进峡谷深处"));
        assert!(!source_grounded_in_text("峡谷", "完全无关的句子"));
    }

    #[test]
    fn multi_word_compact_fallback() {
        assert!(source_grounded_in_text(
            "order block",
            "the orderblock printed here"
        ));
    }

    #[test]
    fn parenthetical_forms() {
        let text = "we use Bar in production";
        assert_eq!(expand_source_forms("Foo (Bar)", text), vec!["Bar"]);
    }

    #[test]
    fn ground_glossary_salvages_inner_form_when_full_string_absent() {
        let glossary = [GlossaryEntry::new("Foo (Bar)", "巴", "")];
        let out = ground_glossary(&glossary, "we use Bar in production");
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].source, "Bar");
        assert_eq!(out[0].target, "巴");
    }

    #[test]
    fn user_terms_always_kept_and_win() {
        let user = vec![GlossaryEntry::new("RIO", "里约", "")];
        let agent = vec![
            GlossaryEntry::new("RIO", "里奥", "agent"),
            GlossaryEntry::new("キャニオン", "峡谷", ""),
        ];
        let text = "RIO キャニオン";
        let merged = merge_glossary_user_priority(&user, &agent, text);
        assert_eq!(merged[0].source, "RIO");
        assert_eq!(merged[0].target, "里约");
        assert!(merged.iter().any(|g| g.source == "キャニオン" && g.target == "峡谷"));
    }

    #[test]
    fn ungrounded_user_term_is_still_kept() {
        let user = vec![GlossaryEntry::new("missing", "缺失", "user")];
        let merged = merge_glossary_user_priority(&user, &[], "hello world");
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].target, "缺失");
    }

    #[test]
    fn union_first_seen_wins() {
        let a = vec![GlossaryEntry::new("A", "甲", "")];
        let b = vec![GlossaryEntry::new("A", "乙", ""), GlossaryEntry::new("B", "丙", "")];
        let u = union_glossaries(&[a, b]);
        assert_eq!(u.len(), 2);
        assert_eq!(u[0].target, "甲");
        assert_eq!(u[1].source, "B");
    }

    #[test]
    fn short_transcript_is_one_window() {
        let cues: Vec<TranscriptCue> = (1..=3)
            .map(|i| TranscriptCue {
                index: i,
                start_ms: 0,
                text: "hello".into(),
            })
            .collect();
        let windows = split_cues_windows(&cues);
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].len(), 3);
    }

    #[test]
    fn mid_length_transcript_stays_single_window() {
        // ~72k chars (the forex course size) must not chunk anymore.
        let cues: Vec<TranscriptCue> = (1..=800)
            .map(|i| TranscriptCue {
                index: i,
                start_ms: 0,
                text: "a".repeat(90),
            })
            .collect();
        let windows = split_cues_windows(&cues);
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].len(), 800);
    }

    #[test]
    fn very_long_transcript_chunks_at_window_limit() {
        let cues: Vec<TranscriptCue> = (1..=3000)
            .map(|i| TranscriptCue {
                index: i,
                start_ms: 0,
                text: "a".repeat(90),
            })
            .collect();
        let windows = split_cues_windows(&cues);
        assert!(windows.len() > 1);
        let covered: usize = windows.iter().map(|w| w.len()).sum();
        assert!(covered >= 3000); // overlap means sum exceeds cue count
    }

    #[test]
    fn style_guide_rewrites_quoted_claim() {
        let glossary = vec![GlossaryEntry::new("RIO", "里约", "")];
        let style = r#"Keep "RIO"译为"里奥" throughout."#;
        let out = align_style_guide_to_glossary(style, &glossary, "zh");
        assert!(out.contains("里约"));
        assert!(!out.contains("里奥"));
    }

    #[test]
    fn style_guide_rewrite_follows_target_language() {
        let glossary = vec![GlossaryEntry::new("RIO", "里约", "")];
        let english_style = r#"Keep "RIO" translated as "里奥" throughout."#;
        // English target: English verbs are recognized and rewritten.
        let en_out = align_style_guide_to_glossary(english_style, &glossary, "en");
        assert!(en_out.contains("里约"), "en_out={en_out}");
        assert!(!en_out.contains("里奥"), "en_out={en_out}");
        // Unknown/other target languages skip the rewrite entirely.
        let ja_out = align_style_guide_to_glossary(english_style, &glossary, "ja");
        assert_eq!(ja_out, english_style);
        // Chinese target does not rewrite English-verb claims.
        let zh_out = align_style_guide_to_glossary(english_style, &glossary, "zh");
        assert_eq!(zh_out, english_style);
    }

    #[test]
    fn merge_style_guides_does_not_panic_when_byte_80_is_inside_cjk() {
        // '。' is 3 bytes. An 78-byte ASCII prefix + '。' makes byte 80 sit
        // inside the ideographic full stop — the old `s[..80]` panicked here.
        let primary = "x".repeat(200);
        let mut other = "a".repeat(78);
        other.push('。');
        other.push_str("另一段完全不同的风格说明，用于触发拼接。");
        let out = merge_style_guides(&[primary.clone(), other.clone()], &[], "zh");
        assert!(out.contains(&primary));
        assert!(out.contains("另一段完全不同"));
    }
}
