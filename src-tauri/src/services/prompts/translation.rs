use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct TranslationPromptLine {
    pub id: usize,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TranslationPromptTerm {
    pub source: String,
    pub target: String,
    pub note: String,
}

pub fn build_batch_translate_prompt(
    source_lang: &str,
    target_lang: &str,
    theme_summary: &str,
    prev_lines: &[String],
    current_lines: &[TranslationPromptLine],
    next_lines: &[String],
    terms: &[TranslationPromptTerm],
) -> String {
    serde_json::json!({
        "task": "translate_segment_batch_with_context",
        "rule": "Return JSON only.",
        "sourceLanguage": source_lang,
        "targetLanguage": target_lang,
        "theme": theme_summary,
        "context": {
            "previousLines": prev_lines,
            "currentLines": current_lines,
            "nextLines": next_lines,
        },
        "terminology": terms,
        "constraints": [
            "Translate only currentLines.",
            "Preserve batch-local line id (1..N).",
            "Keep meaning faithful and natural.",
            "Apply provided terminology when relevant.",
            "Prefer one translation line per input line.",
            "No extra explanations."
        ],
        "output": {
            "translations": [
                { "id": 1, "text": "translated text" }
            ]
        }
    })
    .to_string()
}
