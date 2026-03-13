use serde_json::Value;

pub fn parse_llm_json_response(raw: &str) -> Result<Value, String> {
    let mut candidates = Vec::new();
    let trimmed = raw.trim();
    if !trimmed.is_empty() {
        candidates.push(trimmed.to_string());
    }

    for block in extract_fenced_blocks(raw) {
        if !block.trim().is_empty() {
            candidates.push(block);
        }
    }

    for slice in extract_all_balanced(raw, '{', '}') {
        candidates.push(slice);
    }
    for slice in extract_all_balanced(raw, '[', ']') {
        candidates.push(slice);
    }

    for candidate in candidates {
        if let Ok(value) = parse_candidate(&candidate) {
            return Ok(value);
        }
    }

    Err(format!(
        "failed to parse llm json response: {}",
        truncate_for_error(raw, 2000)
    ))
}

fn parse_candidate(candidate: &str) -> Result<Value, serde_json::Error> {
    if let Ok(value) = serde_json::from_str::<Value>(candidate) {
        return Ok(value);
    }
    let normalized = remove_trailing_commas_outside_strings(candidate);
    if normalized != candidate {
        if let Ok(value) = serde_json::from_str::<Value>(&normalized) {
            return Ok(value);
        }
    }
    serde_json::from_str::<Value>(candidate)
}

fn extract_fenced_blocks(raw: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cursor = 0usize;
    while let Some(start_rel) = raw[cursor..].find("```") {
        let start = cursor + start_rel;
        let after_start = start + 3;
        let Some(end_rel) = raw[after_start..].find("```") else {
            break;
        };
        let end = after_start + end_rel;
        let mut body = raw[after_start..end].trim().to_string();
        if body.to_ascii_lowercase().starts_with("json") {
            body = body[4..].trim().to_string();
        }
        if !body.is_empty() {
            out.push(body);
        }
        cursor = end + 3;
    }
    out
}

fn extract_all_balanced(raw: &str, start_ch: char, end_ch: char) -> Vec<String> {
    let mut out = Vec::new();
    for (start, ch) in raw.char_indices() {
        if ch != start_ch {
            continue;
        }
        if let Some(end) = find_balanced_end(raw, start, start_ch, end_ch) {
            out.push(raw[start..end].to_string());
        }
    }
    out
}

fn find_balanced_end(raw: &str, start: usize, start_ch: char, end_ch: char) -> Option<usize> {
    let mut depth: i32 = 0;
    let mut in_str = false;
    let mut escaped = false;
    for (offset, ch) in raw[start..].char_indices() {
        if in_str {
            if escaped {
                escaped = false;
                continue;
            }
            if ch == '\\' {
                escaped = true;
                continue;
            }
            if ch == '"' {
                in_str = false;
            }
            continue;
        }
        if ch == '"' {
            in_str = true;
            continue;
        }
        if ch == start_ch {
            depth += 1;
            continue;
        }
        if ch == end_ch {
            depth -= 1;
            if depth == 0 {
                return Some(start + offset + ch.len_utf8());
            }
        }
    }
    None
}

fn remove_trailing_commas_outside_strings(input: &str) -> String {
    let chars = input.chars().collect::<Vec<_>>();
    let mut out = String::with_capacity(input.len());
    let mut i = 0usize;
    let mut in_str = false;
    let mut escaped = false;
    while i < chars.len() {
        let ch = chars[i];
        if in_str {
            out.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_str = false;
            }
            i += 1;
            continue;
        }
        if ch == '"' {
            in_str = true;
            out.push(ch);
            i += 1;
            continue;
        }
        if ch == ',' {
            let mut j = i + 1;
            while j < chars.len() && chars[j].is_whitespace() {
                j += 1;
            }
            if j < chars.len() && (chars[j] == '}' || chars[j] == ']') {
                i += 1;
                continue;
            }
        }
        out.push(ch);
        i += 1;
    }
    out
}

fn truncate_for_error(raw: &str, max_chars: usize) -> String {
    let text = raw.trim();
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    text.chars().take(max_chars).collect::<String>() + "...(truncated)"
}
