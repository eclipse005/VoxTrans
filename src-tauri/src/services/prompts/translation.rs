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

#[derive(Debug, Clone, Serialize)]
pub struct TranslationNameExample {
    pub source: String,
    pub example_source: String,
    pub example_translation: String,
}

pub fn build_batch_translate_prompt(
    source_lang: &str,
    target_lang: &str,
    theme_summary: &str,
    prev_lines: &[String],
    current_lines: &[TranslationPromptLine],
    next_lines: &[String],
    terms: &[TranslationPromptTerm],
    established_names: &[TranslationNameExample],
) -> String {
    let constraints = vec![
        "STRUCTURAL ALIGNMENT IS NON-NEGOTIABLE: output exactly one translation per currentLines id, in the same order. The ids are an immutable spine.".to_string(),
        "Never merge, split, skip, reorder, or invent ids. One wrong mapping misaligns every following line.".to_string(),
        "Each translation must describe only its own source line; never borrow or shift content from an adjacent line.".to_string(),
        "Translate only currentLines; previousLines and nextLines are context only.".to_string(),
        "nextLines is untranslated source context only. Never copy nextLines (or their meaning) into translations.".to_string(),
        "OUTPUT LANGUAGE: every translation must be in targetLanguage. Do not paraphrase currentLines in the source language.".to_string(),
        "PROPER NOUNS: marked-script spans (katakana names, or Latin names inside CJK text) are names. Transcribe them phonetically or keep them; never translate a name into a common noun. When establishedNames lists a name, reuse that example's rendering.".to_string(),
        "TERMINOLOGY ENFORCEMENT: when a source line contains any term from `terminology`, use that term's target verbatim. Match by meaning and allow spacing, capitalization, and punctuation variants of the term's source form. Do not expand, translate, or paraphrase terms the table already covers, and respect the decisions baked into the table.".to_string(),
        "NATURALNESS: produce fluent, idiomatic target language. Avoid word-for-word calques; do not add information absent from the source.".to_string(),
        "CONTEXT CONSISTENCY: previousLines may contain already-translated pairs formatted as \"source → translation\". When a name, term, or recurring phrase in currentLines already has a rendering there, reuse that established translation. Never translate previousLines or nextLines themselves.".to_string(),
        "No extra explanations.".to_string(),
    ];
    let mut obj = serde_json::json!({
        "task": "translate_segment_batch_with_context",
        "rule": "Return JSON only.",
        "sourceLanguage": source_lang,
        "targetLanguage": target_lang,
        "context": {
            "previousLines": prev_lines,
            "currentLines": current_lines,
            "nextLines": next_lines,
        },
        "terminology": terms,
        "constraints": constraints,
        "output": {
            "translations": [
                { "id": 1, "text": "translated text" }
            ]
        }
    });
    if !theme_summary.trim().is_empty() {
        obj["background"] = serde_json::Value::String(theme_summary.to_string());
    }
    if !established_names.is_empty() {
        obj["establishedNames"] = serde_json::to_value(established_names).unwrap_or_default();
    }
    obj.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_lines() -> Vec<TranslationPromptLine> {
        vec![TranslationPromptLine {
            id: 1,
            text: "hello".to_string(),
        }]
    }

    #[test]
    fn empty_theme_omits_background_and_style_guide() {
        let prompt = build_batch_translate_prompt(
            "en",
            "zh",
            "",
            &[],
            &sample_lines(),
            &[],
            &[],
            &[],
        );
        let parsed: serde_json::Value = serde_json::from_str(&prompt).unwrap();
        assert!(
            parsed.get("background").is_none(),
            "empty theme_summary must not send a background field"
        );
        let constraints = parsed["constraints"].as_array().unwrap();
        assert!(
            !constraints
                .iter()
                .any(|c| c.as_str().unwrap().contains("style guide")),
            "NATURALNESS must not mention a style guide that does not exist"
        );
        assert!(
            constraints.iter().any(|c| {
                c.as_str()
                    .unwrap()
                    .contains("source → translation")
            }),
            "bilingual previousLines remain the consistency channel"
        );
        assert!(
            constraints.iter().any(|c| {
                c.as_str()
                    .unwrap()
                    .contains("every translation must be in targetLanguage")
            }),
            "output-language lock must be in constraints"
        );
        assert!(parsed.get("establishedNames").is_none());
    }

    #[test]
    fn established_names_are_omitted_when_empty_and_emitted_when_present() {
        let names = vec![TranslationNameExample {
            source: "Alpha".to_string(),
            example_source: "meet Alpha".to_string(),
            example_translation: "遇见阿尔法".to_string(),
        }];
        let prompt = build_batch_translate_prompt(
            "en",
            "zh",
            "",
            &[],
            &sample_lines(),
            &[],
            &[],
            &names,
        );
        let parsed: serde_json::Value = serde_json::from_str(&prompt).unwrap();
        assert_eq!(parsed["establishedNames"][0]["source"], "Alpha");
    }
}

