pub fn build_retry_constrained_prompt(
    base_user_prompt: &str,
    attempt: u32,
    max_attempts: u32,
    retry_hint: &str,
) -> String {
    format!(
        "{base_user_prompt}\n\n# Retry Constraint\nPrevious output validation failed ({attempt}/{max_attempts}): {retry_hint}\nReturn a complete corrected JSON response only. Do not add any markdown, explanations, or extra text."
    )
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
