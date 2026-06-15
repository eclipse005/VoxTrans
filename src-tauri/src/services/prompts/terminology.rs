use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexedUserTermPromptItem {
    pub index: usize,
    pub source: String,
    pub target: String,
    pub note: String,
}

/// Build the per-window briefing prompt for Step3. One call per transcript
/// window produces a partial briefing (glossary + style guide); the caller
/// unions glossaries across windows and keeps the first window's style guide.
///
/// Open-ended by design: the model adapts what it extracts to the content
/// (names for a drama, abbreviations for finance, definitions for a lecture)
/// without any domain hard-coding. `user_terms` are authoritative; the model
/// only extracts NEW terms beyond them.
pub fn build_briefing_prompt(
    source_lang: &str,
    target_lang: &str,
    transcript_window: &str,
    user_terms: &[IndexedUserTermPromptItem],
) -> String {
    let default = serde_json::json!({
        "task": "build_translation_briefing",
        "rule": "Return JSON only.",
        "sourceLanguage": source_lang,
        "targetLanguage": target_lang,
        "transcript": transcript_window,
        "userTerms": user_terms,
        "goal": "Produce a briefing (style guide + glossary) that keeps this video's translation consistent and fluent across batches. The style guide drives the translator's decisions.",
        "extraction": {
            "glossary": "Anything needing ONE consistent translation: names, proper nouns, domain terms, abbreviations/acronyms, recurring fixed phrases. Adapt to THIS content; do not force fields that do not apply. Do NOT repeat entries already covered by userTerms.",
            "styleGuide": "Write styleGuide as ONE plain string (not an object): a few sentences of free-form guidance for the translator. Touch on whatever matters for THIS content — tone, how to handle abbreviations and names, number/currency conventions, pronoun clarity, readability — woven into flowing text. Empty string if nothing notable. Never use an object/map with keys like registerTone, abbreviationHandling, namingConvention, etc."
        },
        "constraints": {
            "abbreviations": "For each abbreviation/acronym, DECIDE in target: preserve the source form (target == source) when that is the field convention, OR give the standard translation. Do not auto-expand abbreviations into long phrases unless that is the convention.",
            "userTermsAuthority": "userTerms are AUTHORITATIVE: keep their source->target exactly and never change an abbreviation userTerm's target. Extract only NEW terms beyond userTerms.",
            "scope": "Only terms/notes relevant to this transcript window. Avoid generic filler, full clauses, and long fragments."
        },
        "output": {
            "glossary": [
                {
                    "source": "term in source language",
                    "target": "preserved source form OR standard translation",
                    "note": "optional short context"
                }
            ],
            "styleGuide": "Casual, direct teaching tone. Keep domain jargon (e.g. OB, FVG) in English; translate concept terms. Render timeframes as 月图/日图/4小时图. Add dropped subjects (你们/我们) where English omits them. Prefer short punchy lines."
        }
    })
    .to_string();
    default
}
