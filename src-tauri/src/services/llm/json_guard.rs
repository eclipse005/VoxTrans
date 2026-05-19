use serde_json::Value;

use super::error::{LlmError, LlmErrorKind};
use super::json_candidates::collect_json_candidates;
pub use super::json_validator::JsonResponseValidator;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsonRepairSource {
    Raw,
    ThoughtStripped,
    FencedJson,
    BalancedJson,
    CommonRepair,
}

impl JsonRepairSource {
    pub fn as_str(self) -> &'static str {
        match self {
            JsonRepairSource::Raw => "raw",
            JsonRepairSource::ThoughtStripped => "thought_stripped",
            JsonRepairSource::FencedJson => "fenced_json",
            JsonRepairSource::BalancedJson => "balanced_json",
            JsonRepairSource::CommonRepair => "common_repair",
        }
    }
}

#[derive(Debug, Clone)]
pub struct JsonRepairOutcome {
    pub value: Value,
    pub source: JsonRepairSource,
}

pub fn extract_and_repair_json_with_outcome(raw: &str) -> Result<JsonRepairOutcome, LlmError> {
    let mut parse_failures: Vec<String> = Vec::new();
    let trimmed = raw.trim();
    let candidates = collect_json_candidates(trimmed);

    for (candidate, source) in candidates {
        match serde_json::from_str::<Value>(&candidate) {
            Ok(value) => return Ok(JsonRepairOutcome { value, source }),
            Err(err) => parse_failures.push(format!(
                "candidate parse failed: {}",
                summarize_parse_error(&candidate, &err)
            )),
        }

        let repaired = repair_common_json_issues(&candidate);
        match serde_json::from_str::<Value>(&repaired) {
            Ok(value) => {
                return Ok(JsonRepairOutcome {
                    value,
                    source: JsonRepairSource::CommonRepair,
                });
            }
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
    for (count, ch) in normalized.chars().enumerate() {
        if count >= max_chars {
            out.push_str("...");
            break;
        }
        out.push(ch);
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

fn repair_common_json_issues(input: &str) -> String {
    let mut out = input
        .replace('\u{feff}', "")
        .replace(['“', '”'], "\"")
        .replace(['‘', '’'], "'");

    while out.contains(",}") || out.contains(",]") {
        out = out.replace(",}", "}").replace(",]", "]");
    }

    out.trim().to_string()
}
