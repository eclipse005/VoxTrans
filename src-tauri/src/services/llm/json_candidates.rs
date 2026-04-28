use super::json_guard::JsonRepairSource;

pub(super) fn collect_json_candidates(trimmed: &str) -> Vec<(String, JsonRepairSource)> {
    let mut candidates = Vec::new();
    if !trimmed.is_empty() {
        candidates.push((trimmed.to_string(), JsonRepairSource::Raw));
    }

    if let Some(stripped) = strip_thought_blocks(trimmed)
        && !stripped.is_empty()
    {
        candidates.push((stripped, JsonRepairSource::ThoughtStripped));
    }

    if let Some(fenced) = extract_fenced_json(trimmed) {
        candidates.push((fenced, JsonRepairSource::FencedJson));
    }
    if let Some(balanced) = extract_first_balanced_json(trimmed) {
        candidates.push((balanced, JsonRepairSource::BalancedJson));
    }

    candidates
}

fn extract_fenced_json(text: &str) -> Option<String> {
    let start = text.find("```")?;
    let after_start = &text[start + 3..];
    let mut body = after_start;
    if after_start.trim_start().to_lowercase().starts_with("json") {
        let idx = after_start.find('\n')?;
        body = &after_start[idx + 1..];
    }
    let end = body.find("```")?;
    Some(body[..end].trim().to_string())
}

fn extract_first_balanced_json(text: &str) -> Option<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut start_idx: Option<usize> = None;
    let mut stack: Vec<char> = Vec::new();
    let mut in_string = false;
    let mut escaped = false;

    for (i, ch) in chars.iter().enumerate() {
        if in_string {
            if escaped {
                escaped = false;
                continue;
            }
            if *ch == '\\' {
                escaped = true;
                continue;
            }
            if *ch == '"' {
                in_string = false;
            }
            continue;
        }

        match *ch {
            '"' => in_string = true,
            '{' | '[' => {
                if start_idx.is_none() {
                    start_idx = Some(i);
                }
                stack.push(*ch);
            }
            '}' => {
                if stack.pop() != Some('{') {
                    continue;
                }
                if stack.is_empty() {
                    let start = start_idx?;
                    return Some(chars[start..=i].iter().collect::<String>());
                }
            }
            ']' => {
                if stack.pop() != Some('[') {
                    continue;
                }
                if stack.is_empty() {
                    let start = start_idx?;
                    return Some(chars[start..=i].iter().collect::<String>());
                }
            }
            _ => {}
        }
    }

    None
}

fn strip_thought_blocks(text: &str) -> Option<String> {
    let mut result = String::new();
    let mut remaining = text;
    let mut found_any = false;

    while let Some(start) = remaining.find("<thought>") {
        found_any = true;
        result.push_str(&remaining[..start]);

        if let Some(end) = remaining[start..].find("</thought>") {
            remaining = &remaining[start + end + 10..];
        } else {
            remaining = "";
            break;
        }
    }

    result.push_str(remaining);

    if found_any {
        Some(result.trim().to_string())
    } else {
        None
    }
}
