/// Dedicated reshape pass after a translation batch fails semantic checks.
/// Sends source lines + the candidate JSON so the model can return a valid
/// `translations` array without redoing the whole translation prompt.
pub fn build_semantic_reshape_prompt(
    original_task_prompt: &str,
    retry_hint: &str,
    candidate: Option<&str>,
) -> String {
    let parsed = serde_json::from_str::<serde_json::Value>(original_task_prompt).ok();
    let current_lines = parsed
        .as_ref()
        .and_then(|v| {
            v.get("currentLines")
                .or_else(|| v.pointer("/context/currentLines"))
        })
        .cloned()
        .unwrap_or_else(|| serde_json::json!([]));
    let source_language = parsed
        .as_ref()
        .and_then(|v| v.get("sourceLanguage"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let target_language = parsed
        .as_ref()
        .and_then(|v| v.get("targetLanguage"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let candidate_value = candidate
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| {
            serde_json::from_str::<serde_json::Value>(s)
                .unwrap_or_else(|_| serde_json::Value::String(s.to_string()))
        })
        .unwrap_or(serde_json::Value::Null);

    serde_json::json!({
        "sourceLanguage": source_language,
        "targetLanguage": target_language,
        "currentLines": current_lines,
        "candidate": candidate_value,
        "problem": retry_hint,
        "instruction": "Fix candidate into {\"translations\":[{\"id\":1,\"text\":\"...\"}]} with every currentLines id in order. Reuse existing texts. Translate only missing or empty ids. JSON only, no wrapper.",
    })
    .to_string()
}

pub fn build_retry_constrained_prompt(
    base_user_prompt: &str,
    attempt: u32,
    max_attempts: u32,
    retry_hint: &str,
    previous_output: Option<&str>,
) -> String {
    let mut out = format!(
        "{base_user_prompt}\n\n# Retry Constraint\nPrevious output validation failed ({attempt}/{max_attempts}): {retry_hint}\n"
    );

    if let Some(prev) = previous_output.map(str::trim).filter(|s| !s.is_empty()) {
        out.push_str("\n## Previous incomplete output\n");
        out.push_str(prev);
        out.push('\n');
    }

    out.push_str(
        "\n## Requirements\n\
         - Return the FULL batch as one valid JSON object using the original output schema.\n\
         - Include every expected id with a non-empty translation.\n\
         - Do not return only the missing or corrected items; resubmit the complete set.\n\
         - Do not add markdown fences, explanations, or extra text outside the JSON.\n",
    );
    out
}

pub fn build_json_repair_prompt(
    original_prompt: &str,
    schema_constraints: &str,
    failure_hint: &str,
    raw_text: &str,
) -> String {
    format!(
        concat!(
            "You are a JSON repair tool.\n",
            "Your job is to repair the candidate response into valid JSON without redoing the task.\n",
            "Preserve the original intent and fields whenever possible.\n",
            "Do not add explanations, markdown fences, or commentary.\n",
            "If the candidate is partially valid, minimally fix it.\n",
            "If a field is missing but can be copied from the candidate, keep it.\n",
            "If something cannot be inferred, use the smallest safe JSON value instead of inventing extra content.\n\n",
            "Target constraints:\n",
            "{schema_constraints}\n\n",
            "Validation failure:\n",
            "{failure_hint}\n\n",
            "Original task prompt:\n",
            "{original_prompt}\n\n",
            "Candidate response to repair:\n",
            "{raw_text}\n\n",
            "Return repaired JSON only."
        ),
        schema_constraints = schema_constraints,
        failure_hint = failure_hint,
        original_prompt = original_prompt,
        raw_text = raw_text,
    )
}

#[cfg(test)]
mod tests {
    use super::{build_retry_constrained_prompt, build_semantic_reshape_prompt};

    #[test]
    fn retry_prompt_includes_hint_previous_output_and_full_batch_requirements() {
        let prompt = build_retry_constrained_prompt(
            "BASE_TASK",
            2,
            4,
            "missing ids [3,5]; got ids [1,2,4]; expected 5 items",
            Some("{\"translations\":[{\"id\":1,\"text\":\"a\"}]}"),
        );

        assert!(prompt.starts_with("BASE_TASK"));
        assert!(prompt.contains("Previous output validation failed (2/4)"));
        assert!(prompt.contains("missing ids [3,5]"));
        assert!(prompt.contains("## Previous incomplete output"));
        assert!(prompt.contains("\"id\":1"));
        assert!(prompt.contains("Return the FULL batch"));
        assert!(prompt.contains("Do not return only the missing"));
    }

    #[test]
    fn retry_prompt_omits_previous_section_when_empty() {
        let prompt = build_retry_constrained_prompt("BASE", 2, 3, "empty ids [1]", None);
        assert!(!prompt.contains("## Previous incomplete output"));
        assert!(prompt.contains("empty ids [1]"));
    }

    #[test]
    fn reshape_prompt_sends_sources_and_candidate_not_the_full_translate_task() {
        let original = serde_json::json!({
            "task": "translate_segment_batch_with_context",
            "sourceLanguage": "en",
            "targetLanguage": "zh-CN",
            "context": {
                "currentLines": [
                    { "id": 1, "text": "Hello" },
                    { "id": 2, "text": "World" }
                ]
            }
        })
        .to_string();
        let candidate = r#"{"output":{"translations":[{"id":1,"text":"你好"}]}}"#;
        let prompt = build_semantic_reshape_prompt(&original, "missing ids [2]", Some(candidate));
        let parsed: serde_json::Value = serde_json::from_str(&prompt).unwrap();
        assert_eq!(parsed["currentLines"][1]["text"], "World");
        assert_eq!(parsed["problem"], "missing ids [2]");
        assert_eq!(parsed["candidate"]["output"]["translations"][0]["text"], "你好");
        assert!(parsed["instruction"].as_str().unwrap().contains("Reuse"));
        assert!(!prompt.contains("translate_segment_batch_with_context"));
        assert!(parsed.get("output").is_none());
        assert!(parsed.get("expectedResponse").is_none());
        assert!(parsed.get("constraints").is_none());
    }
}
