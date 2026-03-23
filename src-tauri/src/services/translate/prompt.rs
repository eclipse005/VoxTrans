use serde::Deserialize;

#[derive(Debug, Clone)]
pub struct PunctuationPromptInput {
    pub previous_text: String,
    pub current_text: String,
    pub next_text: String,
}

pub fn build_punctuation_system_prompt() -> String {
    "You are a subtitle punctuation normalizer. Improve punctuation, capitalization, and spacing only. Do not rewrite meaning. Do not merge neighboring sentences into the current one. Return JSON only in this shape: {\"punctuatedText\":\"...\"}.".to_string()
}

pub fn build_punctuation_user_prompt(input: &PunctuationPromptInput) -> String {
    let payload = serde_json::json!({
        "task": "punctuation_restore",
        "language": "en",
        "context": {
            "previous": input.previous_text,
            "current": input.current_text,
            "next": input.next_text
        },
        "rules": [
            "Focus on context.current only",
            "Do not add or remove semantic content",
            "Do not merge with previous or next",
            "Keep wording as close as possible",
            "Only adjust punctuation, capitalization, and spacing"
        ],
        "output": {
            "json_only": true,
            "schema": { "punctuatedText": "string" }
        }
    });
    payload.to_string()
}

#[derive(Debug, Clone)]
pub struct TranslatePromptSegmentInput {
    pub index: usize,
    pub source_text: String,
}

#[derive(Debug, Clone)]
pub struct TranslatePromptInput {
    pub source_lang: String,
    pub target_lang: String,
    pub previous_context: String,
    pub next_context: String,
    pub theme: String,
    pub terminology_entries: Vec<TranslateTerminologyPromptEntry>,
    pub segments: Vec<TranslatePromptSegmentInput>,
}

#[derive(Debug, Clone)]
pub struct SegmentOptimizePromptInput {
    pub preferred_segment_count: usize,
    pub source_text: String,
    pub translated_text: String,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
pub struct TranslateTerminologyPromptEntry {
    #[serde(default, deserialize_with = "deserialize_string_or_empty")]
    pub source: String,
    #[serde(default, deserialize_with = "deserialize_string_or_empty")]
    pub target: String,
    #[serde(default, deserialize_with = "deserialize_string_or_empty")]
    pub note: String,
}

pub fn build_translate_system_prompt() -> String {
    "You are a professional subtitle translator for streaming-quality subtitles. \
Translate faithfully, naturally, and in culturally appropriate target-language phrasing. \
Preserve intent, tone, register, and key domain terminology. \
Do not add commentary or explanations. \
Output JSON only in this shape: {\"segments\":[{\"index\":1,\"translation\":\"...\"}]}"
        .to_string()
}

pub fn build_segment_optimize_system_prompt() -> String {
    "You are a professional bilingual subtitle splitter. \
Choose the best final split for one overlong subtitle, optimizing for semantic completeness, natural reading rhythm, bilingual watchability, and clean alignment. \
Consider multiple plausible split approaches internally, then return only the best result. \
Return JSON only in this shape: {\"segments\":[{\"origin\":\"...\",\"translation\":\"...\"}],\"reason\":\"...\",\"confidence\":0.0}. \
Return 2 or 3 segments, keep origin and translation counts identical, and do not add commentary."
        .to_string()
}

#[derive(Debug, Clone)]
pub struct TranslateSummaryPromptInput {
    pub source_lang: String,
    pub target_lang: String,
    pub context_head: String,
    pub context_middle: String,
    pub context_tail: String,
    pub terminology_entries: Vec<TranslateTerminologyPromptEntry>,
}

pub fn build_translate_summary_system_prompt() -> String {
    "You are a subtitle translation domain analyst. \
Return JSON only in this shape: {\"theme\":\"...\",\"primaryTerminologyEntries\":[{\"source\":\"...\",\"target\":\"...\",\"note\":\"...\"}],\"supportingTerminologyEntries\":[{\"source\":\"...\",\"target\":\"...\",\"note\":\"...\"}]}. \
Only keep terminology that is relevant to this video's domain."
        .to_string()
}

pub fn build_translate_summary_user_prompt(input: &TranslateSummaryPromptInput) -> String {
    let glossary = input
        .terminology_entries
        .iter()
        .map(terminology_entry_to_json)
        .collect::<Vec<_>>();
    let payload = serde_json::json!({
        "task": "subtitle_translation_summary_planning",
        "sourceLang": input.source_lang,
        "targetLang": input.target_lang,
        "context": {
            "head": input.context_head,
            "middle": input.context_middle,
            "tail": input.context_tail
        },
        "terminology": glossary,
        "requirements": [
            "theme must be exactly 2 sentences in source language: first sentence for main topic, second sentence for key point",
            "Select entries only from provided terminology list; do not invent new terms",
            "For selected entries, keep source/target/note exactly as provided",
            "primaryTerminologyEntries: compact and strict; repeated concepts, main workflow vocabulary, headline entities, and terms central to understanding",
            "supportingTerminologyEntries: broader same-domain terms that are plausibly useful context, especially when ASR may misrecognize terms",
            "Do not duplicate the same term in both groups",
            "Order each list from most relevant to less relevant",
            "Prefer useful recall for supporting terms, but exclude weakly related background terms and generic everyday words"
        ],
        "output": {
            "json_only": true,
            "schema": {
                "theme": "string(two sentences)",
                "primaryTerminologyEntries": [
                    {
                        "source": "string",
                        "target": "string",
                        "note": "string(optional)"
                    }
                ],
                "supportingTerminologyEntries": [
                    {
                        "source": "string",
                        "target": "string",
                        "note": "string(optional)"
                    }
                ]
            }
        }
    });
    payload.to_string()
}

pub fn build_translate_user_prompt(input: &TranslatePromptInput) -> String {
    let segments = input
        .segments
        .iter()
        .map(|segment| {
            serde_json::json!({
                "index": segment.index,
                "origin": segment.source_text,
            })
        })
        .collect::<Vec<_>>();

    let payload = serde_json::json!({
        "task": "subtitle_translate",
        "sourceLang": input.source_lang,
        "targetLang": input.target_lang,
        "globalStrategy": {
            "theme": input.theme
        },
        "context": {
            "previous": input.previous_context,
            "next": input.next_context,
        },
        "terminology": input
            .terminology_entries
            .iter()
            .map(terminology_entry_to_json)
            .collect::<Vec<_>>(),
        "rules": [
            "Translate only origin for each segment",
            "Use local segment index only (1..N in current batch), not global subtitle index",
            "Keep one output for each input index; do not omit, merge, or split segments",
            "Do not move meaning across lines; translate each input line independently",
            "Preserve meaning and speaker intent; do not add facts or commentary",
            "Use natural subtitle phrasing in target language",
            "Conciseness-first: avoid unnecessary expansion; if multiple faithful options exist, prefer the shorter one",
            "Use provided terminology consistently",
            "Use previous/next context only for disambiguation",
            "Return JSON only"
        ],
        "segments": segments,
        "output": {
            "json_only": true,
            "schema": {
                "segments": [
                    {
                        "index": "number",
                        "translation": "string"
                    }
                ]
            }
        }
    });

    payload.to_string()
}

pub fn build_segment_optimize_user_prompt(input: &SegmentOptimizePromptInput) -> String {
    let payload = serde_json::json!({
        "task": "subtitle_segment_optimize",
        "preferredSegmentCount": input.preferred_segment_count,
        "original": {
            "origin": input.source_text,
            "translation": input.translated_text,
        },
        "rules": [
            "Preserve meaning; do not add or remove information",
            "Prefer natural split points such as punctuation, clauses, conjunctions, and semantic pauses",
            "Keep segments natural, watchable, and reasonably balanced",
            "Align origin and translation for comfortable bilingual viewing, but do not force unnatural word-order mirroring",
            "Prefer preferredSegmentCount when natural, but return whichever of 2 or 3 is better",
            "Keep every segment non-empty",
            "Only adjust split boundaries; do not rewrite beyond what is needed for split alignment"
        ],
        "output": {
            "json_only": true,
            "schema": {
                "segments": [
                    {
                        "origin": "string",
                        "translation": "string"
                    }
                ],
                "reason": "string",
                "confidence": "number(0..1)"
            }
        }
    });
    payload.to_string()
}

fn terminology_entry_to_json(entry: &TranslateTerminologyPromptEntry) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    map.insert("source".to_string(), serde_json::Value::String(entry.source.clone()));
    map.insert("target".to_string(), serde_json::Value::String(entry.target.clone()));
    let note = entry.note.trim();
    if !note.is_empty() {
        map.insert("note".to_string(), serde_json::Value::String(note.to_string()));
    }
    serde_json::Value::Object(map)
}

fn deserialize_string_or_empty<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<String>::deserialize(deserializer)?;
    Ok(value.unwrap_or_default())
}
