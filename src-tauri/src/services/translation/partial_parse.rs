//! Best-effort extraction of translation id→text pairs from incomplete
//! model output while an SSE stream is still growing.
//!
//! # Design (UTF-8 safety by construction)
//!
//! Partial model output is a valid UTF-8 `&str`. **We never invent byte
//! offsets with arithmetic** (e.g. `i + 500`) for slicing — that is what
//! panicked on Chinese characters, which are multi-byte in UTF-8.
//!
//! All slice endpoints come from:
//! - `str::find` / `find` on ASCII keys (`"id"`, `"text"`, …) — always char boundaries
//! - `char_indices` / `chars` — always char boundaries
//! - `len()` of a `&str` — always a char boundary
//!
//! Never fails: returns whatever complete or trailing-open `text` fields
//! can be recovered. Strict validation stays at batch completion.

use std::collections::HashMap;

/// Extract partial translations from raw (possibly incomplete) model text.
///
/// Supports the primary schema:
/// ```json
/// { "translations": [ { "id": 1, "text": "..." }, ... ] }
/// ```
/// and the map form `{ "1": { "text": "..." } }` when entries are present.
pub(super) fn extract_partial_translations(raw: &str) -> HashMap<usize, String> {
    let mut out = HashMap::new();
    if raw.trim().is_empty() {
        return out;
    }

    // Prefer scanning inside a translations array if present.
    if let Some(array_body) = find_translations_array_body(raw) {
        scan_id_text_objects(array_body, &mut out);
    } else {
        scan_id_text_objects(raw, &mut out);
    }

    // Also pick up map-form entries: "12":{"text":"..."}
    scan_map_form_entries(raw, &mut out);

    out
}

fn find_translations_array_body(raw: &str) -> Option<&str> {
    // ASCII key — find() returns a char boundary.
    // Fast path: the key is emitted in exact lowercase in the common case,
    // so try an exact match first and only pay for a lowercased copy of the
    // whole buffer when a case-variant key is actually present.
    let key = "\"translations\"";
    let idx = match raw.find(key) {
        Some(idx) => idx,
        None => raw.to_ascii_lowercase().find(key)?,
    };
    let after_key = &raw[idx + key.len()..];
    let bracket = after_key.find('[')?;
    let start = idx + key.len() + bracket + 1;
    // start is derived only from ASCII finds + fixed ASCII lengths → char boundary.
    debug_assert!(raw.is_char_boundary(start));
    Some(&raw[start..])
}

/// Walk every `"id": N` and take the following text field until the next `"id"`
/// (or end of region). No fixed-size byte windows.
fn scan_id_text_objects(region: &str, out: &mut HashMap<usize, String>) {
    let mut search_from = 0;
    while search_from < region.len() {
        debug_assert!(region.is_char_boundary(search_from));
        let Some(rel) = region[search_from..].find("\"id\"") else {
            break;
        };
        let id_key_at = search_from + rel;
        let after_key = id_key_at + "\"id\"".len();
        debug_assert!(region.is_char_boundary(after_key));

        let Some((id, after_id_num)) = parse_json_number_after_colon(&region[after_key..]) else {
            // Skip this key and keep searching.
            search_from = after_key;
            continue;
        };

        // Absolute index just past the id number digits.
        let after_id_abs = after_key + after_id_num;
        debug_assert!(region.is_char_boundary(after_id_abs));

        // Object span for this id: until the next "id" key or end of region.
        // Both endpoints are char boundaries (find ASCII / len).
        let next_id_rel = region[after_id_abs..].find("\"id\"");
        let span_end = next_id_rel
            .map(|r| after_id_abs + r)
            .unwrap_or(region.len());
        debug_assert!(region.is_char_boundary(span_end));
        let span = &region[after_id_abs..span_end];

        if let Some(text) = extract_text_field_after(span) {
            insert_prefer_longer(out, id, text);
        }

        search_from = after_id_abs;
    }
}

/// After a key, parse `: <digits>` and return (value, byte length consumed from `s`).
fn parse_json_number_after_colon(s: &str) -> Option<(usize, usize)> {
    let colon = s.find(':')?;
    let after_colon = &s[colon + 1..];
    let trimmed = after_colon.trim_start();
    let trim_prefix = after_colon.len() - trimmed.len();
    let digits: String = trimmed.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    let id = digits.parse().ok()?;
    // colon + 1 + whitespace + digits, all ASCII → char-safe length
    let consumed = colon + 1 + trim_prefix + digits.len();
    Some((id, consumed))
}

fn extract_text_field_after(window: &str) -> Option<String> {
    for key in ["\"text\"", "\"translation\"", "\"translatedText\""] {
        if let Some(pos) = window.find(key) {
            let after = &window[pos + key.len()..];
            let colon = after.find(':')?;
            let mut rest = after[colon + 1..].trim_start();
            if !rest.starts_with('"') {
                continue;
            }
            rest = &rest[1..];
            return Some(unescaped_string_prefix(rest));
        }
    }
    None
}

/// Decode a JSON string body until an unescaped closing quote, or to end if open.
/// Iterates by `char`, so multi-byte UTF-8 is handled safely.
fn unescaped_string_prefix(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('t') => out.push('\t'),
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some('u') => {
                    let mut hex = String::new();
                    for _ in 0..4 {
                        if let Some(h) = chars.next() {
                            hex.push(h);
                        }
                    }
                    if let Ok(code) = u16::from_str_radix(&hex, 16) {
                        if let Some(ch) = char::from_u32(code as u32) {
                            out.push(ch);
                        }
                    }
                }
                Some(other) => out.push(other),
                None => break,
            }
        } else if c == '"' {
            break;
        } else {
            out.push(c);
        }
    }
    out
}

/// Map form: `"12": { "text": "..." }` — walk by `char_indices` only.
fn scan_map_form_entries(raw: &str, out: &mut HashMap<usize, String>) {
    let mut i = 0;
    while i < raw.len() {
        debug_assert!(raw.is_char_boundary(i));
        let Some(rel) = raw[i..].find('"') else {
            break;
        };
        let quote_at = i + rel;
        let after_quote = quote_at + 1;
        if after_quote > raw.len() || !raw.is_char_boundary(after_quote) {
            break;
        }
        let rest = &raw[after_quote..];
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if digits.is_empty() {
            i = after_quote;
            continue;
        }
        let after_digits = after_quote + digits.len(); // digits are ASCII
        if raw.as_bytes().get(after_digits) != Some(&b'"') {
            i = after_quote;
            continue;
        }
        let Ok(id) = digits.parse::<usize>() else {
            i = after_quote;
            continue;
        };
        let after_key = after_digits + 1;
        let after_key_str = &raw[after_key..];
        let Some(brace_rel) = after_key_str.find('{') else {
            i = after_key;
            continue;
        };
        let obj_start = after_key + brace_rel;
        // Span until next top-level-ish `"\d+"` key or end — use next `"` + digits pattern
        // simply: until next `"id"` is wrong for map form; take to end of string is fine
        // for extract_text_field_after (it stops at closed string).
        // Bound the object by the next `"\d+"` map key if present.
        let obj_region = &raw[obj_start..];
        let span_end = find_next_numeric_map_key_offset(obj_region)
            .map(|off| obj_start + off)
            .unwrap_or(raw.len());
        debug_assert!(raw.is_char_boundary(span_end));
        let span = &raw[obj_start..span_end];
        if let Some(text) = extract_text_field_after(span) {
            insert_prefer_longer(out, id, text);
        }
        i = after_key;
    }
}

/// Offset of the next `"123"` map key inside `s`, if any (char-boundary offset).
fn find_next_numeric_map_key_offset(s: &str) -> Option<usize> {
    let mut i = 1; // skip the opening `{` we are already inside when possible
    while i < s.len() {
        if !s.is_char_boundary(i) {
            i += 1;
            continue;
        }
        if s.as_bytes().get(i) != Some(&b'"') {
            i += s[i..].chars().next().map(|c| c.len_utf8()).unwrap_or(1);
            continue;
        }
        let rest = &s[i + 1..];
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if !digits.is_empty() && rest.as_bytes().get(digits.len()) == Some(&b'"') {
            // Avoid matching the opening of the current object content — require `:` soon after
            let after = &rest[digits.len() + 1..];
            let trimmed = after.trim_start();
            if trimmed.starts_with(':') {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

fn insert_prefer_longer(out: &mut HashMap<usize, String>, id: usize, text: String) {
    match out.get(&id) {
        Some(prev) if prev.len() > text.len() => {}
        _ => {
            out.insert(id, text);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_complete_items_from_partial_array() {
        let raw = r#"{"translations":[{"id":1,"text":"Hello"},{"id":2,"text":"Wor"#;
        let map = extract_partial_translations(raw);
        assert_eq!(map.get(&1).map(String::as_str), Some("Hello"));
        assert_eq!(map.get(&2).map(String::as_str), Some("Wor"));
    }

    #[test]
    fn grows_open_text_field() {
        let a = extract_partial_translations(r#"{"translations":[{"id":1,"text":"H"#);
        assert_eq!(a.get(&1).map(String::as_str), Some("H"));
        let b = extract_partial_translations(r#"{"translations":[{"id":1,"text":"Hello wor"#);
        assert_eq!(b.get(&1).map(String::as_str), Some("Hello wor"));
    }

    #[test]
    fn empty_input_returns_empty_map() {
        assert!(extract_partial_translations("").is_empty());
        assert!(extract_partial_translations("{").is_empty());
    }

    #[test]
    fn handles_escaped_quotes_in_closed_string() {
        let raw = r#"{"translations":[{"id":1,"text":"say \"hi\""},{"id":2,"text":"x"#;
        let map = extract_partial_translations(raw);
        assert_eq!(map.get(&1).map(String::as_str), Some(r#"say "hi""#));
        assert_eq!(map.get(&2).map(String::as_str), Some("x"));
    }

    #[test]
    fn chinese_long_text_full_recovery_no_panic() {
        // Root cause regression: old code used `i + 500` byte windows and
        // panicked mid multi-byte CJK. New design has no artificial byte cap.
        let mut text = String::new();
        for _ in 0..200 {
            text.push('做');
        }
        let raw = format!(
            r#"{{"translations":[{{"id":1,"text":"{text}"}},{{"id":2,"text":"自告"#
        );
        let map = extract_partial_translations(&raw);
        assert_eq!(map.get(&1).map(|s| s.chars().count()), Some(200));
        assert_eq!(map.get(&2).map(String::as_str), Some("自告"));
    }

    #[test]
    fn chinese_only_stream_prefix_grows() {
        let raw = r#"{"translations":[{"id":1,"text":"你好世界这是一段"#;
        let map = extract_partial_translations(raw);
        assert_eq!(map.get(&1).map(String::as_str), Some("你好世界这是一段"));
    }
}
