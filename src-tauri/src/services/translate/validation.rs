use super::types::TranslatePipelineRequest;
use serde_json::Value;
use std::collections::HashMap;

pub fn validate_request(request: &TranslatePipelineRequest) -> Result<(), String> {
    if request.task_id.trim().is_empty() {
        return Err("taskId is required".to_string());
    }
    if request.media_path.trim().is_empty() {
        return Err("mediaPath is required".to_string());
    }
    if request.source_lang.trim().is_empty() {
        return Err("sourceLang is required".to_string());
    }
    if request.target_lang.trim().is_empty() {
        return Err("targetLang is required".to_string());
    }
    if request.tokens.is_empty() {
        return Err("tokens is required".to_string());
    }
    if request.translate_api_key.trim().is_empty() {
        return Err("translateApiKey is required".to_string());
    }
    if request.translate_base_url.trim().is_empty() {
        return Err("translateBaseUrl is required".to_string());
    }
    if request.translate_model.trim().is_empty() {
        return Err("translateModel is required".to_string());
    }
    Ok(())
}

pub fn validate_llm_segments(
    json: &Value,
    expected_indexes: &[usize],
) -> Result<HashMap<usize, String>, String> {
    let arr = json
        .get("segments")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "schema check failed: `segments` must be array".to_string())?;
    if arr.len() != expected_indexes.len() {
        return Err(format!(
            "schema check failed: expected {} segments, got {}",
            expected_indexes.len(),
            arr.len()
        ));
    }

    let mut translated_by_index: HashMap<usize, String> = HashMap::new();
    for item in arr {
        let index = item
            .get("index")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize)
            .ok_or_else(|| "schema check failed: each segment.index must be number".to_string())?;
        let translated_text = item
            .get("translatedText")
            .and_then(|v| v.as_str())
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .ok_or_else(|| {
                format!("schema check failed: translatedText is empty for index {index}")
            })?;

        if translated_by_index.insert(index, translated_text).is_some() {
            return Err(format!("schema check failed: duplicated index {index}"));
        }
    }

    for index in expected_indexes {
        if !translated_by_index.contains_key(index) {
            return Err(format!("schema check failed: missing index {index}"));
        }
    }

    Ok(translated_by_index)
}
