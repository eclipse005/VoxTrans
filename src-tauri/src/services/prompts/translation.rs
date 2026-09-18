use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct TranslationPromptLine {
    pub id: usize,
    pub text: String,
}

/// One display slot of a translation unit: `id` is the global segment id the
/// model must echo back; `text` is the source cue shown on screen.
#[derive(Debug, Clone, Serialize)]
pub struct TranslationPromptSlot {
    pub id: usize,
    pub text: String,
}

/// One translation unit: a complete source sentence (`text`) plus the display
/// slots its translation must be distributed onto.
#[derive(Debug, Clone, Serialize)]
pub struct TranslationPromptUnit {
    pub id: usize,
    pub text: String,
    pub slots: Vec<TranslationPromptSlot>,
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

#[allow(clippy::too_many_arguments)]
pub fn build_batch_translate_prompt(
    source_lang: &str,
    target_lang: &str,
    style_guide: &str,
    prev_units: &[String],
    current_units: &[TranslationPromptUnit],
    next_units: &[String],
    terms: &[TranslationPromptTerm],
    established_names: &[TranslationNameExample],
) -> String {
    let mut instruction = String::from(
        "Translate currentUnits into targetLanguage. Each unit is one complete sentence from consecutive subtitle cues of one recording, in order — the units together form one passage. Write faithful, fluent, idiomatic targetLanguage: grammar, word order, and convention follow that language, not a calque of the source. previousLines may be \"source → translation\" pairs and nextLines are upcoming source; use them only as context. After translating a unit as one natural sentence, distribute that translation across the unit's displaySlots: each slot carries the portion of the translation that belongs on screen with its sourceText. Every unit id and every displaySlot id must appear exactly once, and concatenating a unit's displaySlot texts in slot order must reproduce the unit translation (whitespace and punctuation differences aside). A word of targetLanguage must never be split across two slots: each slot text must begin and end at a word boundary of targetLanguage — punctuation may attach to the word. Never merge, split, or drop ids; never leave a unit or a slot empty. Return JSON only as {\"units\":[{\"id\":1,\"translation\":\"...\",\"slots\":[{\"id\":101,\"text\":\"...\"}]}]}.",
    );
    if !terms.is_empty() {
        instruction.push_str(
            " If a unit contains a terminology source, use that target verbatim.",
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
        "previousLines": prev_units,
        "currentUnits": current_units,
        "nextLines": next_units,
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

    fn unit(id: usize, text: &str, slots: &[(usize, &str)]) -> TranslationPromptUnit {
        TranslationPromptUnit {
            id,
            text: text.to_string(),
            slots: slots
                .iter()
                .map(|(id, text)| TranslationPromptSlot {
                    id: *id,
                    text: text.to_string(),
                })
                .collect(),
        }
    }

    #[test]
    fn empty_style_guide_omits_field_and_instruction() {
        let prompt = build_batch_translate_prompt(
            "en",
            "zh",
            "",
            &[],
            &[unit(1, "Hence this video.", &[(1, "Hence"), (2, "this video.")])],
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
        assert_eq!(parsed["currentUnits"][0]["text"], "Hence this video.");
        assert_eq!(parsed["currentUnits"][0]["slots"][0]["id"], 1);
        assert_eq!(parsed["currentUnits"][0]["slots"][1]["id"], 2);
        assert!(
            instruction.contains("complete sentence"),
            "the unit is a whole sentence, not an isolated cue"
        );
        assert!(
            instruction.contains("displaySlots"),
            "slots are the display surface of the translation"
        );
        assert!(
            instruction.contains("reproduce the unit translation"),
            "slot concatenation must equal the unit translation"
        );
        assert!(
            instruction.contains("never be split across two slots"),
            "word-integrity constraint must be stated"
        );
    }

    #[test]
    fn style_guide_is_named_and_instructed() {
        let prompt = build_batch_translate_prompt(
            "en",
            "zh",
            "Keep English names. Informal spoken register.",
            &[],
            &[unit(1, "hello", &[(1, "hello")])],
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
            &[unit(1, "hello", &[(1, "hello")])],
            &[],
            &[],
            &names,
        );
        let parsed: serde_json::Value = serde_json::from_str(&prompt).unwrap();
        assert_eq!(parsed["establishedNames"][0]["source"], "Alpha");
    }

    #[test]
    fn units_are_instructed_as_one_passage() {
        let prompt = build_batch_translate_prompt(
            "en",
            "zh-CN",
            "",
            &[],
            &[unit(1, "hello", &[(1, "hello")])],
            &[],
            &[],
            &[],
        );
        let parsed: serde_json::Value = serde_json::from_str(&prompt).unwrap();
        let instruction = parsed["instruction"].as_str().unwrap();
        assert!(instruction.contains("one passage"));
        assert!(instruction.contains("not a calque of the source"));
        assert!(instruction.contains("never leave a unit or a slot empty"));
        assert!(
            instruction.contains("{\"units\":"),
            "the response contract must be stated verbatim"
        );
    }
}
