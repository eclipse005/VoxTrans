use serde_json::Value;

use super::error::{LlmError, LlmErrorKind};

#[derive(Debug, Clone)]
pub struct JsonResponseValidator {
    pub required_top_level_keys: Vec<String>,
}

impl JsonResponseValidator {
    pub fn with_required_keys(keys: &[&str]) -> Self {
        Self {
            required_top_level_keys: keys.iter().map(|s| (*s).to_string()).collect(),
        }
    }

    pub fn validate(&self, value: &Value) -> Result<(), LlmError> {
        let obj = value.as_object().ok_or_else(|| {
            LlmError::new(
                LlmErrorKind::InvalidSchema,
                "schema check failed: root JSON is not object",
            )
        })?;
        for key in &self.required_top_level_keys {
            if !obj.contains_key(key) {
                return Err(LlmError::new(
                    LlmErrorKind::InvalidSchema,
                    format!("schema check failed: missing key `{key}`"),
                ));
            }
        }
        Ok(())
    }
}

pub fn extract_and_repair_json(raw: &str) -> Result<Value, LlmError> {
    let mut candidates: Vec<String> = Vec::new();
    let mut parse_failures: Vec<String> = Vec::new();
    let trimmed = raw.trim();
    if !trimmed.is_empty() {
        candidates.push(trimmed.to_string());
    }

    // 剔除 <thought> 标签后重试
    if let Some(stripped) = strip_thought_blocks(trimmed) {
        if !stripped.is_empty() {
            candidates.push(stripped);
        }
    }

    if let Some(fenced) = extract_fenced_json(trimmed) {
        candidates.push(fenced);
    }
    if let Some(balanced) = extract_first_balanced_json(trimmed) {
        candidates.push(balanced);
    }

    for candidate in candidates {
        match serde_json::from_str::<Value>(&candidate) {
            Ok(value) => return Ok(value),
            Err(err) => parse_failures.push(format!(
                "candidate parse failed: {}",
                summarize_parse_error(&candidate, &err)
            )),
        }

        let repaired = repair_common_json_issues(&candidate);
        match serde_json::from_str::<Value>(&repaired) {
            Ok(value) => return Ok(value),
            Err(err) => parse_failures.push(format!(
                "repaired candidate parse failed: {}",
                summarize_parse_error(&repaired, &err)
            )),
        }
    }

    let failure_detail = if parse_failures.is_empty() {
        "no JSON candidate found in response".to_string()
    } else {
        dedup_preserve_order(parse_failures).join(" | ")
    };

    Err(LlmError::new(
        LlmErrorKind::InvalidJson,
        format!(
            "failed to extract valid JSON from LLM response: {}; raw preview: {}",
            failure_detail,
            preview_text(trimmed, 240)
        ),
    ))
}

fn preview_text(text: &str, max_chars: usize) -> String {
    if text.is_empty() {
        return "<empty>".to_string();
    }
    let normalized = text.replace('\r', "\\r").replace('\n', "\\n");
    let mut out = String::new();
    let mut count = 0usize;
    for ch in normalized.chars() {
        if count >= max_chars {
            out.push_str("...");
            break;
        }
        out.push(ch);
        count += 1;
    }
    out
}

fn summarize_parse_error(input: &str, err: &serde_json::Error) -> String {
    let reason = trim_serde_json_error_message(&err.to_string());
    let near = snippet_near_error(input, err.line(), err.column(), 48);
    format!("{reason}; near: {near}")
}

fn trim_serde_json_error_message(message: &str) -> String {
    if let Some((head, _)) = message.split_once(" at line ") {
        return head.trim().to_string();
    }
    message.trim().to_string()
}

fn snippet_near_error(input: &str, line: usize, column: usize, radius: usize) -> String {
    let offset = line_col_to_char_offset(input, line, column).unwrap_or(0);
    let chars: Vec<char> = input.chars().collect();
    if chars.is_empty() {
        return "<empty>".to_string();
    }

    let start = offset.saturating_sub(radius);
    let end = (offset + radius).min(chars.len());
    let mut snippet: String = chars[start..end].iter().collect();
    snippet = snippet.replace('\r', "\\r").replace('\n', "\\n");
    if start > 0 {
        snippet = format!("...{snippet}");
    }
    if end < chars.len() {
        snippet.push_str("...");
    }
    snippet
}

fn line_col_to_char_offset(input: &str, line: usize, column: usize) -> Option<usize> {
    if line == 0 || column == 0 {
        return None;
    }

    let mut current_line = 1usize;
    let mut current_col = 1usize;

    for (index, ch) in input.chars().enumerate() {
        if current_line == line && current_col == column {
            return Some(index);
        }
        if ch == '\n' {
            current_line += 1;
            current_col = 1;
        } else {
            current_col += 1;
        }
    }

    if current_line == line && current_col == column {
        Some(input.chars().count())
    } else {
        None
    }
}

fn dedup_preserve_order(items: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for item in items {
        if !out.iter().any(|existing| existing == &item) {
            out.push(item);
        }
    }
    out
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

fn repair_common_json_issues(input: &str) -> String {
    let mut out = input
        .replace('\u{feff}', "")
        .replace('“', "\"")
        .replace('”', "\"")
        .replace('‘', "'")
        .replace('’', "'");

    while out.contains(",}") || out.contains(",]") {
        out = out.replace(",}", "}").replace(",]", "]");
    }

    out.trim().to_string()
}

/// 移除 <thought>...</thought> 块，避免思考过程干扰 JSON 提取
fn strip_thought_blocks(text: &str) -> Option<String> {
    let mut result = String::new();
    let mut remaining = text;
    let mut found_any = false;

    while let Some(start) = remaining.find("<thought>") {
        found_any = true;
        // 添加 <thought> 之前的内容
        result.push_str(&remaining[..start]);

        // 查找 </thought> 结束标签
        if let Some(end) = remaining[start..].find("</thought>") {
            // 跳过整个 <thought>...</thought> 块
            remaining = &remaining[start + end + 10..];
        } else {
            // 没有闭合标签，移除从 <thought> 开始的所有内容
            remaining = "";
            break;
        }
    }

    // 添加剩余部分
    result.push_str(remaining);

    if found_any {
        Some(result.trim().to_string())
    } else {
        None
    }
}
