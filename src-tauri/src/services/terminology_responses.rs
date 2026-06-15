use serde::{Deserialize, Serialize};

use crate::services::llm::client::LlmSemanticValidationError;

use super::terminology::TerminologyEntry;

/// Parsed per-window briefing: the glossary terms extracted from this window
/// and (for the first window) the style guide that drives the translator.
#[derive(Clone)]
pub(super) struct BriefingResponse {
    pub glossary: Vec<TerminologyEntry>,
    pub style_guide: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct BriefingExtraction {
    #[serde(default)]
    glossary: Vec<TerminologyEntryExtraction>,
    #[serde(default, alias = "style_guide")]
    style_guide: serde_json::Value,
    #[serde(default)]
    output: Option<BriefingOutputExtraction>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct BriefingOutputExtraction {
    #[serde(default)]
    glossary: Vec<TerminologyEntryExtraction>,
    #[serde(default, alias = "style_guide")]
    style_guide: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct TerminologyEntryExtraction {
    #[serde(default)]
    source: String,
    #[serde(default)]
    target: String,
    #[serde(default)]
    note: String,
}

pub(super) fn parse_briefing_response(
    value: serde_json::Value,
) -> Result<BriefingResponse, LlmSemanticValidationError> {
    let result = serde_json::from_value::<BriefingExtraction>(value).map_err(|err| {
        LlmSemanticValidationError::retryable(format!("briefing parse failed: {err}"))
    })?;

    let glossary = if result.glossary.is_empty() {
        result
            .output
            .as_ref()
            .map(|item| item.glossary.clone())
            .unwrap_or_default()
    } else {
        result.glossary
    };
    let style_guide = if normalize_style_guide(&result.style_guide).is_empty() {
        result
            .output
            .as_ref()
            .map(|item| normalize_style_guide(&item.style_guide))
            .unwrap_or_default()
    } else {
        normalize_style_guide(&result.style_guide)
    };

    let glossary = glossary
        .into_iter()
        .map(|entry| TerminologyEntry {
            source: entry.source,
            target: entry.target,
            note: entry.note,
        })
        .collect();

    Ok(BriefingResponse {
        glossary,
        style_guide: style_guide.trim().to_string(),
    })
}

/// Normalize the `styleGuide` field into a single string regardless of how the
/// model encoded it.
///
/// The prompt asks for a plain string, but capable models routinely return a
/// structured object (e.g. `{"registerTone": "...", "naming": "..."}`). Both
/// forms carry identical semantics for the downstream translator, so we coerce
/// them into the flat string Step4 expects instead of rejecting the call:
///   - string  -> trimmed as-is
///   - object  -> `"key: value"` per non-empty entry, joined by newlines
///                (keys sorted for deterministic output)
///   - null/other -> empty string
fn normalize_style_guide(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let mut lines = Vec::new();
            for key in keys {
                if let Some(text) = map.get(key).and_then(non_empty_string) {
                    lines.push(format!("{key}: {text}"));
                }
            }
            lines.join("\n")
        }
        _ => String::new(),
    }
}

/// Extract a trimmed, non-empty string from a JSON value; returns None for
/// null, empty, or whitespace-only values so they can be skipped.
fn non_empty_string(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(s) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::parse_briefing_response;

    #[test]
    fn style_guide_as_string_is_kept_as_is() {
        let raw = serde_json::json!({
            "glossary": [],
            "styleGuide": "Casual tone. Keep jargon in English."
        });
        let out = parse_briefing_response(raw).expect("parse");
        assert_eq!(out.style_guide, "Casual tone. Keep jargon in English.");
    }

    #[test]
    fn style_guide_as_object_is_flattened_to_lines() {
        // The failure mode observed in production: the model returns a
        // structured object instead of a string. The parser must coerce it,
        // not reject it.
        let raw = serde_json::json!({
            "glossary": [],
            "styleGuide": {
                "registerTone": "formal debate",
                "namingConvention": "use common transliterations"
            }
        });
        let out = parse_briefing_response(raw).expect("parse");
        assert_eq!(
            out.style_guide,
            "namingConvention: use common transliterations\nregisterTone: formal debate"
        );
    }

    #[test]
    fn style_guide_object_drops_empty_values() {
        let raw = serde_json::json!({
            "glossary": [],
            "styleGuide": {
                "registerTone": "casual",
                "namingConvention": "",
                "numbers": "   "
            }
        });
        let out = parse_briefing_response(raw).expect("parse");
        assert_eq!(out.style_guide, "registerTone: casual");
    }

    #[test]
    fn style_guide_null_falls_back_to_output_nested_field() {
        let raw = serde_json::json!({
            "glossary": [],
            "styleGuide": null,
            "output": {
                "styleGuide": "fallback guide"
            }
        });
        let out = parse_briefing_response(raw).expect("parse");
        assert_eq!(out.style_guide, "fallback guide");
    }

    #[test]
    fn glossary_entries_are_preserved_regardless_of_style_guide_shape() {
        let raw = serde_json::json!({
            "glossary": [
                {"source": "OB", "target": "订单块", "note": ""}
            ],
            "styleGuide": {"tone": "teaching"}
        });
        let out = parse_briefing_response(raw).expect("parse");
        assert_eq!(out.glossary.len(), 1);
        assert_eq!(out.glossary[0].source, "OB");
        assert_eq!(out.glossary[0].target, "订单块");
    }
}
