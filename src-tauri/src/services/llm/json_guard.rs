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
            LlmError::new(LlmErrorKind::InvalidSchema, "schema check failed: root JSON is not object")
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
    let trimmed = raw.trim();
    if !trimmed.is_empty() {
        candidates.push(trimmed.to_string());
    }
    if let Some(fenced) = extract_fenced_json(trimmed) {
        candidates.push(fenced);
    }
    if let Some(balanced) = extract_first_balanced_json(trimmed) {
        candidates.push(balanced);
    }

    for candidate in candidates {
        if let Ok(value) = serde_json::from_str::<Value>(&candidate) {
            return Ok(value);
        }

        let repaired = repair_common_json_issues(&candidate);
        if let Ok(value) = serde_json::from_str::<Value>(&repaired) {
            return Ok(value);
        }
    }

    Err(LlmError::new(
        LlmErrorKind::InvalidJson,
        "failed to extract valid JSON from LLM response",
    ))
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

