use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexedUserTermPromptItem {
    pub index: usize,
    pub source: String,
    pub target: String,
    pub note: String,
}

pub fn build_theme_prompt(source_lang: &str, target_lang: &str, context_text: &str) -> String {
    serde_json::json!({
        "task": "summarize_video_theme_for_terminology",
        "rule": "Return JSON only.",
        "sourceLanguage": source_lang,
        "targetLanguage": target_lang,
        "transcript": context_text,
        "goal": "Summarize the dominant topic and field of this transcript for terminology selection.",
        "output": {
            "theme": "One concise sentence."
        }
    })
    .to_string()
}

pub fn build_user_filter_prompt(
    source_lang: &str,
    target_lang: &str,
    theme: &str,
    context_text: &str,
    terms: &[IndexedUserTermPromptItem],
) -> String {
    serde_json::json!({
        "task": "filter_user_terminology_by_video_relevance",
        "rule": "Return JSON only.",
        "sourceLanguage": source_lang,
        "targetLanguage": target_lang,
        "theme": theme,
        "transcript": context_text,
        "userTerms": terms,
        "goal": "Keep only terms that are relevant to this video's domain and content.",
        "output": {
            "keepIndexes": [1, 2]
        }
    })
    .to_string()
}

pub fn build_extract_terms_prompt(
    source_lang: &str,
    target_lang: &str,
    theme: &str,
    context_text: &str,
    max_terms: usize,
) -> String {
    serde_json::json!({
        "task": "extract_domain_terminology_for_translation_consistency",
        "rule": "Return JSON only.",
        "sourceLanguage": source_lang,
        "targetLanguage": target_lang,
        "theme": theme,
        "transcript": context_text,
        "constraints": {
            "maxTerms": max_terms,
            "focus": "domain terminology, named entities, fixed expressions in this context",
            "avoid": "full clauses, long sentence fragments, generic filler words"
        },
        "output": {
            "terms": [
                {
                    "source": "term in source language",
                    "target": "target translation",
                    "note": "optional short context note"
                }
            ]
        }
    })
    .to_string()
}
