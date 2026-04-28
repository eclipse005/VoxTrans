use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Step5PromptLine {
    pub id: usize,
    pub source: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Step5PromptTerm {
    pub source: String,
    pub target: String,
    pub note: String,
}

pub fn build_source_split_prompt(
    source_lang: &str,
    target_lang: &str,
    source_text: &str,
    draft_translation: &str,
    source_limit: f64,
    target_limit: f64,
    min_parts: usize,
) -> String {
    let expected_parts = min_parts.max(2);
    serde_json::json!({
        "task": "split_source_segment_for_subtitle_alignment",
        "rule": "Think step by step internally, but output JSON only.",
        "sourceLanguage": source_lang,
        "targetLanguage": target_lang,
        "sourceText": source_text,
        "draftTranslation": draft_translation,
        "sourceLengthLimit": source_limit,
        "targetLengthLimit": target_limit,
        "expectedParts": expected_parts,
        "constraints": [
            "Return sourceParts only.",
            "sourceParts must be an array of strings with exactly expectedParts items.",
            "Keep original language and wording. Do not translate.",
            "Do not reorder meaning. Keep sequence from sourceText.",
            "Each part should be semantically complete when possible.",
            "Avoid ultra-short fragments like single discourse markers."
        ],
        "output": {
            "sourceParts": ["part 1", "part 2"]
        }
    })
    .to_string()
}

pub fn build_align_prompt(
    source_lang: &str,
    target_lang: &str,
    theme_summary: &str,
    source_text: &str,
    draft_translation: &str,
    part_sources: &[Step5PromptLine],
    terms: &[Step5PromptTerm],
) -> String {
    serde_json::json!({
        "task": "align_translation_to_split_source_lines",
        "rule": "Return JSON only.",
        "sourceLanguage": source_lang,
        "targetLanguage": target_lang,
        "theme": theme_summary,
        "sourceText": source_text,
        "draftTranslation": draft_translation,
        "splitSourceLines": part_sources,
        "terminology": terms,
        "constraints": [
            "Return exactly one translation line for each split source line id.",
            "Keep meaning faithful and natural.",
            "Do not merge lines.",
            "Do not copy full draftTranslation to multiple ids.",
            "Each id should only contain meaning from its own source line.",
            "If uncertain, keep a shorter partial translation for that line only.",
            "Do not add explanations."
        ],
        "output": {
            "translations": [
                {"id": 1, "text": "translated text"}
            ]
        }
    })
    .to_string()
}

pub fn build_polish_prompt(
    source_lang: &str,
    target_lang: &str,
    source_text: &str,
    translation: &str,
    target_length_soft: f64,
    terms: &[Step5PromptTerm],
) -> String {
    serde_json::json!({
        "task": "polish_single_subtitle_line",
        "rule": "Return JSON only.",
        "sourceLanguage": source_lang,
        "targetLanguage": target_lang,
        "sourceText": source_text,
        "currentTranslation": translation,
        "targetLengthSoft": target_length_soft,
        "terminology": terms,
        "constraints": [
            "Keep one line only.",
            "Keep key meaning.",
            "Prefer shorter wording.",
            "No extra notes."
        ],
        "output": {
            "text": "shorter polished translation"
        }
    })
    .to_string()
}
