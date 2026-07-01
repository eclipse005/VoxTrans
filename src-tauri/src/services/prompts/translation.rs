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
    visual_context: Option<&str>,
) -> String {
    let mut constraints = vec![
        "STRUCTURAL ALIGNMENT IS NON-NEGOTIABLE: output exactly one translation per currentLines id, in the same order. The ids are an immutable spine.".to_string(),
        "Never merge, split, skip, reorder, or invent ids. One wrong mapping misaligns every following line.".to_string(),
        "Each translation must describe only its own source line; never borrow or shift content from an adjacent line.".to_string(),
        "Translate only currentLines; previousLines and nextLines are context only.".to_string(),
        "TERMINOLOGY ENFORCEMENT: when a source line contains any term from `terminology`, use that term's target verbatim. Match by meaning and allow spacing, capitalization, and punctuation variants of the term's source form. Do not expand, translate, or paraphrase terms the table already covers, and respect the decisions baked into the table.".to_string(),
        "NATURALNESS: produce fluent, idiomatic target language. Follow the style guide in `background`; avoid word-for-word calques; do not add information absent from the source.".to_string(),
        "No extra explanations.".to_string(),
    ];
    if visual_context.is_some() {
        constraints.push(
            "VISUAL EVIDENCE: the attached images are auxiliary evidence sampled from the video range of currentLines. Use them to disambiguate speakers, resolve referents, identify on-screen text/proper nouns, and ground the scene. Do NOT describe the images. Do NOT transcribe image text into the translation. Translate the source text only.".to_string(),
        );
    }
    let mut obj = serde_json::json!({
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
        "constraints": constraints,
        "output": {
            "translations": [
                { "id": 1, "text": "translated text" }
            ]
        }
    });
    if let Some(vc) = visual_context {
        obj["visualContext"] = serde_json::Value::String(vc.to_string());
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
    fn prompt_without_visual_context_omits_field() {
        let prompt = build_batch_translate_prompt(
            "en",
            "zh",
            "theme",
            &[],
            &sample_lines(),
            &[],
            &[],
            None,
        );
        let parsed: serde_json::Value = serde_json::from_str(&prompt).unwrap();
        assert!(
            parsed.get("visualContext").is_none(),
            "visualContext should be absent when None"
        );
        let constraints = parsed["constraints"].as_array().unwrap();
        assert!(
            !constraints.iter().any(|c| c.as_str().unwrap().contains("VISUAL EVIDENCE")),
            "VISUAL EVIDENCE constraint should be absent when None"
        );
    }

    #[test]
    fn prompt_with_visual_context_adds_field_and_constraint() {
        let prompt = build_batch_translate_prompt(
            "en",
            "zh",
            "theme",
            &[],
            &sample_lines(),
            &[],
            &[],
            Some("auxiliary frames attached"),
        );
        let parsed: serde_json::Value = serde_json::from_str(&prompt).unwrap();
        assert_eq!(parsed["visualContext"].as_str().unwrap(), "auxiliary frames attached");
        let constraints = parsed["constraints"].as_array().unwrap();
        assert!(
            constraints
                .iter()
                .any(|c| c.as_str().unwrap().contains("VISUAL EVIDENCE")),
            "VISUAL EVIDENCE constraint should be present when Some"
        );
    }
}

