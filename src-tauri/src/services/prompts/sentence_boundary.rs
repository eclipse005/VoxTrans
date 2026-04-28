use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SemanticBoundaryPromptCandidate {
    pub id: usize,
    pub split_after_token: usize,
    pub left_preview: String,
    pub right_preview: String,
    pub reason: String,
}

pub fn build_semantic_refinement_prompt(
    source_lang: &str,
    source_text: &str,
    word_limit: usize,
    desired_parts: usize,
    candidates: &[SemanticBoundaryPromptCandidate],
) -> String {
    serde_json::json!({
        "task": "refine_long_asr_sentence_boundaries_for_translation",
        "rule": "Think internally, but output JSON only.",
        "sourceLanguage": source_lang,
        "sourceText": source_text,
        "preferredParts": desired_parts,
        "softMaxWordsPerPart": word_limit,
        "candidateBoundaries": candidates,
        "constraints": [
            "Pick only ids from candidateBoundaries.",
            "Return breakIds in reading order.",
            "Split long ASR text into semantically complete translation units.",
            "Prefer likely missing punctuation and clause boundaries.",
            "Avoid fragments that start or end with dangling function words.",
            "Do not rewrite, translate, or add text."
        ],
        "output": {
            "breakIds": [1, 2]
        }
    })
    .to_string()
}
