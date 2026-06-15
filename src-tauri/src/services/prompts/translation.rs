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
    let default = serde_json::json!({
        "task": "translate_segment_batch_with_context",
        "rule": "Return JSON only.",
        "sourceLanguage": source_lang,
        "targetLanguage": target_lang,
        "background": theme_summary,
        "context": {
            "previousLines": prev_lines,
            "currentLines": current_lines,
            "nextLines": next_lines,
        },
        "terminology": terms,
        "constraints": [
            "STRUCTURAL ALIGNMENT IS NON-NEGOTIABLE: output exactly one translation per currentLines id, in the same order. The ids are an immutable spine.",
            "Never merge, split, skip, reorder, or invent ids. One wrong mapping misaligns every following line.",
            "Each translation must describe only its own source line; never borrow or shift content from an adjacent line.",
            "Translate only currentLines; previousLines and nextLines are context only.",
            "TERMINOLOGY ENFORCEMENT: when a source line contains any term from `terminology`, use that term's target verbatim. Match by meaning and allow spacing, capitalization, and punctuation variants of the term's source form. Do not expand, translate, or paraphrase terms the table already covers, and respect the decisions baked into the table.",
            "NATURALNESS: produce fluent, idiomatic target language. Follow the style guide in `background`; avoid word-for-word calques; do not add information absent from the source.",
            "No extra explanations."
        ],
        "output": {
            "translations": [
                { "id": 1, "text": "translated text" }
            ]
        }
    })
    .to_string();
    default
}
