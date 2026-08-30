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
    style_guide: &str,
    prev_lines: &[String],
    current_lines: &[TranslationPromptLine],
    next_lines: &[String],
    terms: &[TranslationPromptTerm],
    established_names: &[TranslationNameExample],
) -> String {
    let mut instruction = String::from(
        "Translate currentLines into targetLanguage. currentLines are consecutive subtitle cues from one recording, in order — not independent sentences. Write faithful, fluent, idiomatic targetLanguage: grammar, word order, and convention follow that language, not a calque of the source. previousLines may be \"source → translation\" pairs and nextLines are upcoming source; use them only as context. Return JSON only as {\"translations\":[{\"id\":1,\"text\":\"...\"}]} with every currentLines id in order. Each id is a display slot: fill it with the portion of the natural translation that belongs on screen with that cue. Never merge, split, or drop ids, and do not leave an id empty. Concatenating the translations in order should read as natural targetLanguage for the passage.",
    );
    if !terms.is_empty() {
        instruction.push_str(
            " If a line contains a terminology source, use that target verbatim.",
        );
    }
    if !established_names.is_empty() {
        instruction.push_str(" Reuse establishedNames renderings for those names.");
    }
    if !style_guide.trim().is_empty() {
        instruction.push_str(" Follow styleGuide for tone, register, and how to treat names.");
    }
    let mut obj = serde_json::json!({
        "sourceLanguage": source_lang,
        "targetLanguage": target_lang,
        "previousLines": prev_lines,
        "currentLines": current_lines,
        "nextLines": next_lines,
        "instruction": instruction,
    });
    if !style_guide.trim().is_empty() {
        obj["styleGuide"] = serde_json::Value::String(style_guide.to_string());
    }
    if !terms.is_empty() {
        obj["terminology"] = serde_json::to_value(terms).unwrap_or_default();
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
    fn empty_style_guide_omits_field_and_instruction() {
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
            parsed.get("styleGuide").is_none(),
            "empty style_guide must not send a styleGuide field"
        );
        assert!(parsed.get("background").is_none());
        assert!(parsed.get("output").is_none(), "do not teach an output wrapper");
        assert!(parsed.get("constraints").is_none());
        assert!(parsed.get("terminology").is_none());
        let instruction = parsed["instruction"].as_str().unwrap();
        assert!(
            !instruction.contains("style guide"),
            "must not mention a style guide that does not exist"
        );
        assert!(
            instruction.contains("source → translation"),
            "bilingual previousLines remain the consistency channel"
        );
        assert!(
            instruction.contains("targetLanguage"),
            "output-language lock must stay in the instruction"
        );
        assert!(
            instruction.contains("idiomatic targetLanguage"),
            "quality bar is idiomatic target language, not a source calque"
        );
        assert!(parsed.get("establishedNames").is_none());
        assert_eq!(parsed["currentLines"][0]["text"], "hello");
        assert!(
            instruction.contains("consecutive subtitle cues"),
            "batch is one passage of cues, not isolated sentences"
        );
        assert!(
            instruction.contains("display slot"),
            "each id is a display slot of the natural translation"
        );
    }

    #[test]
    fn style_guide_is_named_and_instructed() {
        let prompt = build_batch_translate_prompt(
            "en",
            "zh",
            "Keep English names. Informal spoken register.",
            &[],
            &sample_lines(),
            &[],
            &[],
            &[],
        );
        let parsed: serde_json::Value = serde_json::from_str(&prompt).unwrap();
        assert_eq!(
            parsed["styleGuide"],
            "Keep English names. Informal spoken register."
        );
        let instruction = parsed["instruction"].as_str().unwrap();
        assert!(instruction.contains("styleGuide"));
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

    #[test]
    fn batch_is_instructed_as_a_passage_of_cues() {
        let prompt = build_batch_translate_prompt(
            "en",
            "zh-CN",
            "",
            &[],
            &sample_lines(),
            &[],
            &[],
            &[],
        );
        let parsed: serde_json::Value = serde_json::from_str(&prompt).unwrap();
        let instruction = parsed["instruction"].as_str().unwrap();
        assert!(instruction.contains("not independent sentences"));
        assert!(instruction.contains("not a calque of the source"));
        assert!(instruction.contains("display slot"));
    }
}

